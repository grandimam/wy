import asyncio
import json
import os
from pathlib import Path

import pytest
from textual.widgets import Input, OptionList, Select, TextArea
from typer.testing import CliRunner

from wy import history, service, trace
from wy.cli import app
from wy.explorer import Explorer
from wy.ingestion.claude import ClaudeCollector
from wy.repository import sources
from wy.storage import Store


def write_session(home, agent, cwd, session_id="shared", name=None):
    path = home / ("sessions" if agent == "codex" else "projects/project") / f"{name or session_id}.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    reason = f"I chose ThreadPoolExecutor in worker.py because {agent} found blocking I/O."
    if agent == "codex":
        rows = [
            {"type": "session_meta", "payload": {"id": session_id, "cwd": str(cwd) if cwd else None, "timestamp": "2026-10-08T01:00:00Z"}},
            {"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Update worker.py"}]}},
            {"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": reason}]}},
        ]
    else:
        meta = {"sessionId": session_id, "cwd": str(cwd) if cwd else None, "timestamp": "2026-10-08T02:00:00Z"}
        rows = [
            {"type": "file-history-snapshot", "snapshot": {}},
            {**meta, "type": "user", "uuid": "u1", "message": {"content": "Update worker.py"}},
            {**meta, "type": "assistant", "uuid": "a1", "message": {"content": [
                {"type": "thinking", "thinking": "PRIVATE THOUGHT"},
                {"type": "text", "text": reason},
                {"type": "tool_use", "id": "tool-a", "name": "Read", "input": {"file_path": "worker.py", "password": "hidden-secret"}},
            ]}},
            {**meta, "type": "user", "uuid": "u2", "message": {"content": [
                {"type": "tool_result", "tool_use_id": "tool-a", "content": [{"type": "text", "text": "ThreadPoolExecutor in worker.py"}]}]}},
        ]
    path.write_text("\n".join(map(json.dumps, rows)) + "\n")
    return path


def homes():
    return Path(os.environ["CODEX_HOME"]), Path(os.environ["CLAUDE_CONFIG_DIR"])


def test_discovery_is_repository_scoped_for_both_agents(repo, tmp_path):
    codex, claude = homes()
    for agent, home in (("codex", codex), ("claude", claude)):
        write_session(home, agent, repo)
        write_session(home, agent, tmp_path.parent / "unrelated", "other")
        write_session(home, agent, None, "unknown")
    entries = history.discover(repo)
    assert {(e["agent"], e["id"]) for e in entries} == {("codex", "shared"), ("claude", "shared")}
    assert all(e["agent"] == "claude" for e in history.discover(repo, "claude"))
    assert history.discover(repo, "none") == []
    result = CliRunner().invoke(app, ["sessions", "--repo", str(repo), "--json"])
    assert result.exit_code == 0, result.output
    assert len(json.loads(result.output)) == 2
    assert "unrelated" not in result.output
    with pytest.raises(ValueError, match="ambiguous"):
        history.resolve(repo, ["shared"])
    assert history.resolve(repo, ["claude:shared"])[0].parent.name == "project"


def test_claude_skips_private_thinking_and_links_tools(repo):
    path = write_session(homes()[1], "claude", repo)
    session = ClaudeCollector().collect(path)
    assert session.agent == "claude"
    assert [e.kind for e in session.events] == ["user", "assistant", "read", "tool_output"]
    assert session.events[-1].call_id == session.events[-2].call_id == "tool-a"
    assert "PRIVATE THOUGHT" not in session.model_dump_json()
    assert "hidden-secret" not in session.model_dump_json()
    assert session.events[1].source_line == 3


def test_review_both_preserves_identity_and_stored_snapshot(repo):
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    codex, claude = homes()
    paths = [write_session(codex, "codex", repo), write_session(claude, "claude", repo)]
    review = service.review(repo, history_source="both")
    assert review.session_id is None
    assert len(review.sessions) == 2
    citations = [e for e in review.decisions[0].evidence if e.kind == "session"]
    assert {e.agent for e in citations} == {"codex", "claude"}
    assert len({e.id for e in citations}) == len(citations)
    assert review.decisions[0].provenance == "recorded"
    for citation in citations:
        data = trace.inspect(review, review.decisions[0], citation)
        assert data["agent"] == citation.agent
        assert data["session_id"] == "shared"
        assert all(e["id"] in {x.id for x in history.session_for_evidence(review, citation).events} for e in data["context"])
    before = history.saved_sessions(review)[0].model_dump_json()
    paths[0].write_text(paths[0].read_text().replace("blocking I/O", "different conditions"))
    service.review(repo, history_source="both")
    assert history.saved_sessions(review)[0].model_dump_json() == before
    claude_only = service.review(repo, history_source="claude")
    assert [s.agent for s in claude_only.sessions] == ["claude"]


def test_unrelated_and_unverified_explicit_sessions_are_rejected(repo):
    before = service.review(repo)
    for cwd in (None, repo.parent / "different"):
        path = write_session(homes()[0], "codex", cwd, "unrelated")
        with pytest.raises(ValueError, match="refusing unrelated or unverified"):
            service.review(repo, session_path=path)
        assert Store(repo).latest().id == before.id


def test_subdirectory_matches_but_nested_repository_does_not(repo):
    import subprocess

    sub = repo / "src"
    sub.mkdir()
    assert history.belongs(str(sub), repo)
    nested = sub / "independent"
    subprocess.run(["git", "init", "-q", str(nested)], check=True)
    assert not history.belongs(str(nested), repo)
    assert not history.belongs("relative/path", repo)


def test_project_agent_dirs_are_context_not_application_sources(repo):
    for directory in (".codex", ".claude"):
        path = repo / directory / "settings.json"
        path.parent.mkdir()
        path.write_text('{"example": "ThreadPoolExecutor(8)"}')
    texts, _, _ = sources(repo)
    assert not any(name.startswith((".codex/", ".claude/")) for name in texts)


def test_session_browser_keeps_providers_separate(repo):
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    write_session(homes()[0], "codex", repo)
    write_session(homes()[1], "claude", repo)
    review = service.review(repo, history_source="both")

    async def navigate():
        workspace = Explorer(review)
        async with workspace.run_test(size=(145, 48)) as pilot:
            await pilot.pause()
            decision = review.decisions[0]
            for agent in ("codex", "claude"):
                citation = next(e for e in decision.evidence if e.agent == agent)
                workspace.navigate(("evidence", decision.id, citation.id))
                await pilot.pause()
                assert workspace.session.agent == agent
                assert agent + " found blocking" in workspace.query_one("#event-text", TextArea).text
            workspace.query_one("#session-select", Select).value = "claude:shared"
            await pilot.pause()
            options = workspace.query_one("#events", OptionList)
            assert all(options.get_option_at_index(i).id.startswith("claude:") for i in range(options.option_count))
            command = workspace.query_one("#command", Input)
            command.value = "/event event-2"
            command.focus()
            await pilot.press("enter")
            # Codex's event-2 remains tied to Codex, not the selected Claude session.
            assert workspace.session.agent == "codex"

    asyncio.run(navigate())
