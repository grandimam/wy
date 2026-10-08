"""Repository-scoped discovery and identity-preserving history access."""

from __future__ import annotations

import json
from pathlib import Path

from wy.ingestion.claude import ClaudeCollector
from wy.ingestion.codex import CodexCollector
from wy.models import Review, Session
from wy.repository import repo_root
from wy.storage import Store

SOURCES = {"codex", "claude", "both", "none"}


def belongs(cwd: str | None, root: Path) -> bool:
    if not cwd or not Path(cwd).is_absolute():
        return False
    path = Path(cwd).resolve()
    if path == root:
        return True
    if not path.is_relative_to(root):
        return False
    try:
        return repo_root(path) == root
    except (ValueError, OSError):
        return False


def discover(root: Path, source: str = "both", codex_home: Path | None = None,
             claude_home: Path | None = None) -> list[dict]:
    if source not in SOURCES:
        raise ValueError("History source must be codex, claude, both or none")
    root = repo_root(root)
    entries = []
    if source in {"codex", "both"}:
        for home in dict.fromkeys([codex_home, root / ".codex"]):
            entries.extend({**item, "agent": "codex"} for item in CodexCollector().discover(home))
    if source in {"claude", "both"}:
        for home in dict.fromkeys([claude_home, root / ".claude"]):
            entries.extend(ClaudeCollector().discover(home))
    matched, checked = {}, {}
    for item in entries:
        cwd = item.get("cwd")
        if not isinstance(cwd, str):
            continue
        if cwd not in checked:
            checked[cwd] = belongs(cwd, root)
        if checked[cwd]:
            matched.setdefault((item["agent"], item["id"]), item)
    return sorted(matched.values(), key=lambda s: str(s.get("timestamp") or ""), reverse=True)


def resolve(root: Path, selectors: list[str], source: str = "both") -> list[Path]:
    entries = None
    paths = []
    for selector in selectors:
        path = Path(selector)
        if path.is_file():
            paths.append(path)
            continue
        if entries is None:
            entries = discover(root, source)
        matches = [s for s in entries if selector in {s["id"], f"{s['agent']}:{s['id']}"}]
        if len(matches) != 1:
            raise ValueError("Session not found in this repository or ambiguous; use agent:id or an explicit path")
        paths.append(Path(matches[0]["path"]))
    return paths


def collect(path: Path) -> Session:
    # Detect supported formats from bounded metadata, without guessing by folder name.
    with path.open(encoding="utf-8") as stream:
        prefix = stream.read(1_000_000)
    for line in prefix.splitlines()[:100]:
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if not isinstance(row, dict):
            continue
        if row.get("type") in {"session_meta", "thread.started", "response_item", "event_msg", "item.completed"}:
            return CodexCollector().collect(path)
        if row.get("sessionId") and row.get("type") in {"user", "assistant", "progress"}:
            return ClaudeCollector().collect(path)
    raise ValueError("Unrecognized session format; expected a Codex or Claude Code JSONL transcript")


def saved_sessions(review: Review) -> list[Session]:
    store = Store(Path(review.root))
    keys = [ref.storage_key for ref in review.sessions]
    if not keys and review.session_id:
        keys = [review.session_id]
    sessions = []
    for key in keys:
        try:
            data = store.get("session", key)
        except ValueError:
            continue
        session = Session.model_validate(data)
        if not belongs(session.cwd, Path(review.root)):
            raise ValueError("Cached session is not verifiably scoped to this repository; create a fresh review")
        sessions.append(session)
    return sessions


def session_for_evidence(review: Review, evidence) -> Session | None:
    return next((s for s in saved_sessions(review) if s.path == evidence.file
                 and (evidence.session_id is None or evidence.session_id == s.id)
                 and (evidence.agent is None or evidence.agent == s.agent)), None)
