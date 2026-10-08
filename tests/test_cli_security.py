import json
from pathlib import Path

from typer.testing import CliRunner

from wy.cli import app
from wy.security import redact

runner = CliRunner()


def test_full_cli_flow(repo):
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    args = ["--repo", str(repo), "--json"]
    result = runner.invoke(app, ["review", *args])
    assert result.exit_code == 0, result.output
    decision = json.loads(result.output)["decisions"][0]
    for command in ["decisions", "gaps"]:
        result = runner.invoke(app, [command, *args])
        assert result.exit_code == 0 and json.loads(result.output)["decisions"]
    result = runner.invoke(app, ["explain", "worker.py:1", *args])
    assert json.loads(result.output)["id"] == decision["id"]
    result = runner.invoke(app, ["ask", decision["id"], "What alternatives exist?", *args])
    assert "asyncio" in json.loads(result.output)["answer"]
    assert runner.invoke(app, ["review", "--repo", str(repo)]).exit_code == 0
    result = runner.invoke(app, ["explain", "worker.py:900", *args])
    assert result.exit_code == 1 and "No decision" in result.output


def test_secrets_redacted_before_storage_and_output(repo):
    secret = "sk-" + "a" * 32
    (repo / "worker.py").write_text(f'api_key = "{secret}"\npool = ThreadPoolExecutor(8)\n')
    result = runner.invoke(app, ["review", "--repo", str(repo), "--json"])
    assert result.exit_code == 0
    assert secret not in result.output
    assert "[REDACTED]" in result.output
    for artifact in (repo / ".wy").glob("*"):
        assert secret.encode() not in artifact.read_bytes()
    result = runner.invoke(app, ["snapshot", "--repo", str(repo), "--json"])
    assert result.exit_code == 0
    for artifact in (repo / ".wy").glob("*"):
        assert secret.encode() not in artifact.read_bytes()


def test_redaction_common_shapes():
    for value in [
        'password="hidden-password"',
        "postgres://user:hidden-password@host/db",
        "Bearer hidden-password",
        "-----BEGIN PRIVATE KEY-----\nhidden-password\n-----END PRIVATE KEY-----",
    ]:
        assert "hidden-password" not in redact(value)


def test_json_session_flag(repo):
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    fixture = Path(__file__).parent / "fixtures" / "codex-exec.jsonl"
    result = runner.invoke(app, ["review", "--repo", str(repo), "--session", str(fixture), "--json"])
    assert result.exit_code == 0, result.output
    assert json.loads(result.output)["decisions"][0]["provenance"] == "recorded"


def test_cached_read_never_uses_provider(repo, monkeypatch):
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    runner.invoke(app, ["review", "--repo", str(repo)])

    def fail(*args, **kwargs):
        raise AssertionError("Provider must not run")

    monkeypatch.setattr("wy.cli.OllamaProvider.from_env", fail)
    assert runner.invoke(app, ["decisions", "--repo", str(repo)]).exit_code == 0


def test_multiline_secret_redaction_preserves_source_line_numbers(repo):
    source = 'key = """-----BEGIN PRIVATE KEY-----\nVERY-PRIVATE-BODY\n-----END PRIVATE KEY-----"""\npool = ThreadPoolExecutor(8)\n'
    (repo / "worker.py").write_text(source)
    result = runner.invoke(app, ["review", "--repo", str(repo), "--json"])
    assert result.exit_code == 0, result.output
    data = json.loads(result.output)
    assert "VERY-PRIVATE-BODY" not in result.output
    assert data["decisions"][0]["location"]["start_line"] == 4
    assert redact(source).count("\n") == source.count("\n")
