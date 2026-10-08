"""A read-only terminal workspace for navigating decisions and their evidence."""

from __future__ import annotations

import shlex
import sqlite3
import subprocess
from pathlib import Path

from rich.text import Text
from textual import events, on, work
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.screen import ModalScreen
from textual.widgets import (
    Button,
    Footer,
    Header,
    Input,
    Label,
    OptionList,
    Static,
    TabbedContent,
    TabPane,
    TextArea,
    Tree,
)
from textual.widgets.option_list import Option, OptionDoesNotExist

from wy import presentation, service, trace
from wy.models import Review
from wy.repository import repo_root
from wy.security import read_source, redact

Route = tuple[str, str | None, str | None]

COMMANDS = {
    "/help": "Show commands and navigation keys",
    "/decisions": "Focus the decision tree",
    "/decision <number>": "Open a decision",
    "/evidence <number>": "Follow a citation on the selected decision",
    "/session": "Browse the saved Codex conversation",
    "/event <event-id>": "Open a stored conversation event",
    "/code": "Show the code panes",
    "/line <number>": "Jump to a line in the current file",
    "/find <words>": "Filter decisions by question or file",
    "/ask <question>": "Investigate the selected decision using cached evidence",
    "/origin": "Show review and session provenance",
    "/back": "Go back",
    "/forward": "Go forward",
    "/review [session-id-or-path]": "Create a fresh offline review; optionally include a Codex session",
    "/quit": "Close the workspace",
}


class MessageScreen(ModalScreen):
    CSS = """
    MessageScreen { align: center middle; background: #000000 60%; }
    #message-box { width: 85%; height: 85%; border: round #69c9c1; background: #152337; padding: 1 2; }
    #message-scroll { height: 1fr; }
    #message-copy { height: auto; }
    #close-message { dock: bottom; margin-top: 1; }
    """
    BINDINGS = [("escape", "dismiss", "Close")]

    def __init__(self, text: str):
        super().__init__()
        self.text = text

    def compose(self):
        with Vertical(id="message-box"):
            with VerticalScroll(id="message-scroll"):
                yield Static(Text(self.text), id="message-copy")
            yield Button("Close · Esc", id="close-message")

    @on(Button.Pressed, "#close-message")
    def close_message(self):
        self.dismiss()


class Explorer(App):
    TITLE = "wy · decision workspace"
    CSS = """
    Screen { background: #0d1420; }
    Header { background: #152337; }
    #origin-strip { height: 2; padding: 0 1; background: #152337; color: #afc5de; }
    #workspace { height: 1fr; }
    #sidebar { width: 33%; min-width: 25; max-width: 52; border-right: solid #2d425a; }
    #filter { margin: 1 1 0 1; }
    #decision-tree { height: 1fr; background: #0d1420; padding: 0 1; }
    #sidebar-note { height: auto; padding: 1; color: #8ea5bf; }
    #main { width: 1fr; }
    #navigation { height: 3; padding: 0 1; }
    #navigation Button { min-width: 9; margin-right: 1; }
    #breadcrumb { width: 1fr; padding: 1 0 0 1; color: #99d9d4; }
    TabbedContent { height: 1fr; }
    TabPane { padding: 1; }
    #decision-scroll { height: 1fr; }
    #decision-copy, #origin-copy { height: auto; padding: 0 1 1 1; }
    .section-label { height: auto; color: #99d9d4; padding: 1 1 0 1; text-style: bold; }
    #citations { height: auto; max-height: 16; min-height: 4; margin: 0 1; }
    #code-status { height: auto; max-height: 6; color: #e9bf78; padding: 0 1 1 1; }
    #code-tools { height: 3; }
    #line-input { width: 16; }
    #source-search { width: 1fr; }
    #find-next { min-width: 10; }
    #code-panes { height: 1fr; }
    .source-pane { width: 1fr; }
    .source-title { height: 2; padding: 0 1; color: #99d9d4; }
    TextArea { height: 1fr; border: solid #2d425a; }
    #related { height: auto; max-height: 6; min-height: 2; }
    #conversation-note { height: auto; max-height: 4; color: #e9bf78; }
    #conversation-panes { height: 1fr; }
    #event-sidebar { width: 36%; min-width: 20; }
    #events { height: 1fr; }
    #event-main { width: 1fr; }
    #event-label { height: auto; max-height: 4; padding: 0 1; color: #99d9d4; }
    #event-buttons { height: 3; }
    #event-buttons Button { min-width: 8; margin-right: 1; }
    #event-text { height: 1fr; }
    #status { height: 1; padding: 0 1; background: #152337; color: #afc5de; }
    #command { margin: 0 1; height: 3; }
    #command-hints { height: auto; max-height: 7; display: none; margin: 0 1; background: #152337; }
    """
    BINDINGS = [
        Binding("ctrl+q", "quit", "Quit", priority=True),
        Binding("alt+left", "back", "Back", priority=True),
        Binding("alt+right", "forward", "Forward", priority=True),
        Binding("ctrl+k", "search_decisions", "Find decision", priority=True),
        Binding("ctrl+g", "goto_line", "Go to line", priority=True),
        Binding("ctrl+f", "find_source", "Find in code", priority=True),
        Binding("escape", "sidebar", "Decisions", priority=True),
        Binding("ctrl+j", "command", "Commands", priority=True),
    ]

    def __init__(self, review: Review):
        super().__init__()
        self.review = review
        self.session = presentation.saved_session(review)
        self.routes: list[Route] = []
        self.route_index = -1
        self.selected_decision: str | None = None
        self.selected_event: str | None = None
        self.current_file: str | None = None
        self.paired_event: str | None = None
        self.decision_nodes = {}
        self.route_nodes = {}
        self.reviewing = False

    def compose(self) -> ComposeResult:
        yield Header()
        yield Static(id="origin-strip", markup=False)
        with Horizontal(id="workspace"):
            with Vertical(id="sidebar"):
                yield Input(placeholder="Find decision or file…  Ctrl+K", id="filter")
                yield Tree("Decisions", id="decision-tree")
                yield Static("Enter / click to open · expand to follow citations\nR = recorded  I = inferred  U = unexplained\n! = stale evidence", id="sidebar-note", markup=False)
            with Vertical(id="main"):
                with Horizontal(id="navigation"):
                    yield Button("← Back", id="back")
                    yield Button("Forward →", id="forward")
                    yield Static("Select a decision", id="breadcrumb", markup=False)
                with TabbedContent(id="tabs"):
                    with TabPane("Decision", id="decision-tab"):
                        with VerticalScroll(id="decision-scroll"):
                            yield Static("No decisions in this review.", id="decision-copy", markup=False)
                            yield Label("FOLLOW THE EVIDENCE  ·  select a citation", classes="section-label")
                            yield OptionList(id="citations", markup=False)
                    with TabPane("Code", id="code-tab"):
                        yield Static("Select a code citation to inspect its saved and current source.", id="code-status", markup=False)
                        with Horizontal(id="code-tools"):
                            yield Input(placeholder="Go to line", id="line-input", type="integer")
                            yield Input(placeholder="Find in current file…", id="source-search")
                            yield Button("Find next", id="find-next")
                        with Horizontal(id="code-panes"):
                            with Vertical(classes="source-pane"):
                                yield Label("SAVED EXCERPT · review time", classes="source-title")
                                yield TextArea(read_only=True, show_line_numbers=True, id="saved-code")
                            with Vertical(classes="source-pane"):
                                yield Label("CURRENT FILE · read-only", classes="source-title")
                                yield TextArea(read_only=True, show_line_numbers=True, id="current-code")
                        yield Label("RELATED DECISIONS · sharing this citation", classes="section-label")
                        yield OptionList(id="related", markup=False)
                    with TabPane("Conversation", id="conversation-tab"):
                        yield Static("Stored Codex events. Proximity is context, not proof of cause or authorship.", id="conversation-note", markup=False)
                        with Horizontal(id="conversation-panes"):
                            with Vertical(id="event-sidebar"):
                                yield Input(placeholder="Search stored events…", id="event-filter")
                                yield OptionList(id="events", markup=False)
                            with Vertical(id="event-main"):
                                yield Static("Select an event", id="event-label", markup=False)
                                with Horizontal(id="event-buttons"):
                                    yield Button("← Prev", id="previous-event")
                                    yield Button("Next →", id="next-event")
                                    yield Button("Call / result", id="paired-event", disabled=True)
                                yield TextArea(read_only=True, id="event-text")
                    with TabPane("Origin", id="origin-tab"):
                        with VerticalScroll():
                            yield Static(id="origin-copy", markup=False)
        yield Static("Read-only · Tab changes pane · arrows navigate · Enter opens", id="status", markup=False)
        yield OptionList(id="command-hints", markup=False)
        yield Input(placeholder="/help · /decisions · /session · /find …    Ctrl+J to focus", id="command")
        yield Footer()

    def on_mount(self):
        self.theme = "textual-dark"
        self.query_one("#origin-strip", Static).update(
            f"{self.review.id} · {self.review.created_at} · {self.review.provider}\n"
            f"Codex context: {self.review.session_id or 'none supplied'} · authorship not proven"
        )
        self.show_origin()
        self.fill_tree()
        self.fill_events()
        if self.review.decisions:
            first = next((d for d in self.review.decisions
                          if presentation.source_scope(d.location.file) == "Implementation / configuration"),
                         self.review.decisions[0])
            self.navigate(("decision", first.id, None))
        else:
            self.query_one("#decision-copy", Static).update(
                "Welcome to wy\n\nType /help in the command bar to see available actions.\n\n"
                "No decisions are available yet. Use /review to analyze local changes, or "
                "/review <session-id-or-path> to include a Codex conversation.\n\n"
                "Review origin and limitations are available in the Origin tab."
            )
        self.query_one("#decision-tree", Tree).focus()
        self.update_navigation()

    def show_origin(self):
        r = self.review
        text = Text("REVIEW ORIGIN\n\n", style="bold")
        text.append(f"Review: {r.id}\nCreated: {r.created_at}\nRepository: {r.root}\n"
                    f"Storage: {Path(r.root) / '.wy' / 'wy.sqlite3'}\n"
                    f"Analysis: {r.provider} · {r.input_tokens} input / {r.output_tokens} output tokens\n"
                    f"Codex context: {r.session_id or 'None supplied'}\n"
                    f"Transcript: {self.session.path if self.session else 'Unavailable'}\n"
                    f"Baseline: {r.baseline_id or 'None'}\n\n", style="")
        text.append("What this establishes\n", style="bold cyan")
        text.append("The selected session supplied contextual evidence. It does not prove who authored the changes. "
                    "Recorded means a reason was stated, not that it was correct. Inferred means a hypothesis. "
                    "Unexplained means the available evidence does not establish the reason.\n\n", style="")
        text.append("Later assessments\n", style="bold cyan")
        text.append("Assessments are retrospective and identities are self-reported. Their authoring session "
                    "is not recorded by the current import format.\n\n", style="")
        text.append("Stored conversation\n", style="bold cyan")
        text.append("This workspace reads the saved session snapshot, not the live transcript. "
                    "It traces observable statements and evidence, not private model reasoning.\n\n", style="")
        text.append("\n".join(r.warnings), style="yellow")
        self.query_one("#origin-copy", Static).update(text)

    def fill_tree(self, query: str = ""):
        tree = self.query_one("#decision-tree", Tree)
        tree.clear()
        tree.show_root = False
        tree.root.expand()
        self.decision_nodes = {}
        self.route_nodes = {}
        groups = {}
        for scope in ("Implementation / configuration", "Examples / fixtures", "Tests", "Documentation"):
            if any(presentation.source_scope(d.location.file) == scope
                   and query.casefold() in f"{d.question} {d.explanation} {d.location.file}".casefold()
                   for d in self.review.decisions):
                groups[scope] = tree.root.add(Text(scope, style="bold cyan"), expand=True)
        for number, d in enumerate(self.review.decisions, 1):
            if query.casefold() not in f"{d.question} {d.explanation} {d.location.file}".casefold():
                continue
            label = Text(f"{number:02}  {d.provenance[0].upper()}{' !' if d.stale else ''}  ")
            label.append(d.question)
            node = groups[presentation.source_scope(d.location.file)].add(label, data=("decision", d.id, None))
            self.decision_nodes[d.id] = node
            self.route_nodes[node.data] = node
            for e in d.evidence:
                label = f"{'chat' if e.kind == 'session' else 'code'} · "
                label += e.event_id or f"{e.file}:{e.start_line}"
                child = node.add_leaf(Text(label), data=("evidence", d.id, e.id))
                self.route_nodes[child.data] = child
            for i, item in enumerate(d.reflections):
                child = node.add_leaf(Text(f"assessment · {item.assessment} · {item.agent}"), data=("assessment", d.id, str(i)))
                self.route_nodes[child.data] = child
        self.query_one("#status", Static).update(f"{len(self.decision_nodes)} decisions shown · Enter opens · Right expands citations")

    @on(Input.Changed, "#filter")
    def filter_decisions(self, event: Input.Changed):
        self.fill_tree(event.value)

    @on(Tree.NodeSelected, "#decision-tree")
    def choose_tree_node(self, event: Tree.NodeSelected):
        if event.node.data:
            self.navigate(event.node.data)

    def fill_events(self, query: str = ""):
        options = self.query_one("#events", OptionList)
        options.clear_options()
        if not self.session:
            self.query_one("#conversation-note", Static).update("No saved Codex session is available for this review.")
            return
        options.add_options([
            Option(Text(f"{e.id} · {e.kind}\n{' '.join(e.text.split())[:110]}"), id=e.id)
            for e in self.session.events
            if query.casefold() in f"{e.id} {e.kind} {e.text}".casefold()
        ])

    @on(Input.Changed, "#event-filter")
    def filter_events(self, event: Input.Changed):
        self.fill_events(event.value)

    @on(OptionList.OptionSelected, "#events")
    def choose_event(self, event: OptionList.OptionSelected):
        self.navigate(("event", self.selected_decision, event.option.id))

    @on(OptionList.OptionSelected, "#citations")
    def choose_citation(self, event: OptionList.OptionSelected):
        self.navigate(("evidence", self.selected_decision, event.option.id))

    @on(OptionList.OptionSelected, "#related")
    def choose_related(self, event: OptionList.OptionSelected):
        self.navigate(("decision", event.option.id, None))

    def navigate(self, route: Route, *, remember: bool = True):
        if remember and (self.route_index < 0 or self.routes[self.route_index] != route):
            self.routes = self.routes[:self.route_index + 1]
            self.routes.append(route)
            self.route_index += 1
        kind, decision_id, detail = route
        if decision_id:
            self.selected_decision = decision_id
            self.show_decision(decision_id, assessment=int(detail) if kind == "assessment" else None)
        if kind == "evidence":
            decision = service.select(self.review, decision_id)
            evidence = trace.select_evidence(decision, detail)
            if evidence.kind == "session":
                self.show_event(evidence.event_id, cited=evidence.excerpt)
            else:
                self.show_source(trace.inspect(self.review, decision, evidence))
        elif kind == "event":
            self.show_event(detail)
        else:
            self.query_one("#tabs", TabbedContent).active = "decision-tab"
        self.query_one("#breadcrumb", Static).update(
            f"{decision_id or 'Review'} → {detail or kind}"
        )
        node = self.route_nodes.get(route)
        if node:
            node.parent.expand()
            self.query_one("#decision-tree", Tree).move_cursor(node)
        self.update_navigation()

    def update_navigation(self):
        self.query_one("#back", Button).disabled = self.route_index <= 0
        self.query_one("#forward", Button).disabled = self.route_index >= len(self.routes) - 1

    def show_decision(self, decision_id: str, assessment: int | None = None):
        d = service.select(self.review, decision_id)
        text = Text(d.question + "\n", style="bold")
        text.append(f"{d.location.file}:{d.location.start_line} · {d.location.symbol}\n{d.id}\n\n", style="dim")
        scope = presentation.source_scope(d.location.file)
        text.append(f"SCOPE · {scope}\n", style="cyan")
        if scope != "Implementation / configuration":
            text.append("This finding is in supporting material; it does not establish an application design choice.\n", style="yellow")
        if d.stale:
            text.append("STALE · source or cited evidence changed since review\n\n", style="bold yellow")
        text.append(f"ORIGINAL RATIONALE · {d.provenance.upper()}\n", style="cyan")
        text.append(d.explanation + "\n\n", style="")
        session_count = sum(e.kind == "session" for e in d.evidence)
        text.append(f"{session_count} cited conversation events · authorship not proven\n", style="dim")
        if not session_count:
            text.append("The linked review session is not cited as evidence for this decision.\n", style="yellow")
        for label, values in (("ALTERNATIVES TO INVESTIGATE", d.alternatives),
                              ("ASSUMPTIONS TO CHECK", d.assumptions),
                              ("UNANSWERED QUESTIONS", d.unresolved_questions)):
            if values:
                text.append("\n" + label + "\n", style="bold cyan")
                for value in values:
                    text.append("• " + value + "\n", style="")
        for i, item in enumerate(d.reflections):
            text.append(f"\n{'▶ ' if assessment == i else ''}LATER ASSESSMENT · {item.assessment.upper()}\n", style="bold yellow")
            text.append(f"{item.created_at}\nAgent: {item.agent} · model: {item.model or 'unknown'}\n"
                        f"Context: {item.context} · identity self-reported\nAuthor session: not recorded\n"
                        f"{item.rationale}\nUncertainty: {item.uncertainty}\n", style="")
            for value in item.alternatives:
                text.append(f"Alternative: {value}\n", style="")
            for value in item.assumptions:
                text.append(f"Assumption: {value}\n", style="")
            if item.suggested_change:
                text.append(f"Suggested change: {item.suggested_change}\n", style="")
            text.append("Citations: " + ", ".join(item.evidence_ids) + "\n", style="dim")
        self.query_one("#decision-copy", Static).update(text)
        options = self.query_one("#citations", OptionList)
        options.clear_options()
        citations = d.evidence
        if assessment is not None:
            ids = set(d.reflections[assessment].evidence_ids)
            citations = [e for e in citations if e.id in ids]
        options.add_options([
            Option(Text(f"{e.kind.upper()} · {e.event_id or f'{e.file}:{e.start_line}'}\n"
                        f"{'Conversation context' if e.kind == 'session' else 'Target file' if e.file == d.location.file else 'Related source · relevance requires checking'} · {e.id}"), id=e.id)
            for e in citations
        ])
        self.query_one("#decision-scroll", VerticalScroll).scroll_home(animate=False)

    def show_source(self, data: dict):
        e = data["evidence"]
        self.current_file = e["file"]
        current = data.get("current")
        self.query_one("#code-status", Static).update(
            f"{e['file']} · saved lines {e['start_line']}–{e['end_line']}\n"
            f"{data['note']}\nCurrent anchor: {current['status'].replace('_', ' ') if current else 'unavailable'}"
        )
        saved = self.query_one("#saved-code", TextArea)
        live = self.query_one("#current-code", TextArea)
        language = {"py": "python", "js": "javascript", "rs": "rust", "md": "markdown", "yml": "yaml"}.get(
            Path(e["file"]).suffix.lstrip("."), Path(e["file"]).suffix.lstrip(".")
        )
        for area in (saved, live):
            area.language = language if language in area.available_languages else None
        saved.line_number_start = e["start_line"]
        saved.load_text(e["excerpt"])
        raw = read_source(Path(self.review.root), e["file"])
        live.load_text(redact(raw) if raw is not None else "Current source unavailable.")
        self.query_one("#tabs", TabbedContent).active = "code-tab"
        if current:
            self.call_after_refresh(live.move_cursor, (current["anchor_line"] - 1, 0), center=True)
        related = self.query_one("#related", OptionList)
        related.clear_options()
        related.add_options([Option(Text(f"#{d['number']} {d['question']}"), id=d["id"]) for d in data["related_decisions"]])

    def show_event(self, event_id: str | None, cited: str | None = None):
        self.query_one("#tabs", TabbedContent).active = "conversation-tab"
        event = next((e for e in self.session.events if e.id == event_id), None) if self.session else None
        self.selected_event = event_id if event else None
        self.paired_event = None
        if event:
            self.query_one("#event-label", Static).update(
                f"{event.id} · {event.kind} · transcript line {event.source_line}\n"
                f"Codex context: {self.session.id}"
            )
            self.query_one("#event-text", TextArea).load_text(event.text)
            if event.call_id:
                pair = next((e for e in self.session.events if e.id != event.id and e.call_id == event.call_id), None)
                self.paired_event = pair.id if pair else None
            options = self.query_one("#events", OptionList)
            try:
                options.highlighted = options.get_option_index(event.id)
            except OptionDoesNotExist:
                # A search may intentionally exclude the opened event.
                pass
        else:
            self.query_one("#event-label", Static).update("Event unavailable in the saved session")
            self.query_one("#event-text", TextArea).load_text(cited or "No stored event selected.")
        self.query_one("#paired-event", Button).disabled = self.paired_event is None
        self.query_one("#conversation-note", Static).update(
            "Cited event → stored conversation. Nearby events are context, not proof of cause."
            if cited else "Stored conversation snapshot · nearby events are context, not proof of cause."
        )

    @on(Button.Pressed)
    def button_pressed(self, event: Button.Pressed):
        actions = {"back": self.action_back, "forward": self.action_forward, "find-next": self.find_next}
        if event.button.id in actions:
            actions[event.button.id]()
        elif event.button.id in {"previous-event", "next-event"}:
            self.step_event(-1 if event.button.id == "previous-event" else 1)
        elif event.button.id == "paired-event" and self.paired_event:
            self.navigate(("event", self.selected_decision, self.paired_event))

    def step_event(self, offset: int):
        if not self.session or not self.session.events:
            self.notify("No saved session events")
            return
        index = next((i for i, e in enumerate(self.session.events) if e.id == self.selected_event), -1)
        index = max(0, min(len(self.session.events) - 1, index + offset))
        self.navigate(("event", self.selected_decision, self.session.events[index].id))

    @on(Input.Submitted, "#line-input")
    def goto_line(self, event: Input.Submitted):
        area = self.query_one("#current-code", TextArea)
        try:
            line = int(event.value)
        except ValueError:
            self.notify("Enter a line number", severity="warning")
            return
        if not 1 <= line <= area.document.line_count:
            self.notify("Line is outside the current file", severity="warning")
            return
        area.move_cursor((line - 1, 0), center=True)
        area.focus()

    @on(Input.Submitted, "#source-search")
    def search_source(self):
        self.find_next()

    def find_next(self):
        query = self.query_one("#source-search", Input).value
        if not query:
            return
        area = self.query_one("#current-code", TextArea)
        row, col = area.cursor_location
        lines = area.text.splitlines(keepends=True)
        start = sum(len(line) for line in lines[:row]) + col + 1
        position = area.text.find(query, start)
        if position < 0:
            position = area.text.find(query)
        if position < 0:
            self.notify("No match in the current file", severity="warning")
            return
        row = area.text.count("\n", 0, position)
        col = position - (area.text.rfind("\n", 0, position) + 1)
        area.move_cursor((row, col), center=True)
        area.focus()

    def action_back(self):
        if self.route_index > 0:
            self.route_index -= 1
            self.navigate(self.routes[self.route_index], remember=False)

    def action_forward(self):
        if self.route_index + 1 < len(self.routes):
            self.route_index += 1
            self.navigate(self.routes[self.route_index], remember=False)

    def action_search_decisions(self):
        self.query_one("#filter", Input).focus()

    def action_sidebar(self):
        self.query_one("#decision-tree", Tree).focus()

    def action_goto_line(self):
        self.query_one("#tabs", TabbedContent).active = "code-tab"
        self.query_one("#line-input", Input).focus()

    def action_find_source(self):
        self.query_one("#tabs", TabbedContent).active = "code-tab"
        self.query_one("#source-search", Input).focus()

    def action_command(self):
        self.query_one("#command", Input).focus()

    def on_key(self, event: events.Key):
        if (event.key == "down" and self.focused is self.query_one("#command", Input)
                and self.query_one("#command-hints", OptionList).display):
            event.prevent_default()
            event.stop()
            hints = self.query_one("#command-hints", OptionList)
            hints.highlighted = 0
            hints.focus()
            return
        if event.key == "slash" and not isinstance(self.focused, (Input, TextArea)):
            event.prevent_default()
            event.stop()
            field = self.query_one("#command", Input)
            field.value = "/"
            field.focus()
            field.cursor_position = 1

    def on_resize(self, event: events.Resize):
        if self.is_mounted:
            panes = self.query_one("#code-panes", Horizontal)
            panes.styles.layout = "vertical" if event.size.width < 110 else "horizontal"
            for pane in self.query(".source-pane"):
                pane.styles.width = "1fr" if event.size.width >= 110 else "100%"
                pane.styles.height = "1fr"

    @on(Input.Changed, "#command")
    def command_changed(self, event: Input.Changed):
        hints = self.query_one("#command-hints", OptionList)
        hints.clear_options()
        query = event.value.strip()
        matches = [(name, description) for name, description in COMMANDS.items()
                   if query and " " not in query and name.startswith(query)]
        hints.add_options([Option(Text(f"{name}  —  {description}"), id=name) for name, description in matches])
        hints.display = bool(matches)

    @on(OptionList.OptionSelected, "#command-hints")
    def choose_command_hint(self, event: OptionList.OptionSelected):
        field = self.query_one("#command", Input)
        name = event.option.id.split()[0]
        field.value = name + " "
        field.cursor_position = len(field.value)
        field.focus()

    @on(Input.Submitted, "#command")
    def submit_command(self, event: Input.Submitted):
        raw = event.value.strip()
        event.input.value = ""
        if not raw:
            return
        name, _, argument = raw.partition(" ")
        argument = argument.strip()
        try:
            if name in {"/help", "help", "?"}:
                self.push_screen(MessageScreen(
                    "WY WORKSPACE\n\n" + "\n".join(f"{key}\n  {value}\n" for key, value in COMMANDS.items())
                    + "\nClick or press Enter on a decision. Expand it to follow citations.\n"
                    "Tab switches focus. Alt+Left / Alt+Right move through your trail.\n"
                    "Ctrl+K finds a decision. Ctrl+F searches code. Ctrl+G goes to a line.\n"
                    "Ctrl+J focuses this command bar. Ctrl+Q exits.\n\n"
                    "Browsing is read-only. /review explicitly creates a fresh saved review."
                ))
            elif name == "/quit":
                self.exit()
            elif name == "/decisions":
                self.action_sidebar()
            elif name == "/decision":
                d = service.select(self.review, argument)
                self.navigate(("decision", d.id, None))
            elif name == "/evidence":
                if not self.selected_decision:
                    raise ValueError("Select a decision first")
                d = service.select(self.review, self.selected_decision)
                e = trace.select_evidence(d, argument)
                self.navigate(("evidence", d.id, e.id))
            elif name == "/session":
                self.query_one("#tabs", TabbedContent).active = "conversation-tab"
                self.query_one("#events", OptionList).focus()
            elif name == "/event":
                if not self.session or not any(e.id == argument for e in self.session.events):
                    raise ValueError("No such event in the saved session")
                self.navigate(("event", self.selected_decision, argument))
            elif name == "/code":
                self.query_one("#tabs", TabbedContent).active = "code-tab"
            elif name == "/line":
                if not self.current_file:
                    raise ValueError("Open a code citation first")
                field = self.query_one("#line-input", Input)
                field.value = argument
                self.query_one("#tabs", TabbedContent).active = "code-tab"
                self.goto_line(Input.Submitted(field, argument))
            elif name == "/find":
                self.query_one("#filter", Input).value = argument
                self.action_sidebar()
            elif name == "/ask":
                if not self.selected_decision or not argument:
                    raise ValueError("Select a decision and supply a question")
                answer = service.ask(service.select(service.load_review(Path(self.review.root)), self.selected_decision), argument)
                self.push_screen(MessageScreen(answer.answer + "\n\n" + answer.uncertainty))
            elif name == "/origin":
                self.query_one("#tabs", TabbedContent).active = "origin-tab"
            elif name == "/back":
                self.action_back()
            elif name == "/forward":
                self.action_forward()
            elif name == "/review":
                self.run_review(argument)
            else:
                raise ValueError("Unknown command. Type /help to see available actions.")
        except (ValueError, OSError) as exc:
            self.notify(redact(str(exc)), severity="warning", timeout=6)

    def run_review(self, argument: str):
        # Explicit command only; browsing never generates or overwrites a review.
        if self.reviewing:
            raise ValueError("A review is already running")
        session_path = None
        if argument:
            from wy.ingestion.codex import CodexCollector

            args = shlex.split(argument)
            if len(args) != 1:
                raise ValueError("Use /review followed by one session ID or quoted transcript path")
            selected = args[0]
            session_path = Path(selected)
            if not session_path.is_file():
                matches = [s for s in CodexCollector().discover() if s["id"] == selected]
                if len(matches) != 1:
                    raise ValueError("Session not found; supply its JSONL path")
                session_path = Path(matches[0]["path"])
        self.notify("Analyzing local changes…")
        self.reviewing = True
        self.review_worker(session_path)

    @work(thread=True, exclusive=True)
    def review_worker(self, session_path: Path | None):
        try:
            result = service.review(Path(self.review.root), session_path=session_path)
        except (ValueError, OSError, sqlite3.DatabaseError, subprocess.SubprocessError) as exc:
            self.reviewing = False
            self.call_from_thread(self.notify, redact(str(exc)), severity="error", timeout=8)
            return
        self.call_from_thread(self.apply_review, result)

    def apply_review(self, result: Review):
        self.reviewing = False
        self.review = result
        self.session = presentation.saved_session(self.review)
        self.routes = []
        self.route_index = -1
        self.selected_decision = None
        self.selected_event = None
        self.current_file = None
        self.paired_event = None
        for selector in ("#saved-code", "#current-code", "#event-text"):
            self.query_one(selector, TextArea).load_text("")
        for selector in ("#citations", "#related"):
            self.query_one(selector, OptionList).clear_options()
        self.query_one("#decision-copy", Static).update("No decisions found. See Origin for review limitations.")
        self.query_one("#code-status", Static).update("Select a code citation to inspect saved and current source.")
        self.query_one("#event-label", Static).update("Select a stored event")
        self.query_one("#paired-event", Button).disabled = True
        self.query_one("#filter", Input).value = ""
        self.query_one("#event-filter", Input).value = ""
        self.on_mount()
        self.notify("Fresh offline review saved")


def explore(review: Review):
    Explorer(review).run()


def start(repo: Path):
    try:
        review = service.load_review(repo)
    except ValueError as exc:
        if str(exc) != "No review named latest":
            raise
        review = Review(id="No saved review", root=str(repo_root(repo)), created_at="Not reviewed",
                        warnings=["Type /review to analyze current changes, or /review <session-id-or-path> to include Codex evidence."])
    explore(review)
