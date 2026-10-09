use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
fn repo() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    git(
        root.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    git(root.path(), &["config", "user.name", "Test"]);
    fs::write(root.path().join("lib.rs"), "pub fn answer() -> i32 { 1 }\n").unwrap();
    git(root.path(), &["add", "lib.rs"]);
    git(root.path(), &["commit", "-qm", "baseline"]);
    root
}
fn call(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wy"))
        .arg("--repo")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}
fn json(root: &Path, args: &[&str]) -> Value {
    let out = call(root, args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn offline_review_baseline_reflection_and_staleness() {
    let root = repo();
    let baseline = json(root.path(), &["snapshot", "--json"]);
    fs::write(root.path().join("lib.rs"), "pub fn answer() -> i32 { 2 }\n").unwrap();
    let review = json(
        root.path(),
        &[
            "review",
            "--source",
            "none",
            "--baseline",
            baseline["id"].as_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(review["changes"][0]["file"], "lib.rs");
    let focused = json(
        root.path(),
        &["focus", "lib.rs:1", "Why this return value?", "--json"],
    );
    assert_eq!(focused["provenance"], "unexplained");
    let req = json(root.path(), &["reflection-request", "--json"]);
    assert_eq!(req["decisions"].as_array().unwrap().len(), 1);
    let evidence = json(root.path(), &["evidence", "1", "1", "--json"]);
    assert_eq!(evidence["current"]["status"], "unchanged");
    let response = json(
        root.path(),
        &["ask", "1", "What alternatives exist?", "--json"],
    );
    assert!(response["answer"].as_str().unwrap().contains("Options"));
    fs::write(root.path().join("lib.rs"), "pub fn answer() -> i32 { 3 }\n").unwrap();
    let cached = json(root.path(), &["decisions", "--json"]);
    assert_eq!(cached["decisions"][0]["stale"], true);
    assert!(!call(root.path(), &["ask", "1", "Why?"]).status.success());
}
#[test]
fn cli_rejects_conflicts_noninteractive_workspace_and_bad_citations() {
    let root = repo();
    assert!(
        !call(
            root.path(),
            &["review", "--base", "HEAD", "--baseline", "x"]
        )
        .status
        .success()
    );
    let out = call(root.path(), &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("interactive terminal"));
    assert!(
        !call(root.path(), &["reasoning-evidence", "0"])
            .status
            .success()
    );
    assert!(call(root.path(), &["--help"]).status.success());
    for command in ["reason", "why"] {
        let out = call(root.path(), &[command, "--help"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
