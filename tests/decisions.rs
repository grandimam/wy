use serde_json::{Value, json};
use wy::{arr, decisions, insights};

// Run the real CLI path in a subprocess so PATH overrides cannot affect parallel tests.
#[cfg(unix)]
#[test]
fn discovery_pipeline_uses_isolated_stub_cli_and_persists_validated_brief() {
    use std::{os::unix::fs::PermissionsExt, process::Command};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    wy::repository::git(root, &["init", "-q"], true).unwrap();
    std::fs::write(root.join("worker.rs"), "fn persist() {}\n").unwrap();
    let bin = root.join(".wy-test-bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::write(root.join(".gitignore"), ".wy-test-bin/\n.wy/\n").unwrap();
    let mut result = response();
    result["decisions"][0]["files"] = json!(["worker.rs"]);
    result["decisions"][0]["evidence_ids"] =
        json!([format!("diff-{}", &wy::security::digest("worker.rs")[..12])]);
    std::fs::write(bin.join("response.json"), result.to_string()).unwrap();
    let script = bin.join("codex");
    std::fs::write(&script,"#!/bin/sh\nout=''\nwhile [ $# -gt 0 ]; do\n if [ \"$1\" = '--output-last-message' ]; then shift; out=$1; fi\n shift\ndone\n/bin/cat > \"$WY_DECISION_TEST_ROOT/.wy-test-bin/prompt.txt\"\n/bin/cp \"$WY_DECISION_TEST_ROOT/.wy-test-bin/response.json\" \"$out\"\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "discovery_child", "--nocapture"])
        .env("PATH", path)
        .env("WY_DECISION_TEST_ROOT", root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[cfg(unix)]
#[test]
fn discovery_child() {
    let Some(root) = std::env::var_os("WY_DECISION_TEST_ROOT") else {
        return;
    };
    let root = std::path::Path::new(&root);
    let r = wy::service::review(
        root,
        &wy::service::ReviewOptions {
            source: "none".into(),
        },
    )
    .unwrap();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let brief = decisions::run(root, &r, "codex", &cancel, |_| {}).unwrap();
    assert_eq!(brief["mode"], "assessment");
    assert_eq!(brief["decisions"][0]["status"], "inferred");
    assert!(decisions::saved(root, &r).unwrap().is_some());
    let prompt = std::fs::read_to_string(root.join(".wy-test-bin/prompt.txt")).unwrap();
    assert!(prompt.contains("fn persist()") && prompt.contains("EVIDENCE PACKET"));
    // Invalid citations must never create another persisted artifact.
    std::fs::write(
        root.join(".wy-test-bin/response.json"),
        response().to_string(),
    )
    .unwrap();
    assert!(decisions::run(root, &r, "codex", &cancel, |_| {}).is_err());
    assert_eq!(
        wy::storage::Store::open(root)
            .unwrap()
            .recent("decision_brief", 40)
            .unwrap()
            .len(),
        1
    );
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(decisions::run(root, &r, "codex", &cancel, |_| {}).is_err());
    cancel.store(false, std::sync::atomic::Ordering::Relaxed);
    std::fs::write(root.join("worker.rs"), "fn changed() {}\n").unwrap();
    assert!(
        decisions::run(root, &r, "codex", &cancel, |_| {})
            .unwrap_err()
            .to_string()
            .contains("Source changed")
    );
    // Session discovery uses immutable captured edits even when today's files
    // differ, are committed, or have disappeared. The CLI is still only our stub.
    let canonical = root.canonicalize().unwrap();
    let path = root.join(".codex/sessions/coding.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let rows = [
        json!({"type":"session_meta","payload":{"id":"coding","cwd":canonical,"timestamp":"2024-01-01T00:00:00Z"}}),
        json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"edit-1","input":"*** Begin Patch\n*** Add File: worker.rs\n+SESSION_CAPTURE_ONLY\n*** End Patch"}}),
    ];
    std::fs::write(
        &path,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    std::fs::write(root.join("worker.rs"), "CURRENT_CODE_MUST_NOT_BE_SENT").unwrap();
    let r = wy::service::review(
        root,
        &wy::service::ReviewOptions {
            source: "codex".into(),
        },
    )
    .unwrap();
    let sessions = wy::history::saved(&r).unwrap();
    let work = wy::session_work::build(&r, &sessions[0]);
    let mut output = response();
    output["decisions"][0]["files"] = json!(["worker.rs"]);
    output["decisions"][0]["evidence_ids"] =
        json!([format!("session-edit-{}", wy::s(&work["edits"][0]["id"]))]);
    std::fs::write(root.join(".wy-test-bin/response.json"), output.to_string()).unwrap();
    let brief = decisions::run(root, &work, "codex", &cancel, |_| {}).unwrap();
    assert_eq!(brief["scope_kind"], "session");
    let prompt = std::fs::read_to_string(root.join(".wy-test-bin/prompt.txt")).unwrap();
    assert!(prompt.contains("SESSION_CAPTURE_ONLY"));
    assert!(!prompt.contains("CURRENT_CODE_MUST_NOT_BE_SENT"));
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(root.join("worker.rs")).unwrap();
    decisions::run(root, &work, "codex", &cancel, |_| {}).unwrap();
    assert!(decisions::saved(root, &work).unwrap().is_some());
}

fn review() -> Value {
    json!({"root":"/repo","id":"review-1","head":"abc","comparison_base":"abc","file_hashes":{"worker.rs":"v1"},"history_source":"all","sessions":[],"changes":[{"file":"worker.rs","diff":"+persist(job)","diff_truncated":false},{"file":"schema.sql","diff":"+CREATE TABLE jobs","diff_truncated":false}]})
}
fn record(file: &str) -> Value {
    json!({"file":file,"decision":"Persist jobs in PostgreSQL","reason":"Jobs must survive restarts","alternatives":["Redis"],"tradeoffs":["Reuse infrastructure; add polling overhead"],"evidence":["durability requirement"]})
}
fn event(records: Vec<Value>) -> Value {
    json!({"id":"e1","kind":"assistant","provenance":{"source_type":"original_turn"},"text":records.iter().map(|r|format!("WY_DECISION\n{r}")).collect::<Vec<_>>().join("\n")})
}
fn packet() -> Value {
    json!({"evidence":[{"id":"diff-1","kind":"diff","file":"worker.rs","text":"+persist(job)"},{"id":"diff-2","kind":"diff","file":"schema.sql","text":"+CREATE TABLE jobs"},{"id":"chat","kind":"session","role":"assistant","text":"Use PostgreSQL so jobs survive restarts.","provenance":{"source_type":"original_turn"}}]})
}
fn response() -> Value {
    json!({"decisions":[{"choice":"Persist jobs in PostgreSQL","reason":"Durable storage","significance":"Jobs survive process restarts","status":"inferred","files":["worker.rs","schema.sql"],"alternatives":["Redis"],"tradeoffs":["Polling overhead"],"evidence_ids":["diff-1","diff-2"],"quote":"","quote_id":""}],"unknowns":[]})
}

#[test]
fn offline_groups_identical_records_across_files_and_excludes_out_of_scope_history() {
    let mut conflicting = record("worker.rs");
    conflicting["reason"] = json!("A different reason");
    let session = json!({"id":"s1","agent":"codex","path":"/history","events":[event(vec![record("worker.rs"),record("schema.sql"),record("unrelated.rs"),conflicting])]});
    let brief = decisions::recorded(&review(), &[session]);
    assert_eq!(arr(&brief["decisions"]).len(), 2);
    assert_eq!(
        brief["decisions"][0]["files"],
        json!(["worker.rs", "schema.sql"])
    );
    assert_eq!(brief["decisions"][0]["status"], "recorded");
    assert_eq!(arr(&brief["packet"]["evidence"]).len(), 3);
    assert!(!brief.to_string().contains("\"files\":[\"unrelated.rs\"]"));
}
#[test]
fn multiple_records_survive_invalid_neighbors_but_never_promote_summaries() {
    let mut e = event(vec![record("worker.rs"), record("schema.sql")]);
    e["text"] = json!(format!(
        "WY_DECISION not json\n{}",
        e["text"].as_str().unwrap()
    ));
    assert_eq!(insights::decisions(&e).len(), 2);
    e["provenance"]["source_type"] = json!("compaction_summary");
    assert!(insights::decisions(&e).is_empty());
    e["provenance"]["source_type"] = json!("original_turn");
    e["kind"] = json!("user");
    assert!(insights::decisions(&e).is_empty());
}
#[test]
fn clean_scope_and_missing_history_never_invent_choices() {
    assert!(arr(&decisions::recorded(&review(), &[])["decisions"]).is_empty());
    let mut r = review();
    r["changes"] = json!([]);
    assert!(arr(&decisions::recorded(&r,&[json!({"events":[event(vec![record("worker.rs")]) ]})])["decisions"]).is_empty());
}
#[test]
fn decisions_require_in_scope_diff_citations_for_every_file() {
    decisions::validate(&response(), &packet(), &review()).unwrap();
    let mut d = response();
    d["decisions"][0]["evidence_ids"] = json!(["diff-1"]);
    assert!(decisions::validate(&d, &packet(), &review()).is_err());
    d["decisions"][0]["files"] = json!(["unrelated.rs"]);
    assert!(decisions::validate(&d, &packet(), &review()).is_err());
    let mut d = response();
    d["decisions"][0]["evidence_ids"] = json!(["invented", "diff-1", "diff-2"]);
    assert!(decisions::validate(&d, &packet(), &review()).is_err());
    decisions::validate(
        &json!({"decisions":[],"unknowns":["No consequential choices established"]}),
        &packet(),
        &review(),
    )
    .unwrap();
}
#[test]
fn recorded_rationale_requires_exact_original_assistant_quote() {
    let mut d = response();
    d["decisions"][0]["status"] = json!("recorded");
    assert!(decisions::validate(&d, &packet(), &review()).is_err());
    d["decisions"][0]["quote"] = json!("Use PostgreSQL so jobs survive restarts.");
    d["decisions"][0]["quote_id"] = json!("chat");
    d["decisions"][0]["evidence_ids"] = json!(["diff-1", "diff-2", "chat"]);
    decisions::validate(&d, &packet(), &review()).unwrap();
    let mut p = packet();
    p["evidence"][2]["role"] = json!("user");
    assert!(decisions::validate(&d, &p, &review()).is_err());
    p = packet();
    p["evidence"][2]["provenance"]["source_type"] = json!("compaction_summary");
    assert!(decisions::validate(&d, &p, &review()).is_err());
    d["decisions"][0]["status"] = json!("inferred");
    d["decisions"][0]["quote"] = json!("");
    d["decisions"][0]["quote_id"] = json!("");
    assert!(decisions::validate(&d, &p, &review()).is_err());
    d["decisions"][0]["status"] = json!("unknown");
    decisions::validate(&d, &p, &review()).unwrap();
}
#[test]
fn code_only_supports_inferred_benefits_not_recorded_intent() {
    let mut p = packet();
    p["evidence"].as_array_mut().unwrap().pop();
    decisions::validate(&response(), &p, &review()).unwrap();
    let mut d = response();
    d["decisions"][0]["status"] = json!("recorded");
    assert!(decisions::validate(&d, &p, &review()).is_err());
}
#[test]
fn cache_is_bound_to_diff_base_source_hashes_and_captured_history() {
    let original = review();
    let key = decisions::scope_key(&original);
    for (field, value) in [
        ("head", json!("new-head")),
        ("comparison_base", json!("new-base")),
        ("file_hashes", json!({"worker.rs":"v2"})),
        ("history_source", json!("none")),
        ("sessions", json!([{"storage_key":"new-snapshot"}])),
        ("changes", json!([])),
    ] {
        let mut r = original.clone();
        r[field] = value;
        assert_ne!(decisions::scope_key(&r), key, "{field}");
    }
    let mut r = original.clone();
    r["id"] = json!("another-capture");
    r["created_at"] = json!("tomorrow");
    assert_eq!(decisions::scope_key(&r), key);
    let dir = tempfile::tempdir().unwrap();
    let store = wy::storage::Store::open(dir.path()).unwrap();
    let brief = decisions::recorded(&original, &[]);
    store.put("decision_brief", "saved", &brief).unwrap();
    assert!(decisions::saved(dir.path(), &original).unwrap().is_some());
    r["changes"] = json!([]);
    assert!(decisions::saved(dir.path(), &r).unwrap().is_none());
}
