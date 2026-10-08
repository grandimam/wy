from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path
from uuid import uuid4

from wy import history
from wy.engine import analyze
from wy.models import ChangedFile, Decision, Location, Review, SessionRef
from wy.provider import Answer, Provider, enrich
from wy.repository import changed_symbols, committed_sources, compare, head, parse_diff, repo_root, sources
from wy.security import digest, read_source, redact
from wy.storage import Store


def snapshot(root: Path) -> dict:
    root = repo_root(root)
    texts, hashes, warnings = sources(root)
    if any("truncated" in w for w in warnings):
        raise ValueError("Cannot establish a complete baseline: repository context exceeds 12 MB")
    result = {
        "id": "snapshot-" + uuid4().hex[:12],
        "root": str(root),
        "head": head(root),
        "created_at": datetime.now(timezone.utc).isoformat(),
        "texts": texts,
        "hashes": hashes,
        "warnings": warnings,
    }
    Store(root).put("snapshot", result["id"], result)
    return result


def review(
    root: Path,
    session_path: Path | None = None,
    baseline_id: str | None = None,
    base: str | None = None,
    diff_path: Path | None = None,
    provider: Provider | None = None,
    *,
    session_paths: list[Path] | None = None,
    history_source: str = "none",
) -> Review:
    root = repo_root(root)
    if sum(x is not None for x in (baseline_id, base, diff_path)) > 1:
        raise ValueError("Choose only one of --baseline, --base or --diff")
    store = Store(root)
    texts, hashes, warnings = sources(root)
    baseline = None
    if baseline_id:
        baseline = store.get("snapshot", baseline_id)
        if baseline["root"] != str(root):
            raise ValueError("Baseline belongs to another repository")
        changes = compare(baseline["texts"], texts)
        warnings.append(
            "Changes since the baseline are isolated, but authorship is not proven; concurrent human edits may be included."
        )
    elif diff_path:
        if diff_path.stat().st_size > 5_000_000:
            raise ValueError("Diff exceeds the 5 MB input limit")
        changes = parse_diff(diff_path.read_text(encoding="utf-8"))
        # Exact addition checks prevent attaching imported diffs to unrelated current files.
        for change in changes:
            lines = texts.get(change.file, "").splitlines()
            if any(n < 1 or n > len(lines) or lines[n - 1] != value for n, value in change.additions.items()):
                raise ValueError(
                    f"Diff does not match current source: {change.file}; check out its target snapshot"
                )
    else:
        revision = base or head(root)
        before = committed_sources(root, revision) if revision else {}
        changes = compare(before, texts)
    if not baseline:
        warnings.append(
            "No pre-session baseline: existing uncommitted changes cannot be distinguished from agent changes; attribution is unknown."
        )
    if history_source not in history.SOURCES:
        raise ValueError("History source must be codex, claude, both or none")
    paths = list(dict.fromkeys(p.resolve() for p in [*([session_path] if session_path else []), *(session_paths or [])]))
    automatic = not paths and history_source != "none"
    if not automatic and len(paths) > 20:
        raise ValueError("Select at most 20 sessions per review")
    if automatic:
        entries = history.discover(root, history_source)
        # Interleave sources so one prolific agent cannot consume the entire budget.
        queues = [[e for e in entries if e["agent"] == agent] for agent in ("codex", "claude")]
        paths = [Path(queue[i]["path"]) for i in range(max(map(len, queues), default=0))
                 for queue in queues if i < len(queue)]
    sessions, refs, total_bytes = [], [], 0
    for path in paths:
        try:
            size = path.stat().st_size
        except OSError:
            if not automatic:
                raise
            warnings.append(f"Skipped unavailable session: {path}")
            continue
        if automatic and (len(sessions) >= 20 or total_bytes + size > 40_000_000 or size > 20_000_000):
            warnings.append(f"Skipped session due to history budget (20 sessions / 40 MB total): {path}")
            continue
        if not automatic and total_bytes + size > 40_000_000:
            raise ValueError("Selected histories exceed the 40 MB total input limit")
        try:
            session = history.collect(path)
        except (ValueError, OSError) as exc:
            if not automatic:
                raise
            warnings.append(f"Skipped unreadable or invalid session {path}: {redact(str(exc))}")
            continue
        if not history.belongs(session.cwd, root):
            if automatic:
                warnings.append(f"Skipped session whose collected workspace did not match this repository: {path}")
                continue
            raise ValueError("Session working directory does not match this repository or is missing; refusing unrelated or unverified history")
        if history_source in {"codex", "claude"} and session.agent != history_source:
            raise ValueError("Explicit session does not match the selected history source")
        if any(s.agent == session.agent and s.id == session.id for s in sessions):
            continue
        total_bytes += size
        sessions.append(session)
        warnings += [f"{session.agent}:{session.id}: {w}" for w in session.warnings]
    for session in sessions:
        key = f"{session.agent}:{session.id}:{digest(session.model_dump_json())[:12]}"
        store.put("session", key, session.model_dump())
        refs.append(SessionRef(id=session.id, agent=session.agent, path=session.path,
                               cwd=session.cwd, storage_key=key))
    if not sessions:
        warnings.append("No agent history supplied; explanations use repository evidence only.")
    decisions = analyze(changes, texts, hashes, sessions, baseline is not None)
    if len(decisions) == 12:
        warnings.append("Annotation limit reached (12); lower-priority candidates may be omitted.")
    if provider:
        for i, decision in enumerate(decisions):
            try:
                decisions[i] = enrich(decision, provider)
            except ValueError:
                warnings.append(
                    f"Invalid or unavailable model result for {decision.id}; kept conservative offline analysis."
                )
    if not decisions:
        warnings.append(
            "No supported significant-decision patterns found; this does not mean the change has no engineering decisions."
        )
    result = Review(
        id="review-" + uuid4().hex[:12],
        root=str(root),
        created_at=datetime.now(timezone.utc).isoformat(),
        head=head(root),
        baseline_id=baseline_id,
        session_id=sessions[0].id if len(sessions) == 1 else None,
        sessions=refs,
        decisions=decisions,
        changes=[
            ChangedFile(
                file=c.file,
                hunks=[Location(file=c.file, start_line=a, end_line=b) for a, b in c.hunks],
                symbols=changed_symbols(
                    c.file, texts.get(c.file, ""), set(c.additions) | {a for a, _ in c.hunks}
                ),
                added_lines=sorted(c.additions),
                removed_line_count=len(c.removed),
                diff=redact(c.patch)[:40000],
                diff_truncated=len(redact(c.patch)) > 40000,
            )
            for c in changes
        ],
        warnings=warnings,
        file_hashes=hashes,
        provider="ollama" if provider else "offline",
        comparison_base=baseline_id or (f"imported patch: {diff_path}" if diff_path else base or head(root)),
        history_source="selected" if session_path or session_paths else history_source,
        input_tokens=provider.input_tokens if provider else 0,
        output_tokens=provider.output_tokens if provider else 0,
    )
    store.save_review(result)
    return result


def fresh(decision: Decision, root: Path) -> bool:
    files = {decision.location.file: decision.snapshot_hash}
    files.update({e.file: e.snapshot_hash for e in decision.evidence if e.kind == "code"})
    for file, expected in files.items():
        text = read_source(root, file)
        if not expected or text is None or digest(text) != expected:
            return False
    return True


def load_review(root: Path) -> Review:
    root = repo_root(root)
    result = Store(root).latest()
    if result.root != str(root):
        raise ValueError("Cached review belongs to another repository; run wy review again")
    history.saved_sessions(result)
    if result.head != head(root):
        result.warnings.append("This saved review predates the current Git HEAD. Run /review or /reason to inspect current changes.")
    for decision in result.decisions:
        decision.stale = not fresh(decision, root)
    return result


def select(result: Review, target: str) -> Decision:
    if target.isdecimal():
        index = int(target) - 1
        if 0 <= index < len(result.decisions):
            return result.decisions[index]
        raise ValueError("Decision number out of range; run wy decisions for the current index")
    for decision in result.decisions:
        if decision.id == target:
            return decision
    try:
        file, raw_line = target.rsplit(":", 1)
        line = int(raw_line)
    except ValueError as exc:
        raise ValueError("Use a decision number, decision ID or file:line") from exc
    path = Path(file)
    if path.is_absolute():
        try:
            file = str(path.resolve().relative_to(result.root))
        except ValueError as exc:
            raise ValueError("Location is outside this repository") from exc
    matches = [
        d
        for d in result.decisions
        if d.location.file == file and d.location.start_line <= line <= d.location.end_line
    ]
    if not matches:
        raise ValueError("No decision at this location; run wy decisions to see available locations")
    return matches[0]


def ask(decision: Decision, question: str, provider: Provider | None = None) -> Answer:
    if decision.stale:
        raise ValueError("Decision or evidence has changed; run wy review before asking follow-up questions")
    if provider:
        return provider.ask(decision, redact(question))
    q = question.lower()
    if "alternative" in q:
        answer = "Options to investigate (not a record of what the agent considered):\n" + "\n".join(
            decision.alternatives
        )
    elif any(word in q for word in ("assum", "verify", "uncertain", "gap")):
        answer = "\n".join([*decision.assumptions, *decision.unresolved_questions])
    elif "evidence" in q or "support" in q:
        answer = "\n\n".join(f"[{e.id}] {e.file}:{e.start_line}\n{e.excerpt}" for e in decision.evidence)
    elif "convention" in q or "consistent" in q:
        answer = (
            "The retrieved examples are not sufficient to establish a repository-wide convention. Inspect these citations and any competing patterns before deciding.\n"
            + "\n".join(decision.unresolved_questions)
        )
    elif "why" in q or "selected" in q or "approach" in q:
        answer = decision.explanation
    else:
        answer = "Offline investigation supports questions about why, alternatives, evidence, conventions and assumptions. Use --model for a semantic follow-up with a configured provider."
    return Answer(
        answer=answer[:5000],
        evidence_ids=[e.id for e in decision.evidence],
        uncertainty="Offline response uses only the cached review. "
        + (
            "Original intent is not established."
            if decision.provenance != "recorded"
            else "Recorded rationale is an assistant statement, not verified correctness."
        ),
    )
