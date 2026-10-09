//! wy has no subcommands: the binary only opens the interactive workspace.
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
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
    fs::write(root.path().join("lib.rs"), "pub fn answer() -> i32 { 1 }\n").unwrap();
    root
}
fn call(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wy"))
        .arg("--repo")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}
#[test]
fn help_and_version_describe_the_interactive_app() {
    let root = repo();
    let help = call(root.path(), &["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("interactive"));
    assert!(!text.contains("Commands:"));
    assert!(call(root.path(), &["--version"]).status.success());
}
#[test]
fn removed_subcommands_and_json_output_are_rejected() {
    let root = repo();
    for args in [
        vec!["review"],
        vec!["reason"],
        vec!["why", "lib.rs:1"],
        vec!["decisions"],
        vec!["--json"],
    ] {
        let out = call(root.path(), &args);
        assert!(!out.status.success(), "{args:?} should be rejected");
    }
}
#[test]
fn requires_a_terminal_and_a_git_repository() {
    let root = repo();
    let out = call(root.path(), &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("run it in a terminal"));
    let outside = tempfile::tempdir().unwrap();
    assert!(!call(outside.path(), &[]).status.success());
}
