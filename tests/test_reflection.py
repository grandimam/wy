import json

import pytest
from typer.testing import CliRunner

from wy import reflection, service
from wy.cli import app
from wy.models import ReflectionResponse


@pytest.fixture
def packet(repo):
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    service.review(repo)
    return reflection.request(repo)


def response(packet):
    decision = packet["decisions"][0]
    return ReflectionResponse(
        request_id=packet["request_id"],
        review_id=packet["review_id"],
        agent="Codex",
        model=None,
        context="separate_review",
        reflections=[
            {
                "decision_id": decision["id"],
                "assessment": "revise",
                "rationale": "The worker count needs workload measurements.",
                "evidence_ids": [decision["evidence"][0]["id"]],
                "alternatives": ["Make the worker count configurable"],
                "assumptions": ["The workload is I/O bound"],
                "suggested_change": "Measure latency under realistic concurrency.",
                "uncertainty": "The original intent is unavailable.",
            }
        ],
    )


def test_reflection_preserves_original_explanation_and_source(repo, packet):
    before = service.load_review(repo).decisions[0]
    source = (repo / "worker.py").read_bytes()
    reflection.record(repo, response(packet))
    after = service.load_review(repo).decisions[0]
    assert after.model_dump(exclude={"reflections"}) == before.model_dump(exclude={"reflections"})
    assert after.reflections[0].assessment == "revise"
    assert after.reflections[0].identity_verification == "self_reported"
    assert (repo / "worker.py").read_bytes() == source
    with pytest.raises(ValueError, match="already been recorded"):
        reflection.record(repo, response(packet))


@pytest.mark.parametrize("invalid", ["citation", "duplicate", "missing", "review", "no_citations"])
def test_invalid_response_never_changes_review(repo, packet, invalid):
    result = response(packet)
    if invalid == "citation":
        result.reflections[0].evidence_ids = ["invented"]
    elif invalid == "duplicate":
        result.reflections *= 2
    elif invalid == "missing":
        result.reflections[0].decision_id = "another-decision"
    elif invalid == "review":
        result.review_id = "another-review"
    else:
        result.reflections[0].evidence_ids = []
    before = (repo / ".wy/review.json").read_bytes()
    with pytest.raises(ValueError):
        reflection.record(repo, result)
    assert (repo / ".wy/review.json").read_bytes() == before


def test_stale_and_superseded_requests_rejected(repo, packet):
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(16)\n")
    with pytest.raises(ValueError, match="changed"):
        reflection.record(repo, response(packet))
    service.review(repo)
    with pytest.raises(ValueError, match="different review"):
        reflection.record(repo, response(packet))


def test_reflection_secrets_redacted(repo, packet):
    result = response(packet)
    result.reflections[0].rationale = 'password="private-value"'
    reflection.record(repo, result)
    assert "private-value" not in (repo / ".wy/review.json").read_text()


def test_focus_covers_existing_undetected_code_and_validates_locations(repo):
    service.review(repo)
    result = reflection.focus(repo, "worker.py:1", "Why a standalone function?", ["worker.py:2"])
    assert result.provenance == "unexplained"
    assert result.location.symbol == "work"
    assert len(result.evidence) == 2
    packet = reflection.request(repo, result.id)
    assert packet["decisions"][0]["id"] == result.id
    for target in ["../worker.py:1", "/etc/passwd:1", "worker.py:99", "worker.py:0"]:
        with pytest.raises(ValueError):
            reflection.focus(repo, target, "Why?", [])
    (repo / "worker.py").write_text("def work():\n    return 2\n")
    with pytest.raises(ValueError, match="changed"):
        reflection.focus(repo, "worker.py:1", "Why?", [])


def test_cli_roundtrip(repo, tmp_path):
    runner = CliRunner()
    args = ["--repo", str(repo), "--json"]
    assert runner.invoke(app, ["review", *args]).exit_code == 0
    focused = runner.invoke(app, ["focus", "worker.py:1", "Why this function?", *args])
    assert focused.exit_code == 0, focused.output
    packet = runner.invoke(app, ["reflection-request", *args])
    assert packet.exit_code == 0, packet.output
    path = tmp_path / "response.json"
    path.write_text(response(json.loads(packet.output)).model_dump_json())
    recorded = runner.invoke(app, ["record-reflection", str(path), *args])
    assert recorded.exit_code == 0, recorded.output
    explained = runner.invoke(app, ["explain", "worker.py:1", "--repo", str(repo)])
    assert "Retrospective assessment: revise" in explained.output
