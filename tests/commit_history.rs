use serde_json::{Value, json};
use std::{fs, path::Path};
use wy::{commits, history, repository, service, storage::Store};

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

/// Same review the app runs on startup and on `r`.
fn capture(root: &Path, source: &str) -> Value {
    service::review(root, &service::ReviewOptions { source: source.into() }).unwrap()
}

/// Commit lookup used by `g` and `/commit`.
fn lookup(root: &Path, revision: &str) -> anyhow::Result<Value> {
    commits::lookup(root, revision, "both")
}

/// Explicit association used by `/link`.
fn link(root: &Path, revision: &str, review: Option<&str>) -> anyhow::Result<Value> {
    commits::link(root, revision, review)
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
    transcript(root, text);
    capture(root, "codex")
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
    assert!(lookup(root, &base).is_err());
    // Neither a later transcript nor a new 'latest' review can replace the capture.
    transcript(root, "Later unrelated discussion.");
    let latest = capture(root, "none");
    fs::remove_file(root.join(".codex/sessions/coding.jsonl")).unwrap();
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 99 }\n").unwrap();
    git(root, &["add", "lib.rs"]);
    git(root, &["commit", "-qm", "later change"]);
    let found = lookup(root, &commit[..10]).unwrap();
    assert_eq!(found["commit"], commit);
    assert_eq!(found["links"][0]["review_id"], reviewed["id"]);
    assert_eq!(found["links"][0]["association"], "snapshot-match");
    assert_eq!(
        found["sessions"][0]["events"][0]["text"],
        "Return 42 because it is the agreed API value."
    );
    assert_eq!(found["links"].as_array().unwrap().len(), 1);
    let filtered = commits::lookup(root, &commit, "claude").unwrap();
    assert!(filtered["sessions"].as_array().unwrap().is_empty());
    assert_eq!(service::load(root).unwrap()["id"], latest["id"]);
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
    let missing = lookup(root, &commit).unwrap_err();
    assert!(missing.to_string().contains("/link"));
    // A user can explicitly associate the broader review with a partial commit.
    let linked = link(root, &commit, reviewed["id"].as_str()).unwrap();
    assert_eq!(linked["association"], "explicit");
    assert_eq!(lookup(root, &commit).unwrap()["sessions"][0]["id"], "coding");
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
    assert!(lookup(root, "HEAD").is_err());
}

#[test]
fn explicit_links_support_tags_multiple_reviews_and_distinct_snapshots() {
    let dir = repo();
    let root = dir.path();
    let original = review(root, "Original explanation.");
    // Refreshing with an unchanged transcript reuses the same pinned snapshot.
    let same = capture(root, "codex");
    let later = review(root, "Later explanation.");
    let commit = git(root, &["rev-parse", "HEAD"]);
    git(root, &["tag", "-a", "-m", "release", "release"]);
    for id in [&original["id"], &same["id"], &later["id"], &original["id"]] {
        link(root, "release", id.as_str()).unwrap();
    }
    let found = lookup(root, &commit[..8]).unwrap();
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
    assert_eq!(link(root, "HEAD", None).unwrap()["review_id"], later["id"]);
    let saved = history::saved(&service::load(root).unwrap()).unwrap();
    assert_eq!(saved[0]["id"], "coding");
}

#[test]
fn rejects_invalid_commits_empty_reviews_and_missing_or_unrelated_evidence() {
    let dir = repo();
    let root = dir.path();
    capture(root, "none");
    let empty = link(root, "HEAD", None).unwrap_err();
    assert!(empty.to_string().contains("no saved conversations"));
    let reviewed = review(root, "Original explanation.");
    for revision in ["missing-revision", "HEAD:lib.rs", "--all"] {
        assert!(link(root, revision, None).is_err());
        assert!(lookup(root, revision).is_err());
    }
    let mut foreign = reviewed.clone();
    foreign["root"] = json!("/another-repository");
    let store = Store::open(root).unwrap();
    store.put("review", "foreign", &foreign).unwrap();
    assert!(link(root, "HEAD", Some("foreign")).is_err());
    link(root, "HEAD", None).unwrap();
    let key = reviewed["sessions"][0]["storage_key"].as_str().unwrap();
    let mut session = store.get("session", key).unwrap();
    session["cwd"] = json!("/another-repository");
    store.put("session", key, &session).unwrap();
    assert!(lookup(root, "HEAD").is_err());
    let db = rusqlite::Connection::open(root.join(".wy/wy.sqlite3")).unwrap();
    db.execute("DELETE FROM artifacts WHERE kind='session' AND id=?", [key])
        .unwrap();
    let missing = lookup(root, "HEAD").unwrap_err();
    assert!(format!("{missing:#}").contains("snapshot is missing"));
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
    link(root, "HEAD", None).unwrap();
    assert_eq!(
        lookup(root, "HEAD").unwrap()["sessions"][0]["events"][0]["text"],
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
        lookup(root, "HEAD").unwrap()["links"][0]["association"],
        "snapshot-match"
    );
}
