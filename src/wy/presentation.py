"""Terminal views of cached evidence; no model calls or source excerpts by default."""

from __future__ import annotations

import shlex
from pathlib import Path

from rich import box
from rich.console import Console
from rich.panel import Panel
from rich.syntax import Syntax
from rich.table import Table
from rich.text import Text

from wy.models import Decision, Review, Session
from wy.storage import Store

STATUS = {"recorded": "green", "inferred": "yellow", "unexplained": "cyan"}


def source_scope(file: str) -> str:
    parts = Path(file).parts
    if any(p in {"examples", "benchmarks", "fixtures", "testdata"} for p in parts):
        return "Examples / fixtures"
    if any(p in {"test", "tests", "__tests__"} for p in parts) or Path(file).name.startswith("test_"):
        return "Tests"
    if "docs" in parts or Path(file).suffix.lower() in {".md", ".rst"}:
        return "Documentation"
    return "Implementation / configuration"


def console() -> Console:
    return Console(highlight=False, markup=False)


def saved_session(review: Review) -> Session | None:
    if not review.session_id:
        return None
    try:
        data = Store(Path(review.root)).get("session", review.session_id)
    except ValueError:
        return None
    return Session.model_validate(data)


def command(review: Review, action: str) -> str:
    return f"wy {action} --repo {shlex.quote(review.root)}"


def origin(out: Console, review: Review, session: Session | None):
    lines = [
        f"Review      {review.id}",
        f"Created     {review.created_at}",
        f"Repository  {review.root}",
        f"Saved in    {Path(review.root) / '.wy' / 'wy.sqlite3'}",
        f"Analysis    {review.provider} · tokens {review.input_tokens} in / {review.output_tokens} out",
        f"Codex       {review.session_id or 'None supplied — repository evidence only'}",
    ]
    if session:
        lines.append(f"Transcript  {session.path}")
    elif review.session_id:
        lines.append("Transcript  Saved session details unavailable")
    lines.append(f"Baseline    {review.baseline_id or 'None'}")
    lines.append("Authorship  Not proven; the linked session is context, not an author identity.")
    out.print(Panel(Text("\n".join(lines)), title="Review origin", border_style="blue"))


def review_view(review: Review, *, gaps_only: bool = False):
    out = console()
    origin(out, review, saved_session(review))
    table = Table(box=box.SIMPLE, expand=True, title="Open gaps" if gaps_only else "Decision index")
    table.add_column("#", style="bold", width=2)
    table.add_column("Decision / location", ratio=3, overflow="fold")
    table.add_column("Rationale", min_width=11)
    table.add_column("Evidence", min_width=9)
    table.add_column("Assessment", min_width=10, overflow="fold")
    shown = 0
    for number, decision in enumerate(review.decisions, 1):
        if gaps_only and not (
            decision.provenance == "unexplained" or decision.unresolved_questions or decision.stale
        ):
            continue
        shown += 1
        loc = decision.location
        label = Text(f"{decision.question}\n", overflow="fold")
        label.append(f"{loc.file}:{loc.start_line}", style="dim")
        if source_scope(loc.file) != "Implementation / configuration":
            label.append("\n" + source_scope(loc.file), style="yellow")
        state = Text(decision.provenance.title(), style=STATUS[decision.provenance])
        if decision.stale:
            state.append("\nSTALE", style="bold red")
        counts = []
        for kind, name in (("session", "session"), ("code", "code"), ("diff", "diff")):
            count = sum(e.kind == kind for e in decision.evidence)
            if count:
                counts.append(f"{count} {name}")
        later = decision.reflections[-1].assessment.replace("_", " ") if decision.reflections else "None"
        table.add_row(str(number), label, state, Text("\n".join(counts) or "None"), Text(later))
    out.print(table)
    out.print(f"{shown} of {len(review.decisions)} decisions · numbers refer to this review only")
    out.print("Recorded = stated reason; Inferred = hypothesis; Unexplained = reason not established.")
    out.print("Later assessments are separate opinions, not original intent or verified identity.")
    for warning in review.warnings:
        out.print(Text("Note: " + warning, style="yellow"))
    out.print("\nInspect a number: " + command(review, "explain <number>"))
    out.print("Trace the session: " + command(review, "session"))


def items(out: Console, title: str, values: list[str]):
    if values:
        out.print(Text("\n" + title, style="bold"))
        for value in values:
            out.print(Text("• " + value))


def decision_view(decision: Decision, review: Review | None = None, *, show_code: bool = False):
    out = console()
    session = saved_session(review) if review else None
    if review:
        origin(out, review, session)
    loc = decision.location
    out.print(Panel(Text(f"{decision.question}\n{loc.file}:{loc.start_line} · {loc.symbol}\n{decision.id}"),
                    title="Decision", border_style="blue"))
    if decision.stale:
        out.print(Text("STALE — source or cited evidence changed. Re-review before relying on this.", style="bold red"))
    out.print(Text("Original rationale · " + decision.provenance.title(), style=STATUS[decision.provenance]))
    out.print(Text(decision.explanation))
    out.print("Authorship: not proven" + ("; changes isolated since baseline." if decision.attribution == "since-baseline" else "."))
    events = {e.id: e for e in session.events} if session else {}
    out.print(Text("\nEvidence trail", style="bold"))
    for number, evidence in enumerate(decision.evidence, 1):
        out.print(Text(f"e {number} · [{evidence.id}] {evidence.kind} · {evidence.file}:{evidence.start_line}"))
        if evidence.kind == "session":
            event = events.get(evidence.event_id)
            out.print(Text(f"  Event: {evidence.event_id or 'unknown'} · kind: {event.kind if event else 'unknown'}"))
            out.print(Text(evidence.excerpt))
            if review and evidence.event_id:
                out.print("  Inspect stored event: " + command(review, f"session --event {shlex.quote(evidence.event_id)}"))
        elif show_code:
            out.print(Text(evidence.excerpt))
    if not any(e.kind == "session" for e in decision.evidence):
        out.print("No conversation event is cited for this decision, even if the review has a linked session.")
    if not show_code and any(e.kind != "session" for e in decision.evidence):
        out.print("Source excerpts hidden; add --show-code to inspect them.")
    items(out, "Alternatives to investigate (not proof they were considered)", decision.alternatives)
    items(out, "Assumptions to check", decision.assumptions)
    items(out, "Unanswered questions", decision.unresolved_questions)
    for item in decision.reflections:
        details = [
            f"Retrospective assessment: {item.assessment}",
            f"Recorded at: {item.created_at}",
            f"Agent: {item.agent} · model: {item.model or 'unknown'}",
            f"Context: {item.context} · identity self-reported",
            "Author session: not recorded for this assessment",
            f"Request: {item.request_id}",
            f"Rationale: {item.rationale}",
            "Citations: " + (", ".join(item.evidence_ids) or "None"),
            f"Uncertainty: {item.uncertainty}",
        ]
        details += ["Alternative: " + value for value in item.alternatives]
        details += ["Assumption: " + value for value in item.assumptions]
        if item.suggested_change:
            details.append("Suggested change: " + item.suggested_change)
        out.print(Panel(Text("\n".join(details)), title="Later assessment · not original intent", border_style="yellow"))
    if review:
        out.print("\nInvestigate: " + command(review, f"ask {decision.id} 'What assumptions should I verify?'"))


def session_view(review: Review, session: Session, event_id: str | None = None):
    out = console()
    origin(out, review, session)
    out.print("Stored conversation snapshot; live transcript changes are not loaded.")
    out.print(f"Workspace: {session.cwd or 'unknown'} · format: {session.format} · {len(session.events)} stored events")
    for warning in session.warnings:
        out.print(Text("Note: " + warning, style="yellow"))
    if event_id:
        event = next((e for e in session.events if e.id == event_id), None)
        if event is None:
            raise ValueError("Event not found in the saved session; run wy session for cited event IDs")
        out.print(Panel(Text(event.text), title=f"{event.id} · {event.kind}"))
        out.print(Text(f"Source: {session.path}:{event.source_line}"))
        return
    first_request = next((e for e in session.events if e.kind == "user"), None)
    if first_request:
        preview = first_request.text[:600]
        if len(first_request.text) > 600:
            preview += "\n[preview truncated]"
        out.print(Panel(Text(preview), title=f"First stored user message · {first_request.id}"))
        out.print("Read full message: " + command(review, f"session --event {first_request.id}"))
    else:
        out.print("No user request captured in this session snapshot.")
    table = Table(box=box.SIMPLE, expand=True, title="Conversation events cited by decisions")
    table.add_column("Event / line")
    table.add_column("Kind")
    table.add_column("Decisions", ratio=2)
    table.add_column("Preview", ratio=3)
    references: dict[str, list[str]] = {}
    for number, decision in enumerate(review.decisions, 1):
        for evidence in decision.evidence:
            if evidence.kind == "session" and evidence.event_id:
                label = f"#{number} {decision.question}"
                values = references.setdefault(evidence.event_id, [])
                if label not in values:
                    values.append(label)
    events = {e.id: e for e in session.events}
    for key, labels in references.items():
        event = events.get(key)
        preview = " ".join(event.text.split()) if event else "Event unavailable in saved snapshot"
        if len(preview) > 180:
            preview = preview[:180] + "…"
        table.add_row(Text(f"{key}\nline {event.source_line if event else '?'}"),
                      Text(event.kind if event else "unknown"), Text("\n".join(labels)), Text(preview))
    if references:
        out.print(table)
        out.print("Inspect a stored event: " + command(review, "session --event <event-id>"))
    else:
        out.print("No conversation events are cited by this review's decisions.")
    out.print("Cited events provide context; they do not prove who authored a change.")


def evidence_view(data: dict):
    out = console()
    evidence = data["evidence"]
    out.print(Panel(Text(f"{evidence['id']} · {evidence['kind']}\n{evidence['file']}:{evidence['start_line']}\n"
                         f"Decision: {data['decision_id']}\nReview: {data['review_id']}\n"
                         f"Codex context: {data['session_id'] or 'None'}"), title="Evidence trace", border_style="blue"))
    if evidence["kind"] == "session":
        out.print(Panel(Text(evidence["excerpt"]), title="Cited conversation excerpt"))
        for title, key in (("Nearby stored events", "context"), ("Linked tool call / result", "linked_tool_events")):
            events = data.get(key, [])
            if events:
                table = Table(box=box.SIMPLE, title=title, expand=True)
                table.add_column("Event")
                table.add_column("Kind")
                table.add_column("Preview", ratio=1)
                for event in events:
                    preview = " ".join(event["text"].split())
                    if len(preview) > 200:
                        preview = preview[:200] + "…"
                    table.add_row(Text(event["id"]), Text(event["kind"]), Text(preview))
                out.print(table)
        out.print("Inspect a full event with: wy session --event <event-id>")
    else:
        lexer = Path(evidence["file"]).suffix.lstrip(".") or "text"
        out.print(Panel(Syntax(evidence["excerpt"], lexer, line_numbers=True,
                              start_line=evidence["start_line"], word_wrap=True),
                        title="Saved excerpt · review time"))
        current = data.get("current")
        if current:
            out.print(Text("Current anchor: " + current["status"].replace("_", " "),
                           style="green" if current["file_unchanged"] else "yellow"))
            if current["status"] == "ambiguous":
                out.print("Matching lines: " + ", ".join(map(str, current["matching_lines"])))
            out.print(Panel(Syntax(current["excerpt"], lexer, line_numbers=True,
                                  start_line=current["start_line"], word_wrap=True),
                            title="Current file · surrounding context"))
    out.print(Text(data["note"], style="yellow"))
    items(out, "Other decisions sharing this citation", [
        f"#{d['number']} {d['question']}" for d in data["related_decisions"]
    ])
