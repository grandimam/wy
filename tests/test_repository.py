import subprocess

import pytest

from wy import service
from wy.repository import changed_symbols, parse_diff, sources
from wy.security import digest


def test_baseline_excludes_preexisting_edits(repo):
    (repo / "unrelated.py").write_text("cache = Redis()\n")
    snapshot = service.snapshot(repo)
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    result = service.review(repo, baseline_id=snapshot["id"])
    assert len(result.decisions) == 1
    assert result.decisions[0].location.file == "worker.py"
    assert result.decisions[0].attribution == "since-baseline"
    assert result.baseline_id == snapshot["id"]
    assert "redis" not in result.model_dump_json().lower()


def test_no_baseline_honest_attribution(repo):
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    result = service.review(repo)
    assert result.decisions[0].attribution == "unknown"
    assert any("cannot be distinguished" in w for w in result.warnings)


def test_staged_and_untracked_included_without_mutations(repo):
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    subprocess.run(["git", "-C", str(repo), "add", "worker.py"], check=True)
    (repo / "cache.py").write_text("cache = Redis()\n")
    old = {p.name: p.read_bytes() for p in repo.glob("*.py")}
    result = service.review(repo)
    assert len(result.decisions) == 2
    assert old == {p.name: p.read_bytes() for p in repo.glob("*.py")}


def test_anchor_and_dependency_staleness(repo):
    (repo / "worker.py").write_text("pool = ThreadPoolExecutor(8)\n")
    (repo / "client.py").write_text("requests.get(url)\n")
    result = service.review(repo)
    assert not service.load_review(repo).decisions[0].stale
    (repo / "client.py").write_text("await client.get(url)\n")
    assert service.load_review(repo).decisions[0].stale
    with pytest.raises(ValueError, match="changed"):
        service.ask(service.load_review(repo).decisions[0], "Why?")
    (repo / "client.py").write_text("requests.get(url)\n")
    assert not service.load_review(repo).decisions[0].stale
    (repo / "worker.py").write_text("\npool = ThreadPoolExecutor(8)\n")
    assert service.load_review(repo).decisions[0].stale
    assert result.decisions[0].snapshot_hash == digest("pool = ThreadPoolExecutor(8)\n")


def test_import_diff_validate_target(repo):
    target = "pool = ThreadPoolExecutor(8)\n"
    (repo / "worker.py").write_text(target)
    diff = repo / "change.diff"
    diff.write_text(
        "diff --git a/worker.py b/worker.py\n--- a/worker.py\n+++ b/worker.py\n@@ -1,2 +1 @@\n-def work():\n-    return 1\n+pool = ThreadPoolExecutor(8)\n"
    )
    assert len(service.review(repo, diff_path=diff).decisions) == 1
    (repo / "worker.py").write_text("\n" + target)
    with pytest.raises(ValueError, match="does not match"):
        service.review(repo, diff_path=diff)


def test_diff_unsafe_paths_and_deleted_files():
    assert parse_diff("--- a/x.py\n+++ b/../../x.py\n@@ -0,0 +1 @@\n+Redis()\n") == []
    assert parse_diff("--- a/x.py\n+++ /dev/null\n@@ -1 +0,0 @@\n-Redis()\n") == []


def test_symbols_nested_and_syntax_error():
    source = "class Worker:\n    def run(self):\n        return 1\n"
    assert [s.symbol for s in changed_symbols("x.py", source, {3})] == ["Worker", "Worker.run"]
    assert changed_symbols("x.py", "invalid syntax !!!!", {1}) == []


def test_excludes_credentials_symlinks_and_binary(repo, tmp_path_factory):
    outside = tmp_path_factory.mktemp("outside") / "private.py"
    outside.write_text('password="TOPSECRET"')
    (repo / "alias.py").symlink_to(outside)
    (repo / ".env").write_text("PASSWORD=TOPSECRET")
    (repo / "credentials.json").write_text('{"password":"TOPSECRET"}')
    (repo / "binary.py").write_bytes(b"\0TOPSECRET")
    texts, _, _ = sources(repo)
    assert set(texts) == {"worker.py"}


def test_unrelated_session_rejected(repo):
    path = repo / "session.jsonl"
    path.write_text('{"type":"session_meta","payload":{"id":"other","cwd":"/another/repo"}}\n')
    with pytest.raises(ValueError, match="does not match"):
        service.review(repo, session_path=path)


def test_empty_repo_and_no_changes(repo):
    assert service.review(repo).decisions == []
    subprocess.run(["git", "init", "-q", str(repo / "new")], check=True)
    (repo / "new" / "cache.py").write_text("cache = Redis()\n")
    assert len(service.review(repo / "new").decisions) == 1


def test_symlink_storage_refused(repo, tmp_path_factory):
    (repo / ".wy").symlink_to(tmp_path_factory.mktemp("outside"))
    with pytest.raises(ValueError, match="symlink"):
        service.review(repo)


def test_baseline_separates_changes_within_same_file(repo):
    (repo / "worker.py").write_text("cache = Redis()\n\ndef work():\n    return 1\n")
    before = service.snapshot(repo)
    (repo / "worker.py").write_text(
        "cache = Redis()\n\ndef work():\n    pool = ThreadPoolExecutor(8)\n    return 1\n"
    )
    result = service.review(repo, baseline_id=before["id"])
    assert len(result.decisions) == 1
    assert result.decisions[0].category == "concurrency"
    assert result.changes[0].added_lines == [4]
    assert result.changes[0].symbols[0].symbol == "work"


def test_repository_diff_driver_is_not_executed(repo):
    subprocess.run(["git", "-C", str(repo), "config", "diff.external", "touch SHOULD_NOT_EXIST"], check=True)
    (repo / "worker.py").write_text("cache = Redis()\n")
    assert service.review(repo).decisions
    assert not (repo / "SHOULD_NOT_EXIST").exists()
