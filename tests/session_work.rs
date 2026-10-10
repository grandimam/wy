use serde_json::{Value, json};
use std::{fs, path::Path};
use wy::{arr, decisions, history, repository, s, service, session_work, storage::Store};

fn transcript(root: &Path, id: &str, date: &str, code: &str) -> Value {
    let path = root.join(format!(".codex/sessions/{id}.jsonl"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let record = format!(
        "WY_DECISION {}",
        json!({"file":"worker.rs","decision":format!("Use {id}"),"reason":format!("Reason for {id}"),"alternatives":["Other implementation"],"tradeoffs":["Extra work"]})
    );
    let rows = [
        json!({"type":"session_meta","payload":{"id":id,"cwd":root,"timestamp":date}}),
        json!({"type":"response_item","timestamp":date,"payload":{"type":"message","role":"user","content":[{"type":"input_text","text":format!("Request for {id}")}]}}),
        json!({"type":"response_item","timestamp":date,"payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":record}]}}),
        json!({"type":"response_item","timestamp":date,"payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"c1","input":format!("*** Begin Patch\n*** Add File: worker.rs\n+{code}\n*** End Patch")}}),
        json!({"type":"response_item","timestamp":date,"payload":{"type":"function_call_output","call_id":"c1","output":"Success. Updated the following files:"}}),
    ];
    fs::write(
        &path,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    history::collect(&path).unwrap()
}
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    repository::git(dir.path(), &["init", "-q"], true).unwrap();
    dir
}
fn review(root: &Path) -> Value {
    service::review(
        root,
        &service::ReviewOptions {
            source: "codex".into(),
        },
    )
    .unwrap()
}

#[test]
fn separate_sessions_touching_same_file_never_share_decision_evidence() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    transcript(&root, "old", "2024-01-01T01:00:00Z", "OLD_CAPTURE");
    transcript(&root, "new", "2024-01-02T01:00:00Z", "NEW_CAPTURE");
    fs::write(root.join("worker.rs"), "TODAY_NOT_SESSION_CODE").unwrap();
    let r = review(&root);
    let sessions = history::saved(&r).unwrap();
    assert_eq!(session_work::latest(&sessions).unwrap()["id"], "new");
    for session in &sessions {
        let work = session_work::build(&r, session);
        let packet = session_work::packet(&work, session);
        let expected = if session["id"] == "old" {
            "OLD_CAPTURE"
        } else {
            "NEW_CAPTURE"
        };
        let other = if session["id"] == "old" {
            "NEW_CAPTURE"
        } else {
            "OLD_CAPTURE"
        };
        assert!(packet.to_string().contains(expected));
        assert!(!packet.to_string().contains(other));
        assert!(!packet.to_string().contains("TODAY_NOT_SESSION_CODE"));
        let brief = decisions::recorded(&work, std::slice::from_ref(session));
        assert_eq!(arr(&brief["decisions"]).len(), 1);
        assert_eq!(
            brief["decisions"][0]["choice"],
            format!("Use {}", s(&session["id"]))
        );
        assert!(
            arr(&brief["packet"]["evidence"])
                .iter()
                .all(|e| e["kind"] != "diff")
        );
    }
}
#[test]
fn commits_current_edits_and_deleted_files_do_not_change_historical_scope() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    transcript(&root, "coding", "2024-01-01T00:00:00Z", "CAPTURED");
    fs::write(root.join("worker.rs"), "CAPTURED\n").unwrap();
    for args in [
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["add", "worker.rs"],
        vec!["commit", "-qm", "committed"],
    ] {
        repository::git(&root, &args, true).unwrap();
    }
    let r = review(&root);
    assert!(arr(&r["changes"]).is_empty());
    let sessions = history::saved(&r).unwrap();
    let work = session_work::build(&r, &sessions[0]);
    assert_eq!(arr(&work["edits"]).len(), 1);
    let key = decisions::scope_key(&work);
    let brief = decisions::recorded(&work, &sessions);
    Store::open(&root)
        .unwrap()
        .put("decision_brief", "saved-session", &brief)
        .unwrap();
    fs::write(root.join("worker.rs"), "DIFFERENT_TODAY").unwrap();
    let new = review(&root);
    let rebuilt = session_work::build(&new, &sessions[0]);
    assert_eq!(decisions::scope_key(&rebuilt), key);
    assert!(decisions::saved(&root, &rebuilt).unwrap().is_some());
    fs::remove_file(root.join("worker.rs")).unwrap();
    fs::remove_file(root.join(".codex/sessions/coding.jsonl")).unwrap();
    let loaded = session_work::load(&root, &work).unwrap();
    assert_eq!(loaded["id"], "coding");
    let ref_edit = history::edit_ref(&work["edits"][0]);
    let (edit, _) = history::saved_edit(&root, &ref_edit).unwrap();
    assert_eq!(edit["text"], "CAPTURED");
    assert_eq!(
        session_work::compare(&root, &work, &edit)["status"],
        "Current code unavailable"
    );
}
#[test]
fn comparison_is_conservative_for_partial_unconfirmed_or_nonfinal_writes() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    let session = transcript(&root, "coding", "2024-01-01T00:00:00Z", "CAPTURED");
    let work = session_work::build(&json!({"root":root}), &session);
    let edit = work["edits"][0].clone();
    fs::write(root.join("worker.rs"), "CAPTURED\n").unwrap();
    assert_eq!(
        session_work::compare(&root, &work, &edit)["status"],
        "Matches captured text"
    );
    fs::write(root.join("worker.rs"), "DIFFERENT").unwrap();
    assert_eq!(
        session_work::compare(&root, &work, &edit)["status"],
        "Changed since captured edit"
    );
    for (field, value) in [
        ("truncated", json!(true)),
        ("state", json!("recorded")),
        ("format", json!("patch")),
    ] {
        let mut e = edit.clone();
        e[field] = value;
        assert!(
            s(&session_work::compare(&root, &work, &e)["status"])
                .contains("comparison unavailable")
        );
    }
    let mut later = edit.clone();
    later["id"] = json!("later");
    later["format"] = json!("patch");
    let mut w = work.clone();
    w["edits"].as_array_mut().unwrap().push(later);
    assert!(
        s(&session_work::compare(&root, &w, &edit)["status"]).contains("comparison unavailable")
    );
}
#[test]
fn preserves_chronological_edits_and_excludes_failed_or_unsafe_changes() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    let mut session = transcript(&root, "coding", "2024-01-01T00:00:00Z", "FIRST");
    let edit_event = arr(&session["events"])
        .iter()
        .find(|e| !arr(&e["code_edits"]).is_empty())
        .unwrap()
        .clone();
    let mut later = edit_event.clone();
    later["id"] = json!("later");
    later["call_id"] = json!("c2");
    later["code_edits"][0]["text"] = json!("SECOND");
    let mut failed = later.clone();
    failed["id"] = json!("failed");
    failed["failed"] = json!(true);
    failed["code_edits"][0]["text"] = json!("NEVER_APPLIED");
    let mut unsafe_edit = later.clone();
    unsafe_edit["id"] = json!("unsafe");
    unsafe_edit["code_edits"][0]["file"] = json!("../outside.rs");
    session["events"]
        .as_array_mut()
        .unwrap()
        .extend([later, failed, unsafe_edit]);
    let work = session_work::build(&json!({"root":root}), &session);
    assert_eq!(arr(&work["edits"]).len(), 2);
    assert_eq!(work["edits"][0]["text"], "FIRST");
    assert_eq!(work["edits"][1]["text"], "SECOND");
    assert!(
        !session_work::packet(&work, &session)
            .to_string()
            .contains("NEVER_APPLIED")
    );
}
#[test]
fn session_validation_rejects_other_sessions_and_current_diff_substitutions() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    let session = transcript(&root, "coding", "2024-01-01T00:00:00Z", "CAPTURED");
    let work = session_work::build(&json!({"root":root}), &session);
    let packet = session_work::packet(&work, &session);
    let source = &packet["evidence"][0];
    let response = json!({"decisions":[{"choice":"Captured choice","reason":"Plausible benefit","significance":"Durability","status":"inferred","files":["worker.rs"],"alternatives":[],"tradeoffs":[],"evidence_ids":[source["id"]],"quote":"","quote_id":""}],"unknowns":[]});
    decisions::validate(&response, &packet, &work).unwrap();
    let mut p = packet.clone();
    p["evidence"][0]["session_key"] = json!("other");
    assert!(decisions::validate(&response, &p, &work).is_err());
    let mut p = packet.clone();
    p["evidence"][0]["kind"] = json!("diff");
    assert!(decisions::validate(&response, &p, &work).is_err());
    let mut p = packet.clone();
    p["evidence"][0]["agent"] = json!("claude");
    assert!(decisions::validate(&response, &p, &work).is_err());
}
#[test]
fn request_turns_keep_planning_notes_confirmations_and_unknown_boundaries_separate() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    let mut session = transcript(&root, "coding", "2024-01-01T00:00:00Z", "CAPTURED");
    let events = arr(&session["events"]);
    let user = events.iter().find(|e| e["kind"] == "user").unwrap().clone();
    let mut response = events
        .iter()
        .find(|e| e["kind"] == "assistant")
        .unwrap()
        .clone();
    response["text"] = json!("A proposed approach");
    let edit = events
        .iter()
        .find(|e| !arr(&e["code_edits"]).is_empty())
        .unwrap()
        .clone();
    let output = events
        .iter()
        .find(|e| e["kind"] == "tool_output")
        .unwrap()
        .clone();
    let note = json!({"id":"notes-plan","kind":"rationale","text":"PLAN_ONLY","provenance":{"source_type":"unknown"}});
    let mut confirmation = user.clone();
    confirmation["id"] = json!("confirm");
    confirmation["text"] = json!("yes do it");
    let mut next_note = note.clone();
    next_note["id"] = json!("notes-implementation");
    next_note["text"] = json!("IMPLEMENTATION_ONLY");
    let mut unknown = user.clone();
    unknown["id"] = json!("unknown-request");
    unknown["provenance"]["source_type"] = json!("unknown");
    let mut later = edit.clone();
    later["id"] = json!("later-edit");
    later["call_id"] = json!("later-call");
    later["source_line"] = json!(900);
    session["events"] = json!([
        user,
        response,
        note,
        confirmation,
        next_note,
        edit,
        output,
        unknown,
        later
    ]);
    let work = session_work::build(&json!({"root":root}), &session);
    let turns = arr(&work["turns"]);
    assert_eq!(turns.len(), 3);
    assert!(arr(&turns[0]["edit_indices"]).is_empty());
    assert!(turns[0]["messages"].to_string().contains("PLAN_ONLY"));
    assert!(
        !turns[0]["messages"]
            .to_string()
            .contains("IMPLEMENTATION_ONLY")
    );
    assert_eq!(turns[1]["request"]["text"], "yes do it");
    assert_eq!(turns[1]["prior_request"]["text"], "Request for coding");
    assert_eq!(turns[1]["edit_indices"], json!([0]));
    assert!(
        turns[1]["messages"]
            .to_string()
            .contains("IMPLEMENTATION_ONLY")
    );
    assert!(!turns[1]["messages"].to_string().contains("PLAN_ONLY"));
    assert!(turns[2]["prior_request"].is_null());
    assert_eq!(turns[2]["edit_indices"], json!([1]));
    assert!(work["steps"][1]["request"].is_null());
}

#[test]
fn recency_uses_known_source_dates_and_new_capture_changes_scope() {
    let a = json!({"id":"a","events":[{"timestamp":"2024-01-01T23:00:00-05:00"}]});
    let b = json!({"id":"b","events":[{"timestamp":"2024-01-02T01:00:00Z"}]});
    let unknown = json!({"id":"unknown","events":[{"text":"unknown"}]});
    assert_eq!(
        session_work::latest(&[a.clone(), b, unknown]).unwrap()["id"],
        "a"
    );
    let w1 = session_work::build(&json!({"root":"/repo"}), &a);
    let mut a2 = a.clone();
    a2["events"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"assistant","text":"new context"}));
    let w2 = session_work::build(&json!({"root":"/repo"}), &a2);
    assert_ne!(decisions::scope_key(&w1), decisions::scope_key(&w2));
}
