use serde_json::{Value, json};
use wy::{session_reader, session_work, history, storage::Store};

#[test]
fn legacy_saved_headers_do_not_invalidate_the_workspace_review() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::process::Command::new("git").args(["init", "-q"]).current_dir(&root).status().unwrap();
    let store = Store::open(&root).unwrap();
    let valid = json!({"id":"legacy","agent":"codex","cwd":root,"path":root.join("gone.jsonl"),"events":[],"warnings":[],"format":"codex-rollout"});
    store.put("session", "valid", &valid).unwrap();
    let mut missing_agent = valid.clone();
    missing_agent.as_object_mut().unwrap().remove("agent");
    store.put("session", "missing-agent", &missing_agent).unwrap();
    let mut unsupported = valid.clone();
    unsupported["agent"] = json!("unsupported");
    store.put("session", "unsupported-agent", &unsupported).unwrap();
    let mut missing_path = valid.clone();
    missing_path["path"] = Value::Null;
    store.put("session", "missing-path", &missing_path).unwrap();
    let (headers, skipped) = store.saved_session_catalog().unwrap();
    assert_eq!(skipped, 3);
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0]["storage_key"], "valid");
    let review = wy::service::workspace_review(&root, &wy::service::ReviewOptions { source: "codex".into() }).unwrap();
    wy::validate("Review", &review).unwrap();
    assert_eq!(review["sessions"].as_array().unwrap().len(), 1);
    assert!(review["warnings"].as_array().unwrap().iter().any(|warning| wy::s(warning).contains("Skipped 3 saved session headers")));
    assert_eq!(store.get("session", "missing-agent").unwrap(), missing_agent);
}

fn work(root: &std::path::Path, count: usize) -> Value {
    json!({"root":root,"id":"review","session":{"storage_key":"snapshot","id":"session","agent":"codex"},
        "warnings":[],"edits":[{"id":"edit","text":format!("{}EDIT_END", "x".repeat(60000))}],
        "turns":(0..count).map(|index| json!({
            "request":{"id":format!("request-{index}"),"kind":"user","text":"request"},
            "messages":[{"kind":"assistant","text":format!("{}RESPONSE_{index}_END", "r".repeat(50000))}],
            "activity":[],"edit_indices":[0]
        })).collect::<Vec<_>>()})
}

#[test]
fn pages_fetch_only_ids_and_selected_detail_without_shortening() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    session_reader::index(&root, &work(&root, 45)).unwrap();
    let page = session_reader::page(&root, "snapshot", 21).unwrap();
    assert_eq!(page["request_count"], 45);
    assert_eq!(page["request_page"].as_array().unwrap().len(), 20);
    assert_eq!(page["request_page"][0]["request"]["id"], "request-20");
    assert!(page["request_page"][0]["request"]["text"].is_null());
    assert_eq!(page["turns"].as_array().unwrap().len(), 1);
    assert_eq!(page["turns"][0]["request"]["id"], "request-21");
    assert!(page["turns"][0]["messages"][0]["text"].as_str().unwrap().ends_with("RESPONSE_21_END"));
    assert!(page["edits"][0]["text"].as_str().unwrap().ends_with("EDIT_END"));
    assert_eq!(page["turns"][0]["edit_indices"], json!([0]));
    assert_eq!(session_reader::page(&root, "snapshot", usize::MAX).unwrap()["selected_request"], 44);
    // Bad content in a different request must not affect this request or its ID page.
    let connection = rusqlite::Connection::open(root.join(".wy/wy.sqlite3")).unwrap();
    connection.execute("UPDATE session_requests SET data='invalid' WHERE session_key='snapshot' AND ordinal=44", []).unwrap();
    assert!(session_reader::page(&root, "snapshot", 40).is_ok());
    assert!(session_reader::page(&root, "snapshot", 44).is_err());
}

#[test]
fn empty_session_has_an_empty_request_page() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    session_reader::index(&root, &work(&root, 0)).unwrap();
    let page = session_reader::page(&root, "snapshot", 0).unwrap();
    assert_eq!(page["turns"], json!([]));
    assert_eq!(page["request_page"], json!([]));
}

#[test]
fn legacy_snapshots_index_on_demand_and_keep_more_than_64_complete_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::process::Command::new("git").args(["init", "-q"]).current_dir(&root).status().unwrap();
    let source = root.join("session.jsonl");
    let text = format!("{}END", "a".repeat(50000));
    let patch = format!("*** Begin Patch\n{}\n*** End Patch", (0..70).map(|i| format!("*** Add File: file-{i}.rs\n+{}", if i == 0 { text.as_str() } else { "code" })).collect::<Vec<_>>().join("\n"));
    let rows = [
        json!({"type":"session_meta","payload":{"id":"session","cwd":root}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"write files"}]}}),
        json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","input":patch,"call_id":"edit"}}),
    ];
    std::fs::write(&source, rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
    let session = history::collect(&source).unwrap();
    let key = session_work::snapshot_key(&session);
    Store::open(&root).unwrap().put("session", &key, &session).unwrap();
    let review = json!({"root":root,"id":"review"});
    session_reader::ensure_index(&root, &key, &review).unwrap();
    std::fs::remove_file(&source).unwrap();
    let page = session_reader::page(&root, &key, 0).unwrap();
    assert_eq!(page["edits"].as_array().unwrap().len(), 70);
    assert_eq!(page["edits"][0]["text"], text);
    assert_eq!(page["edits"][0]["truncated"], false);
    assert_eq!(page["turns"][0]["edit_indices"].as_array().unwrap().len(), 70);
}
