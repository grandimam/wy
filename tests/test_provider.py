import json

import pytest
from pydantic import ValidationError

from wy import service
from wy.models import Justification
from wy.provider import OllamaProvider, enrich


@pytest.fixture
def decision(repo):
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    return service.review(repo).decisions[0]


def test_external_sending_requires_explicit_permission():
    with pytest.raises(ValueError, match="WY_ALLOW_REMOTE"):
        OllamaProvider("https://example.invalid/api/chat", "model")
    with pytest.raises(ValueError):
        OllamaProvider("http://example.invalid/api/chat", "model", True)
    OllamaProvider("https://example.invalid/api/chat", "model", True)
    OllamaProvider("http://127.0.0.1:11434/api/chat", "model")
    with pytest.raises(ValueError):
        OllamaProvider("http://user:password@127.0.0.1/api/chat", "model")


def fake_transport(monkeypatch, payload):
    class Response:
        def __enter__(self):
            return self

        def __exit__(self, *args):
            pass

        def read(self, count):
            return json.dumps(payload).encode()

    class Transport:
        def open(self, request, timeout):
            body = json.loads(request.data)
            assert body["format"]["additionalProperties"] is False
            assert "UNTRUSTED" in body["messages"][0]["content"]
            return Response()

    monkeypatch.setattr("wy.provider.build_opener", lambda *args: Transport())


def test_valid_structured_output_and_usage(monkeypatch, decision):
    result = {
        "explanation": "This may preserve the blocking API.",
        "provenance": "inferred",
        "evidence_ids": [decision.evidence[0].id],
        "alternatives": ["asyncio"],
        "assumptions": ["Tasks are I/O bound"],
        "unresolved_questions": ["Why eight workers?"],
    }
    fake_transport(
        monkeypatch, {"message": {"content": json.dumps(result)}, "prompt_eval_count": 100, "eval_count": 40}
    )
    provider = OllamaProvider("http://127.0.0.1:11434/api/chat", "test")
    enhanced = enrich(decision, provider)
    assert enhanced.provenance == "inferred"
    assert enhanced.location == decision.location
    assert provider.input_tokens == 100 and provider.output_tokens == 40


@pytest.mark.parametrize("content", ["not-json", "{}", '{"provenance":"recorded"}', "[]"])
def test_malformed_response_rejected(monkeypatch, decision, content):
    fake_transport(monkeypatch, {"message": {"content": content}})
    with pytest.raises(ValueError, match="invalid structured"):
        OllamaProvider("http://127.0.0.1:11434/api/chat", "test").justify(decision)


def test_fabricated_citation_rejected(monkeypatch, decision):
    result = Justification(
        explanation="Unverified",
        provenance="inferred",
        evidence_ids=["made-up"],
        alternatives=[],
        assumptions=[],
        unresolved_questions=[],
    )
    fake_transport(monkeypatch, {"message": {"content": result.model_dump_json()}})
    with pytest.raises(ValueError, match="not supplied"):
        OllamaProvider("http://127.0.0.1:11434/api/chat", "test").justify(decision)


def test_failed_model_keeps_offline_decisions(repo):
    class Broken:
        input_tokens = output_tokens = 0

        def justify(self, decision):
            raise ValueError("malformed")

    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    result = service.review(repo, provider=Broken())
    assert result.decisions[0].provenance == "unexplained"
    assert any("kept conservative" in w for w in result.warnings)


def test_inferred_without_evidence_schema_and_host_checks(decision):
    with pytest.raises(ValidationError):
        type(decision).model_validate({**decision.model_dump(), "provenance": "inferred", "evidence": []})
