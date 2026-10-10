use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
use wy::{
    arr,
    history::{self, origins, provenance},
    repository, s,
    storage::Store,
};

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    repository::git(dir.path(), &["init", "-q"], true).unwrap();
    dir
}
fn meta(root: &Path, id: &str) -> Value {
    json!({"type":"session_meta","payload":{"id":id,"cwd":root.canonicalize().unwrap()}})
}
fn turn(id: &str) -> Value {
    json!({"type":"turn_context","payload":{"turn_id":id}})
}
fn message(id: &str, text: &str) -> Value {
    json!({"type":"response_item","payload":{"type":"message","role":"assistant","id":id,"content":[{"type":"output_text","text":text}]}})
}
fn capture(root: &Path, name: &str, rows: &[Value]) -> (PathBuf, String, Value) {
    let directory = root.join(".codex/sessions");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{name}.jsonl"));
    fs::write(
        &path,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let session = history::collect(&path).unwrap();
    let key = format!(
        "codex:{}:{}",
        s(&session["id"]),
        &wy::security::digest(&session.to_string())[..12]
    );
    Store::open(root)
        .unwrap()
        .put("session", &key, &session)
        .unwrap();
    (path, key, session)
}
fn evidence(root: &Path, session: &Value, index: usize) -> Value {
    let mut e = history::event_evidence(session, &session["events"][index]);
    origins::enrich(&root.canonicalize().unwrap(), &mut e).unwrap();
    e
}
fn explanation(e: &Value, status: &str) -> Value {
    let claim =
        json!({"text":"Inspect the captured context.","basis":"observed","evidence_ids":[e["id"]]});
    json!({"title":"A choice","answer":claim,"problem":claim,"before":claim,"after":claim,"steps":[],"tradeoffs":[],"checks":[],"unknowns":[],"judgments":[{"choice":"Use a cache","reason":"A recorded reason is needed.","status":status,"evidence_ids":[e["id"]],"quote":if status=="recorded"{s(&e["text"])}else{""},"quote_id":if status=="recorded"{s(&e["id"])}else{""}}]})
}

#[test]
fn summaries_and_original_messages_keep_distinct_provenance_and_turn_identity() {
    let dir = repo();
    let root = dir.path();
    let mut summary = message(
        "summary",
        "I used a cache in lib.rs because repeated reads are cheaper.",
    );
    summary["payload"]["phase"] = json!("summary");
    let (_, _, session) = capture(
        root,
        "coding",
        &[
            meta(root, "coding"),
            turn("turn-1"),
            message("m-1", "Repeated response."),
            turn("turn-2"),
            message("m-2", "Repeated response."),
            summary,
            json!({"type":"compacted","payload":{"message":"A summary of lib.rs changes."}}),
            message(
                "m-3",
                "This session is being continued from a previous conversation. lib.rs changed.",
            ),
        ],
    );
    assert_eq!(arr(&session["events"]).len(), 5);
    assert_eq!(session["events"][0]["provenance"]["turn_id"], "turn-1");
    assert_eq!(session["events"][1]["provenance"]["turn_id"], "turn-2");
    assert_eq!(
        session["events"][2]["provenance"]["source_type"],
        "compaction_summary"
    );
    assert_eq!(session["events"][3]["kind"], "summary");
    assert_eq!(session["events"][4]["provenance"]["source_type"], "unknown");
    let missing = evidence(root, &session, 3);
    assert_eq!(missing["origin_status"], "unavailable");
    assert!(
        origins::status(&missing)
            .unwrap()
            .contains("Original turn unavailable")
    );
}

#[test]
fn claude_summary_flags_are_secondary_and_parent_message_ids_are_preserved() {
    let dir = repo();
    let root = dir.path();
    let path = root.join("claude.jsonl");
    let rows = [
        json!({"type":"assistant","sessionId":"claude-session","cwd":root.canonicalize().unwrap(),"uuid":"m1","parentUuid":"u1","message":{"content":[{"type":"text","text":"Original answer"},{"type":"thinking","thinking":"PRIVATE"}]}}),
        json!({"type":"user","sessionId":"claude-session","cwd":root.canonicalize().unwrap(),"uuid":"summary","parentUuid":"boundary","isCompactSummary":true,"message":{"content":"I used a cache because lib.rs has repeated reads."}}),
    ];
    fs::write(
        &path,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let session = history::collect(&path).unwrap();
    assert_eq!(session["events"][0]["provenance"]["message_id"], "m1");
    assert_eq!(
        session["events"][0]["provenance"]["parent_message_id"],
        "u1"
    );
    assert_eq!(session["events"][1]["kind"], "rationale");
    assert!(!history::provenance::original(&session["events"][1]));
    assert_eq!(session["events"][2]["kind"], "summary");
}

#[test]
fn opaque_compaction_markers_preserve_the_gap_without_importing_private_content() {
    let dir = repo();
    let root = dir.path();
    let (_, _, session) = capture(
        root,
        "opaque",
        &[
            meta(root, "opaque"),
            json!({"type":"item.completed","item":{"type":"contextCompaction","id":"compact","encrypted_content":"PRIVATE"}}),
            json!({"type":"event_msg","payload":{"type":"agent_message","phase":"analysis","message":"PRIVATE"}}),
        ],
    );
    assert_eq!(arr(&session["events"]).len(), 1);
    assert_eq!(session["events"][0]["kind"], "summary");
    assert!(s(&session["events"][0]["text"]).contains("not retained"));
    assert!(!session.to_string().contains("PRIVATE"));
    assert_eq!(evidence(root, &session, 0)["origin_status"], "unavailable");
    let path = root.join("claude.jsonl");
    fs::write(&path,[
        json!({"type":"user","sessionId":"claude","cwd":root.canonicalize().unwrap(),"uuid":"u1","message":{"content":"A request"}}),
        json!({"type":"system","subtype":"compact_boundary","content":"PRIVATE"}),
    ].iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
    let claude = history::collect(&path).unwrap();
    assert_eq!(claude["events"][1]["kind"], "summary");
    assert!(!claude.to_string().contains("PRIVATE"));
}

#[test]
fn a_later_local_transcript_can_supply_an_original_missing_from_saved_captures() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    let (path, _, _) = capture(
        &root,
        "coding",
        &[
            meta(&root, "coding"),
            turn("t1"),
            message("m1", "First original"),
        ],
    );
    // The on-disk transcript has advanced since the saved review.
    fs::write(
        &path,
        [
            meta(&root, "coding"),
            turn("t1"),
            message("m1", "First original"),
            turn("t2"),
            message("m2", "Later original"),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n"),
    )
    .unwrap();
    let (_, _, summary) = capture(
        &root,
        "child",
        &[
            json!({"type":"session_meta","payload":{"id":"child","cwd":root,"resumed_from_id":"coding","lastTurnId":"t2"}}),
            json!({"type":"compacted","payload":{"message":"Resume summary","source_refs":[{"session_id":"coding","message_id":"m2"}]}}),
        ],
    );
    let e = evidence(&root, &summary, 0);
    assert_eq!(e["origin_status"], "available");
    assert_eq!(
        origins::open(&root, &e["originals"][0]).unwrap()["text"],
        "Later original"
    );
    fs::remove_file(path).unwrap();
    assert_eq!(evidence(&root, &summary, 0)["origin_status"], "available");
}

#[test]
fn explicit_original_references_survive_resume_and_deleted_transcripts() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    let (path, _, original) = capture(
        &root,
        "coding",
        &[
            meta(&root, "coding"),
            turn("first"),
            message(
                "original",
                "I used a cache in lib.rs because repeated reads are cheaper.",
            ),
        ],
    );
    let (_, _, resumed) = capture(
        &root,
        "coding",
        &[
            meta(&root, "coding"),
            turn("later"),
            json!({"type":"compacted","payload":{"message":"The cache in lib.rs was introduced earlier.","source_refs":[{"turn_id":"first","message_id":"original"}]}}),
        ],
    );
    fs::write(root.join("lib.rs"), "pub fn value() {}\n").unwrap();
    let review = wy::service::review(
        &root,
        &wy::service::ReviewOptions {
            source: "codex".into(),
            ..Default::default()
        },
    )
    .unwrap();
    fs::remove_file(path).unwrap();
    let packet = wy::reasoning::packet(&review, "Why a cache?", Some("lib.rs"), &[], None).unwrap();
    assert!(
        arr(&packet["evidence"])
            .iter()
            .any(|e| e["provenance"]["source_type"] == "compaction_summary")
    );
    assert!(
        arr(&packet["evidence"])
            .iter()
            .any(provenance::recorded_evidence)
    );
    let summary = evidence(&root, &resumed, 0);
    assert_eq!(summary["origin_status"], "available");
    let opened = origins::open(&root, &summary["originals"][0]).unwrap();
    assert_eq!(opened["text"], original["events"][0]["text"]);
    assert!(provenance::recorded_evidence(&opened));
    assert!(!provenance::recorded_evidence(&summary));
    let mut forged = summary["originals"][0].clone();
    forged["content_hash"] = json!("different");
    assert!(origins::open(&root, &forged).is_err());
}

#[test]
fn retained_public_originals_are_clickable_context_without_promoting_summary_claims() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    let (_, _, session) = capture(
        &root,
        "retained",
        &[
            meta(&root, "retained"),
            json!({"type":"compacted","payload":{"message":"lib.rs was updated.","retained_context":{"assistant_messages":[
        {"message_id":"a1","turn_id":"t1","complete":true,"phase":"final_answer","text":"The public original answer."},
        {"message_id":"private","turn_id":"t1","complete":true,"phase":"analysis","text":"PRIVATE"},
        {"message_id":"partial","turn_id":"t1","complete":false,"text":"INCOMPLETE"}]}}}),
        ],
    );
    assert_eq!(arr(&session["events"]).len(), 2);
    assert!(!session.to_string().contains("PRIVATE"));
    assert!(!session.to_string().contains("INCOMPLETE"));
    let summary = evidence(&root, &session, 1);
    assert_eq!(summary["origin_status"], "available");
    assert_eq!(summary["originals"][0]["relation"], "retained_context");
    assert_eq!(
        origins::open(&root, &summary["originals"][0]).unwrap()["text"],
        "The public original answer."
    );
}

#[test]
fn fork_boundary_excludes_later_parent_turns_and_unknown_ancestry() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    capture(
        &root,
        "parent",
        &[
            meta(&root, "parent"),
            turn("before"),
            message("m1", "Original before fork"),
            turn("after"),
            message("m2", "Parent after fork"),
            // A later compaction replays the old turn; this must not move the cutoff.
            json!({"type":"compacted","payload":{"message":"Later summary","retained_context":{"assistant_messages":[{"turn_id":"before","message_id":"m1","complete":true,"phase":"final","text":"Original before fork"}]}}}),
        ],
    );
    let mut child_meta = meta(&root, "child");
    child_meta["payload"]["forked_from_id"] = json!("parent");
    child_meta["payload"]["forked_from_turn_id"] = json!("before");
    let refs = json!([{"session_id":"parent","turn_id":"before","message_id":"m1"},{"session_id":"parent","turn_id":"after","message_id":"m2"}]);
    let (_, _, child) = capture(
        &root,
        "child",
        &[
            child_meta.clone(),
            json!({"type":"compacted","payload":{"message":"Summary of parent","source_refs":refs}}),
        ],
    );
    let e = evidence(&root, &child, 0);
    assert_eq!(e["origin_status"], "partial");
    assert_eq!(arr(&e["originals"]).len(), 1);
    assert_eq!(e["originals"][0]["message_id"], "m1");
    child_meta["payload"]
        .as_object_mut()
        .unwrap()
        .remove("forked_from_turn_id");
    let (_, _, unknown) = capture(
        &root,
        "child",
        &[
            child_meta,
            json!({"type":"compacted","payload":{"message":"Summary","source_refs":refs}}),
        ],
    );
    let e = evidence(&root, &unknown, 0);
    assert_eq!(e["origin_status"], "unavailable");
    assert!(s(&e["origin_reason"]).contains("fork boundary"));
}

#[test]
fn conflicting_saved_originals_and_foreign_repositories_cannot_resolve() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    capture(
        &root,
        "coding",
        &[
            meta(&root, "coding"),
            turn("first"),
            message("m1", "Version one"),
        ],
    );
    capture(
        &root,
        "coding",
        &[
            meta(&root, "coding"),
            turn("first"),
            message("m1", "Version two"),
        ],
    );
    let (_, _, summary) = capture(
        &root,
        "coding",
        &[
            meta(&root, "coding"),
            json!({"type":"compacted","payload":{"message":"Summary","source_refs":[{"turn_id":"first","message_id":"m1"}]}}),
        ],
    );
    let e = evidence(&root, &summary, 0);
    assert_eq!(e["origin_status"], "unavailable");
    assert!(s(&e["origin_reason"]).contains("ambiguous"));
    let other = repo();
    let (_, key, mut foreign) = capture(
        &root,
        "foreign",
        &[
            meta(&root, "foreign"),
            turn("t1"),
            message("foreign-m", "Foreign"),
        ],
    );
    foreign["cwd"] = json!(other.path().canonicalize().unwrap());
    Store::open(&root)
        .unwrap()
        .put("session", &key, &foreign)
        .unwrap();
    let mut ref_ = json!({"session_key":key,"agent":"codex","session_id":"foreign","event_id":foreign["events"][0]["id"],"content_hash":provenance::content_hash(&foreign["events"][0])});
    assert!(origins::open(&root, &ref_).is_err());
    ref_["session_id"] = json!("different");
    assert!(origins::open(&root, &ref_).is_err());
}

#[test]
fn recorded_validation_requires_original_and_secondary_only_rationale_stays_unknown() {
    let dir = repo();
    let root = dir.path();
    let (_, _, session) = capture(
        root,
        "coding",
        &[
            meta(root, "coding"),
            message(
                "m1",
                "I used a cache in lib.rs because repeated reads are cheaper.",
            ),
        ],
    );
    let original = evidence(root, &session, 0);
    let mut packet = json!({"evidence":[original],"focus_target":null});
    let recorded = explanation(&original, "recorded");
    wy::reasoning::validate_explanation(&recorded, &packet).unwrap();
    for kind in ["compaction_summary", "unknown"] {
        packet["evidence"][0]["provenance"]["source_type"] = json!(kind);
        assert!(wy::reasoning::validate_explanation(&recorded, &packet).is_err());
        assert!(
            wy::reasoning::validate_explanation(&explanation(&original, "inferred"), &packet)
                .is_err()
        );
        wy::reasoning::validate_explanation(&explanation(&original, "unknown"), &packet).unwrap();
    }
    packet["evidence"][0]
        .as_object_mut()
        .unwrap()
        .remove("provenance");
    assert!(wy::reasoning::validate_explanation(&recorded, &packet).is_err());
    let mut artifact = json!({"packet":packet,"explanation":recorded});
    provenance::sanitize_artifact(&mut artifact);
    assert_eq!(artifact["explanation"]["judgments"][0]["status"], "unknown");
    assert!(s(&artifact["provenance_warning"]).contains("original turn unavailable"));
    // An unrelated original elsewhere in the packet cannot launder a summary citation.
    let mut summary = original.clone();
    summary["id"] = json!("summary");
    summary["provenance"]["source_type"] = json!("compaction_summary");
    let packet = json!({"evidence":[original,summary],"focus_target":null});
    assert!(
        wy::reasoning::validate_explanation(&explanation(&summary, "inferred"), &packet).is_err()
    );
    let mut saved = json!({"packet":packet,"explanation":explanation(&summary,"inferred")});
    provenance::sanitize_artifact(&mut saved);
    assert_eq!(saved["explanation"]["judgments"][0]["status"], "unknown");
}

#[test]
fn summary_provenance_and_missing_original_reach_explanation_packets() {
    let dir = repo();
    let root = dir.path().canonicalize().unwrap();
    fs::write(root.join("lib.rs"), "fn value() { Redis(); }\n").unwrap();
    capture(
        &root,
        "coding",
        &[
            meta(&root, "coding"),
            json!({"type":"compacted","payload":{"message":"I used Redis in lib.rs because repeated reads are cheaper."}}),
        ],
    );
    let review = wy::service::review(
        &root,
        &wy::service::ReviewOptions {
            source: "codex".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let packet = wy::reasoning::packet(&review, "Why a cache?", Some("lib.rs"), &[], None).unwrap();
    let summary = arr(&packet["evidence"])
        .iter()
        .find(|e| e["provenance"]["source_type"] == "compaction_summary")
        .unwrap();
    assert_eq!(summary["origin_status"], "unavailable");
    assert!(s(&summary["origin_reason"]).contains("reference"));
}
