use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use wy::{repository, storage::Store};

fn git(root: &Path, args: &[&str]) -> String {
    repository::git(root, args, true).unwrap().trim().to_owned()
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    git(root, &["config", "user.name", "Test"]);
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 1 }\n").unwrap();
    git(root, &["add", "lib.rs"]);
    git(root, &["commit", "-qm", "baseline"]);
    dir
}

fn call(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wy"))
        .arg("--repo")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

fn output(root: &Path, args: &[&str]) -> Value {
    let out = call(root, args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn transcript(root: &Path, text: &str) -> std::path::PathBuf {
    let directory = root.join(".codex/sessions");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("coding.jsonl");
    let rows = [
        json!({"type":"session_meta","payload":{"id":"coding","cwd":root.canonicalize().unwrap(),"timestamp":wy::now()}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}}),
    ];
    fs::write(
        &path,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    path
}

fn review(root: &Path, text: &str) -> Value {
    let path = transcript(root, text);
    output(
        root,
        &["review", "--session", path.to_str().unwrap(), "--json"],
    )
}

#[test]
fn automatically_matches_committed_review_and_reads_pinned_conversation() {
    let dir = repo();
    let root = dir.path();
    let base = git(root, &["rev-parse", "HEAD"]);
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    let reviewed = review(root, "Return 42 because it is the agreed API value.");
    git(root, &["add", "lib.rs"]);
    git(root, &["commit", "-qm", "return agreed answer"]);
    let commit = git(root, &["rev-parse", "HEAD"]);
    // The pre-change HEAD is never itself treated as the resulting commit.
    assert!(
        !call(root, &["sessions", "--commit", &base])
            .status
            .success()
    );
    // Neither a later transcript nor a new 'latest' review can replace the capture.
    transcript(root, "Later unrelated discussion.");
    let latest = output(root, &["review", "--source", "none", "--json"]);
    fs::remove_file(root.join(".codex/sessions/coding.jsonl")).unwrap();
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 99 }\n").unwrap();
    git(root, &["add", "lib.rs"]);
    git(root, &["commit", "-qm", "later change"]);
    let found = output(root, &["session", "--commit", &commit[..10], "--json"]);
    assert_eq!(found["commit"], commit);
    assert_eq!(found["links"][0]["review_id"], reviewed["id"]);
    assert_eq!(found["links"][0]["association"], "snapshot-match");
    assert_eq!(
        found["sessions"][0]["events"][0]["text"],
        "Return 42 because it is the agreed API value."
    );
    let summary = output(root, &["sessions", "--commit", &commit, "--json"]);
    assert_eq!(summary["sessions"][0]["event_count"], 1);
    assert!(summary["sessions"][0]["events"].is_null());
    assert_eq!(summary["links"].as_array().unwrap().len(), 1);
    let selected = output(
        root,
        &[
            "session",
            "--commit",
            &commit,
            "--id",
            "codex:coding",
            "--event",
            "event-2",
            "--json",
        ],
    );
    assert_eq!(selected, found);
    for args in [
        vec!["session", "--commit", &commit, "--id", "missing"],
        vec!["session", "--commit", &commit, "--event", "event-999"],
    ] {
        assert!(!call(root, &args).status.success());
    }
    let text = call(root, &["session", "--commit", &commit]);
    assert!(String::from_utf8_lossy(&text.stdout).contains("agreed API value"));
    let filtered = output(
        root,
        &[
            "sessions", "--commit", &commit, "--source", "claude", "--json",
        ],
    );
    assert!(filtered["sessions"].as_array().unwrap().is_empty());
    assert_eq!(output(root, &["decisions", "--json"])["id"], latest["id"]);
    let store = Store::open(root).unwrap();
    assert_eq!(store.linked_reviews(&commit).unwrap().len(), 1);
}

#[test]
fn partial_or_modified_commits_do_not_automatically_match() {
    let dir = repo();
    let root = dir.path();
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    fs::write(root.join("extra.rs"), "pub fn extra() {}\n").unwrap();
    let reviewed = review(root, "Change both source files.");
    git(root, &["add", "lib.rs"]);
    git(root, &["commit", "-qm", "partial commit"]);
    let commit = git(root, &["rev-parse", "HEAD"]);
    let missing = call(root, &["session", "--commit", &commit]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("wy link"));
    // A user can explicitly associate the broader review with a partial commit.
    let linked = output(
        root,
        &[
            "link",
            &commit,
            "--review",
            reviewed["id"].as_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(linked["association"], "explicit");
    assert_eq!(
        output(root, &["session", "--commit", &commit, "--json"])["sessions"][0]["id"],
        "coding"
    );
}

#[test]
fn matching_uses_original_source_hashes_not_redacted_text() {
    let dir = repo();
    let root = dir.path();
    fs::write(
        root.join("config.rs"),
        "const PASSWORD: &str = \"first-secret\";\n",
    )
    .unwrap();
    // These assignments normalize to the same redacted text.
    let first = "password = \"first-secret\"\n";
    let second = "password = \"second-secret\"\n";
    assert_eq!(wy::security::redact(first), wy::security::redact(second));
    fs::write(root.join("config.toml"), first).unwrap();
    review(root, "Save configuration.");
    fs::write(root.join("config.toml"), second).unwrap();
    git(root, &["add", "config.rs", "config.toml"]);
    git(root, &["commit", "-qm", "different configuration"]);
    assert!(
        !call(root, &["session", "--commit", "HEAD"])
            .status
            .success()
    );
}

#[test]
fn explicit_links_support_tags_multiple_reviews_and_distinct_snapshots() {
    let dir = repo();
    let root = dir.path();
    let original = review(root, "Original explanation.");
    let same = output(
        root,
        &[
            "review",
            "--session",
            root.join(".codex/sessions/coding.jsonl").to_str().unwrap(),
            "--json",
        ],
    );
    let later = review(root, "Later explanation.");
    let commit = git(root, &["rev-parse", "HEAD"]);
    git(root, &["tag", "-a", "-m", "release", "release"]);
    for id in [&original["id"], &same["id"], &later["id"], &original["id"]] {
        output(
            root,
            &[
                "link",
                "release",
                "--review",
                id.as_str().unwrap(),
                "--json",
            ],
        );
    }
    let found = output(root, &["session", "--commit", &commit[..8], "--json"]);
    assert_eq!(found["links"].as_array().unwrap().len(), 3);
    assert_eq!(found["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(
        found["sessions"][0]["review_ids"].as_array().unwrap().len(),
        2
    );
    assert_ne!(
        found["sessions"][0]["storage_key"],
        found["sessions"][1]["storage_key"]
    );
    assert_eq!(
        found["sessions"][0]["events"][0]["text"],
        "Original explanation."
    );
    assert_eq!(
        found["sessions"][1]["events"][0]["text"],
        "Later explanation."
    );
    assert_eq!(
        output(root, &["link", "HEAD", "--json"])["review_id"],
        later["id"]
    );
    assert_eq!(output(root, &["session", "--json"])[0]["id"], "coding");
}

#[test]
fn rejects_invalid_commits_empty_reviews_and_missing_or_unrelated_evidence() {
    let dir = repo();
    let root = dir.path();
    output(root, &["review", "--source", "none", "--json"]);
    let empty = call(root, &["link", "HEAD"]);
    assert!(!empty.status.success());
    assert!(String::from_utf8_lossy(&empty.stderr).contains("no saved conversations"));
    let reviewed = review(root, "Original explanation.");
    for revision in ["missing-revision", "HEAD:lib.rs", "--all"] {
        assert!(!call(root, &["link", "--", revision]).status.success());
        assert!(
            !call(root, &["sessions", &format!("--commit={revision}")])
                .status
                .success()
        );
    }
    let mut foreign = reviewed.clone();
    foreign["root"] = json!("/another-repository");
    let store = Store::open(root).unwrap();
    store.put("review", "foreign", &foreign).unwrap();
    assert!(
        !call(root, &["link", "HEAD", "--review", "foreign"])
            .status
            .success()
    );
    output(root, &["link", "HEAD", "--json"]);
    let key = reviewed["sessions"][0]["storage_key"].as_str().unwrap();
    let mut session = store.get("session", key).unwrap();
    session["cwd"] = json!("/another-repository");
    store.put("session", key, &session).unwrap();
    assert!(
        !call(root, &["session", "--commit", "HEAD"])
            .status
            .success()
    );
    let db = rusqlite::Connection::open(root.join(".wy/wy.sqlite3")).unwrap();
    db.execute("DELETE FROM artifacts WHERE kind='session' AND id=?", [key])
        .unwrap();
    let missing = call(root, &["session", "--commit", "HEAD"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("snapshot is missing"));
}

#[test]
fn existing_v1_store_and_first_commit_are_supported() {
    let dir = repo();
    let root = dir.path();
    review(root, "Saved before the index existed.");
    let db = rusqlite::Connection::open(root.join(".wy/wy.sqlite3")).unwrap();
    db.execute_batch("DROP TABLE commit_reviews; PRAGMA user_version=1;")
        .unwrap();
    drop(db);
    output(root, &["link", "HEAD", "--json"]);
    assert_eq!(
        output(root, &["session", "--commit", "HEAD", "--json"])["sessions"][0]["events"][0]["text"],
        "Saved before the index existed."
    );

    let fresh = tempfile::tempdir().unwrap();
    let root = fresh.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    git(root, &["config", "user.name", "Test"]);
    fs::write(root.join("lib.rs"), "pub fn first() {}\n").unwrap();
    review(root, "Create the first function.");
    git(root, &["add", "lib.rs"]);
    git(root, &["commit", "-qm", "initial"]);
    assert_eq!(
        output(root, &["sessions", "--commit", "HEAD", "--json"])["links"][0]["association"],
        "snapshot-match"
    );
}
