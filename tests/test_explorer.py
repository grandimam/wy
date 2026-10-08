import asyncio
import json
from pathlib import Path

from typer.testing import CliRunner

from wy import service, trace
from wy.cli import app
from wy.models import Evidence, Session
from wy.security import digest
from wy.storage import Store

runner = CliRunner()


def reviewed(repo, with_session=False):
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    fixture = Path(__file__).parent / "fixtures" / "codex-exec.jsonl"
    path = repo / ".wy" / "test-session.jsonl"
    if with_session:
        path.parent.mkdir(exist_ok=True)
        path.write_text(fixture.read_text() + json.dumps({"type": "session_meta", "payload": {"id": "exec-example", "cwd": str(repo)}}) + "\n")
    return service.review(repo, path if with_session else None)


def test_index_origin_number_and_json_compatibility(repo):
    review = reviewed(repo, True)
    output = runner.invoke(app, ["decisions", "--repo", str(repo)])
    assert output.exit_code == 0, output.output
    for text in ("Review origin", review.id, "exec-example", "Recorded", "Decision index", "Not proven"):
        assert text in output.output
    output = runner.invoke(app, ["explain", "1", "--repo", str(repo)])
    assert "I chose ThreadPoolExecutor" in output.output
    assert "pool = ThreadPoolExecutor" not in output.output
    output = runner.invoke(app, ["explain", "1", "--repo", str(repo), "--json"])
    assert json.loads(output.output) == review.decisions[0].model_dump(mode="json")
    output = runner.invoke(app, ["explain", "0", "--repo", str(repo)])
    assert output.exit_code == 1


def test_gaps_preserve_full_review_numbers(repo):
    review = reviewed(repo, True)
    first = review.decisions[0]
    first.unresolved_questions = []
    second = first.model_copy(deep=True)
    second.id = "decision-second"
    second.provenance = "unexplained"
    second.question = "Second question"
    review.decisions.append(second)
    Store(repo).save_review(review)
    output = runner.invoke(app, ["gaps", "--repo", str(repo)])
    assert "1 of 2 decisions" in output.output
    assert "Second question" in output.output
    assert service.select(service.load_review(repo), "2").id == second.id


def test_trace_source_moved_changed_ambiguous_missing_and_redacted(repo):
    review = reviewed(repo)
    decision = review.decisions[0]
    evidence = decision.evidence[0]
    original = (repo / "worker.py").read_text()
    assert trace.inspect(review, decision, evidence)["current"]["status"] == "unchanged"
    (repo / "worker.py").write_text("# new line\n" + original)
    current = trace.inspect(review, decision, evidence)["current"]
    assert current["status"] == "unique_excerpt_match"
    assert current["anchor_line"] == 2
    assert evidence.start_line == 1
    (repo / "worker.py").write_text(original * 2)
    assert trace.inspect(review, decision, evidence)["current"]["status"] == "ambiguous"
    (repo / "worker.py").write_text('password="super-private"\npool = ThreadPoolExecutor(12)\n')
    output = runner.invoke(app, ["evidence", "1", "1", "--repo", str(repo), "--json"])
    assert output.exit_code == 0, output.output
    assert "super-private" not in output.output
    assert json.loads(output.output)["current"]["status"] == "excerpt_changed"
    (repo / "worker.py").unlink()
    result = trace.inspect(review, decision, evidence)
    assert result["current"] is None
    assert result["evidence"]["excerpt"] == original.strip()


def test_session_trace_has_neighbors_and_tool_pairs(repo):
    review = reviewed(repo, True)
    store = Store(repo)
    key = review.sessions[0].storage_key
    session = Session.model_validate(store.get("session", key))
    # Deliberately nonadjacent pair: relationship comes from call_id, not proximity.
    session.events[0].call_id = "call-a"
    session.events[-1].call_id = "call-a"
    store.put("session", key, session.model_dump())
    event = session.events[0]
    evidence = Evidence(id="session-" + event.id, kind="session", file=session.path,
                        start_line=event.source_line, end_line=event.source_line,
                        excerpt=event.text, event_id=event.id)
    data = trace.inspect(review, review.decisions[0], evidence)
    assert data["context"][0]["id"] == event.id
    assert data["linked_tool_events"][0]["id"] == session.events[-1].id
    output = runner.invoke(app, ["session", "--repo", str(repo), "--event", "event-5", "--json"])
    assert json.loads(output.output)["kind"] == "assistant"
    assert "PRIVATE RECORD" not in output.output


def test_interactive_navigation_is_read_only_and_offline(repo, monkeypatch):
    review = reviewed(repo, True)
    before = (repo / ".wy" / "review.json").read_bytes()

    def fail(*args, **kwargs):
        raise AssertionError("Must not run provider or write review")

    monkeypatch.setattr("wy.cli.OllamaProvider.from_env", fail)
    monkeypatch.setattr(Store, "save_review", fail)
    from textual.widgets import Input, TabbedContent, TextArea

    from wy.explorer import Explorer, MessageScreen

    async def navigate():
        workspace = Explorer(review)
        async with workspace.run_test(size=(140, 45)) as pilot:
            await pilot.pause()
            decision = review.decisions[0]
            workspace.navigate(("evidence", decision.id, decision.evidence[0].id))
            await pilot.pause()
            assert workspace.query_one("#tabs", TabbedContent).active == "code-tab"
            assert "ThreadPoolExecutor" in workspace.query_one("#current-code", TextArea).text
            workspace.action_back()
            assert workspace.query_one("#tabs", TabbedContent).active == "decision-tab"
            workspace.action_forward()
            assert workspace.query_one("#tabs", TabbedContent).active == "code-tab"
            citation = next(e for e in decision.evidence if e.kind == "session")
            workspace.navigate(("evidence", decision.id, citation.id))
            await pilot.pause()
            assert workspace.query_one("#tabs", TabbedContent).active == "conversation-tab"
            assert "I chose" in workspace.query_one("#event-text", TextArea).text
            workspace.step_event(1)
            assert workspace.selected_event == "event-6"
            workspace.step_event(-1)
            assert workspace.selected_event == "event-5"
            await pilot.press("ctrl+j")
            workspace.query_one("#command", Input).value = "/help"
            await pilot.press("enter")
            assert isinstance(workspace.screen, MessageScreen)
            await pilot.click("#close-message")
            workspace.query_one("#command", Input).value = "/find missing"
            workspace.query_one("#command", Input).focus()
            await pilot.press("enter")
            await pilot.pause()
            assert not workspace.decision_nodes
            await pilot.press("ctrl+q")

    asyncio.run(navigate())
    assert (repo / ".wy" / "review.json").read_bytes() == before


def test_bare_wy_opens_workspace_and_help_does_not(repo, monkeypatch):
    started = []
    monkeypatch.setattr("wy.cli.explorer.start", lambda path: started.append(path))
    assert runner.invoke(app, ["--repo", str(repo)]).exit_code == 0
    assert started == [repo]
    assert runner.invoke(app, ["--help"]).exit_code == 0
    assert started == [repo]


def test_first_run_opens_without_inventing_a_review(repo, monkeypatch):
    from wy import explorer

    seen = []
    monkeypatch.setattr(explorer, "explore", lambda review: seen.append(review))
    explorer.start(repo)
    assert seen[0].id == "No saved review"
    assert not seen[0].decisions
    assert not (repo / ".wy" / "review.json").exists()


def test_review_command_creates_review_inside_workspace(repo):
    from textual.widgets import Input

    from wy.explorer import Explorer
    from wy.models import Review

    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")

    async def navigate():
        workspace = Explorer(Review(id="No saved review", root=str(repo), created_at="Not reviewed"))
        async with workspace.run_test(size=(120, 40)) as pilot:
            command = workspace.query_one("#command", Input)
            command.value = "/review"
            command.focus()
            await pilot.press("enter")
            await workspace.workers.wait_for_complete()
            await pilot.pause()
            assert workspace.review.id.startswith("review-")
            assert workspace.selected_decision
            assert (repo / ".wy" / "review.json").exists()

    asyncio.run(navigate())


def test_missing_session_and_literal_markup(repo):
    review = reviewed(repo)
    review.decisions[0].question = "[bold]literal[/bold]"
    Store(repo).save_review(review)
    output = runner.invoke(app, ["explain", "1", "--repo", str(repo)])
    assert "[bold]literal[/bold]" in output.output
    assert "None" in output.output
    output = runner.invoke(app, ["session", "--repo", str(repo)])
    assert output.exit_code == 1 and "No saved session" in output.output


def test_trace_refuses_unsafe_current_source(repo):
    review = reviewed(repo)
    unsafe = Evidence(id="bad", kind="code", file="../outside.py", start_line=1,
                      end_line=1, excerpt="saved", snapshot_hash=digest("saved"))
    assert trace.inspect(review, review.decisions[0], unsafe)["current"] is None
