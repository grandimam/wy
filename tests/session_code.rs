use serde_json::{Value, json};
use std::{fs, path::Path};
use wy::{history, repository, service};

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.invalid"],
    ] {
        repository::git(dir.path(), &args, true).unwrap();
    }
    fs::write(dir.path().join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    repository::git(dir.path(), &["add", "lib.rs"], true).unwrap();
    repository::git(
        dir.path(),
        &["commit", "-qm", "Agent code already committed"],
        true,
    )
    .unwrap();
    dir
}
fn transcript(root: &Path, name: &str, rows: Vec<Value>) -> std::path::PathBuf {
    let folder = root.join(".codex/sessions");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(format!("{name}.jsonl"));
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
fn meta(root: &Path, id: &str) -> Value {
    json!({"type":"session_meta","payload":{"id":id,"cwd":root.canonicalize().unwrap(),"timestamp":wy::now()}})
}
fn call(name: &str, input: &str, id: &str) -> Value {
    json!({"type":"response_item","timestamp":wy::now(),"payload":{"type":"custom_tool_call","name":name,"input":input,"call_id":id}})
}
fn message(role: &str, text: &str) -> Value {
    json!({"type":"response_item","timestamp":wy::now(),"payload":{"type":"message","role":role,"content":[{"type":"input_text","text":text}]}})
}

#[test]
fn committed_code_is_discovered_from_wrapped_edits_and_keeps_its_conversation() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    let patch = "*** Begin Patch\n*** Update File: lib.rs\n@@\n-pub fn answer() -> i32 { 1 }\n+pub fn answer() -> i32 { 42 }\n*** End Patch";
    let wrapped = format!("text(await tools.apply_patch({}));", json!(patch));
    let path = transcript(
        &root,
        "coding",
        vec![
            meta(&root, "coding"),
            message("user", "Return the agreed answer."),
            message(
                "assistant",
                "I will return 42 because that is the agreed API value.",
            ),
            call("exec", &wrapped, "patch-1"),
        ],
    );
    let review = service::review(
        &root,
        &service::ReviewOptions {
            source: "codex".into(),
            sessions: vec![path],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(wy::arr(&review["changes"]).is_empty());
    let recent = wy::arr(&review["recent_code"]);
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0]["file"], "lib.rs");
    assert_eq!(recent[0]["state"], "recorded");
    assert!(wy::s(&recent[0]["text"]).contains("+pub fn answer()"));
    let reference = history::edit_ref(&recent[0]);
    let (edit, session) = history::saved_edit(&root, &reference).unwrap();
    let packet = wy::reasoning::packet_with_edit(
        &review,
        "Why?",
        Some("lib.rs"),
        &[],
        None,
        Some((&edit, &session)),
    )
    .unwrap();
    assert_eq!(packet["focus_session_edit"]["id"], reference["id"]);
    let evidence = wy::arr(&packet["evidence"]);
    assert!(
        evidence
            .iter()
            .any(|e| e["role"] == "change" && wy::s(&e["text"]).contains("+pub fn answer()"))
    );
    assert!(
        evidence
            .iter()
            .any(|e| e["role"] == "assistant" && wy::s(&e["text"]).contains("agreed API value"))
    );
    // The captured edit remains inspectable and explainable after the file is removed.
    fs::remove_file(root.join("lib.rs")).unwrap();
    let fresh = service::review(
        &root,
        &service::ReviewOptions {
            source: "none".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        wy::reasoning::packet_with_edit(
            &fresh,
            "Why?",
            Some("lib.rs"),
            &[],
            None,
            Some((&edit, &session))
        )
        .is_ok()
    );
    let mut forged = reference;
    forged["file"] = json!("other.rs");
    assert!(history::saved_edit(&root, &forged).is_err());
}

#[test]
fn recent_edits_are_bounded_scoped_and_do_not_treat_failed_calls_as_code() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    let patch = |file: &str, body: &str| {
        format!("*** Begin Patch\n*** Add File: {file}\n+{body}\n*** End Patch")
    };
    let mut rows = vec![meta(&root, "calls")];
    rows.push(call(
        "apply_patch",
        &patch("lib.rs", "first version"),
        "first",
    ));
    rows.push(call(
        "apply_patch",
        &patch("lib.rs", "latest version"),
        "latest",
    ));
    rows.push(call(
        "apply_patch",
        &patch("failed.rs", "failed code"),
        "failed",
    ));
    rows.push(json!({"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"failed","output":"apply_patch verification failed"}}));
    rows.push(call(
        "apply_patch",
        &patch("../outside.rs", "outside"),
        "outside",
    ));
    rows.push(call("apply_patch", &patch(".env", "secret"), "secret"));
    let example = format!(
        "tools.apply_patch({})",
        json!(patch("example.rs", "not a call"))
    );
    rows.push(call(
        "exec",
        &format!("const example = {};", json!(example)),
        "quoted",
    ));
    rows.push(call(
        "exec",
        "tools.apply_patch(`*** Begin Patch\n*** Add File: ${name}\n+code\n*** End Patch`)",
        "dynamic",
    ));
    let mut old = call("apply_patch", &patch("old.rs", "old code"), "old");
    old["timestamp"] = json!((chrono::Utc::now() - chrono::Duration::days(8)).to_rfc3339());
    rows.push(old);
    let path = transcript(&root, "calls", rows);
    let session = history::collect(&path).unwrap();
    let recent = history::recent_code(&root, &[session]);
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0]["text"], "latest version");
}

#[test]
fn claude_writes_and_edits_preserve_code_before_event_text_truncation() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    let large = format!("{}\nend_of_generated_code", "line of code\n".repeat(1500));
    let tool = |id: &str, name: &str, input: Value| json!({"type":"assistant","sessionId":"claude","cwd":root,"timestamp":wy::now(),"uuid":id,"message":{"content":[{"type":"tool_use","name":name,"id":id,"input":input}]}});
    let rows = vec![
        tool(
            "write",
            "Write",
            json!({"file_path":root.join("large.rs"),"content":large}),
        ),
        tool(
            "edit",
            "Edit",
            json!({"file_path":root.join("lib.rs"),"old_string":"42","new_string":"43"}),
        ),
        tool(
            "bad",
            "Write",
            json!({"file_path":root.join("failed.rs"),"content":"failed"}),
        ),
        json!({"type":"user","sessionId":"claude","cwd":root,"timestamp":wy::now(),"uuid":"result","message":{"content":[{"type":"tool_result","tool_use_id":"bad","is_error":true,"content":"permission denied"}]}}),
        tool(
            "private",
            "Write",
            json!({"file_path":root.join("config.rs"),"content":"let token = \"sk-abcdefghijklmnopqrst\";"}),
        ),
    ];
    let path = transcript(&root, "claude", rows);
    let session = history::collect(&path).unwrap();
    let recent = history::recent_code(&root, &[session]);
    assert_eq!(recent.len(), 3);
    let full = recent.iter().find(|e| e["file"] == "large.rs").unwrap();
    assert!(wy::s(&full["text"]).ends_with("end_of_generated_code"));
    assert!(
        recent
            .iter()
            .any(|e| e["file"] == "lib.rs" && wy::s(&e["text"]).contains("+43"))
    );
    assert!(
        !json!(recent)
            .to_string()
            .contains("sk-abcdefghijklmnopqrst")
    );
}

#[test]
fn only_three_recent_coding_sessions_are_included() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    let mut sessions = vec![];
    for i in 0..5 {
        let name = format!("session-{i}");
        let patch = format!("*** Begin Patch\n*** Add File: file-{i}.rs\n+code\n*** End Patch");
        let mut event = call("apply_patch", &patch, &name);
        event["timestamp"] = json!((chrono::Utc::now() - chrono::Duration::hours(i)).to_rfc3339());
        let path = transcript(&root, &name, vec![meta(&root, &name), event]);
        sessions.push(history::collect(&path).unwrap());
    }
    let recent = history::recent_code(&root, &sessions);
    assert_eq!(recent.len(), 3);
    assert_eq!(recent[0]["file"], "file-0.rs");
    assert_eq!(recent[2]["file"], "file-2.rs");
}

#[test]
fn codex_event_ids_do_not_replace_the_session_identity() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    let path = transcript(
        &root,
        "identity",
        vec![
            meta(&root, "session-id"),
            message("user", "Make a change"),
            json!({"type":"event_msg","payload":{"type":"item_completed","id":"different-item-id"}}),
            json!({"type":"turn_context","payload":{"id":"turn-id","cwd":root}}),
            call(
                "exec",
                "await tools.apply_patch(`*** Begin Patch\n*** Add File: lib.rs\n+code\n*** End Patch`)",
                "edit",
            ),
        ],
    );
    let session = history::collect(&path).unwrap();
    assert_eq!(session["id"], "session-id");
    assert_eq!(history::recent_code(&root, &[session]).len(), 1);
    let bad = transcript(
        &root,
        "conflicting",
        vec![
            meta(&root, "one"),
            message("user", "hello"),
            meta(&root, "two"),
        ],
    );
    assert!(history::collect(&bad).is_err());
}
