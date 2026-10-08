import json
from pathlib import Path

from wy.ingestion.codex import CodexCollector

FIXTURES = Path(__file__).parent / "fixtures"


def test_rollout_observable_only_and_deduplicated():
    session = CodexCollector().collect(FIXTURES / "codex-rollout.jsonl")
    assert session.id == "rollout-example"
    assert [e.kind for e in session.events] == ["user", "read", "tool_output", "change", "assistant"]
    assert session.events[3].files == ["worker.py"]
    assert "PRIVATE" not in session.model_dump_json()
    assert "ENCRYPTED" not in session.model_dump_json()
    assert session.warnings == ["Skipped malformed session line 11"]


def test_exec_format():
    session = CodexCollector().collect(FIXTURES / "codex-exec.jsonl")
    assert session.id == "exec-example"
    assert session.format == "codex-exec-json"
    assert [e.kind for e in session.events] == ["search", "assistant", "change", "test"]
    assert "PRIVATE" not in session.model_dump_json()


def test_discovery_reads_metadata_only(tmp_path):
    folder = tmp_path / "sessions"
    folder.mkdir()
    (folder / "test.jsonl").write_text(
        json.dumps({"type": "session_meta", "payload": {"id": "abc", "cwd": "/repo"}}) + "\nnot-json\n"
    )
    result = CodexCollector().discover(tmp_path)
    assert result[0]["id"] == "abc"
    assert "events" not in result[0]


def test_malformed_shapes_and_secrets(tmp_path):
    f = tmp_path / "session.jsonl"
    f.write_text(
        '[]\n{"type":"response_item","payload":null}\n'
        + json.dumps(
            {
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": 'password="test-private-value"'}],
                },
            }
        )
    )
    session = CodexCollector().collect(f)
    assert "test-private-value" not in session.model_dump_json()
    assert session.events
