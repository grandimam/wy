use serde_json::{Value, json};
use wy::{
    arr, decisions, history,
    session_work::{self, WorkSelection},
};

fn fixture() -> (tempfile::TempDir, Value, Value) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    wy::repository::git(&root, &["init", "-q"], true).unwrap();
    let path = root.join("session.jsonl");
    let mut rows = vec![
        json!({"type":"session_meta","payload":{"id":"scope-test","cwd":root,"timestamp":"2026-01-01T00:00:00Z"}}),
    ];
    for (i, name) in ["PERSISTENCE", "NAVIGATION"].iter().enumerate() {
        let record = format!(
            "WY_DECISION {}",
            json!({"file":"shared.rs","decision":name,"reason":format!("REASON_{name}")})
        );
        rows.extend([
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":format!("REQUEST_{name}")}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":record}]}}),
            json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":format!("edit-{i}"),"input":format!("*** Begin Patch\n*** Add File: shared.rs\n+CODE_{name}\n*** End Patch")}}),
        ]);
    }
    std::fs::write(
        &path,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let session = history::collect(&path).unwrap();
    let review = json!({"root":root,"id":"test-review"});
    (dir, review, session)
}

#[test]
fn requests_sharing_a_file_keep_edits_and_rationale_separate() {
    let (_dir, review, session) = fixture();
    for (ordinal, selected, excluded) in [
        (0, "PERSISTENCE", "NAVIGATION"),
        (1, "NAVIGATION", "PERSISTENCE"),
    ] {
        let work = session_work::scoped(
            &review,
            &session,
            WorkSelection::Requests {
                ordinals: vec![ordinal],
            },
        )
        .unwrap();
        assert_eq!(arr(&work["edits"]).len(), 1);
        assert_eq!(work["turns"][0]["edit_indices"], json!([0]));
        assert!(work["turns"][0]["prior_request"].is_null());
        assert_eq!(
            work["session"]["storage_key"],
            session_work::snapshot_key(&session)
        );
        let packet = session_work::packet(&work, &session).to_string();
        assert!(packet.contains(&format!("CODE_{selected}")));
        assert!(packet.contains(&format!("REASON_{selected}")));
        assert!(!packet.contains(&format!("CODE_{excluded}")));
        assert!(!packet.contains(&format!("REASON_{excluded}")));
        assert!(!packet.contains(&format!("REQUEST_{excluded}")));
        let brief = decisions::recorded(&work, std::slice::from_ref(&session));
        assert_eq!(arr(&brief["decisions"]).len(), 1);
        assert_eq!(brief["decisions"][0]["choice"], selected);
        assert_eq!(brief["selection"], work["selection"]);
    }
}

#[test]
fn scope_keys_are_normalized_and_distinct_from_whole_session() {
    let (_dir, review, session) = fixture();
    let scope = |ordinals| {
        session_work::scoped(&review, &session, WorkSelection::Requests { ordinals }).unwrap()
    };
    let one = scope(vec![0]);
    let two = scope(vec![1]);
    let both = scope(vec![0, 1]);
    let reordered = scope(vec![1, 0, 1]);
    assert_eq!(
        decisions::scope_key(&both),
        decisions::scope_key(&reordered)
    );
    assert_ne!(decisions::scope_key(&one), decisions::scope_key(&two));
    assert_ne!(
        decisions::scope_key(&both),
        decisions::scope_key(&session_work::build(&review, &session))
    );
    for ordinals in [vec![], vec![2], vec![usize::MAX]] {
        assert!(
            session_work::scoped(&review, &session, WorkSelection::Requests { ordinals }).is_err()
        );
    }
    let mut changed = session.clone();
    changed["events"][0]["text"] = json!("Changed captured request");
    let new = session_work::scoped(
        &review,
        &changed,
        WorkSelection::Requests { ordinals: vec![0] },
    )
    .unwrap();
    assert_ne!(decisions::scope_key(&one), decisions::scope_key(&new));
}

#[test]
fn decision_subsets_cannot_poison_the_full_session_reader_index() {
    let (_dir, review, session) = fixture();
    let root = std::path::Path::new(wy::s(&review["root"]));
    let selected = session_work::scoped(
        &review,
        &session,
        WorkSelection::Requests { ordinals: vec![1] },
    )
    .unwrap();
    assert!(wy::session_reader::index(root, &selected).is_err());
    let whole = session_work::build(&review, &session);
    wy::session_reader::index(root, &whole).unwrap();
    let page = wy::session_reader::page(root, wy::s(&whole["session"]["storage_key"]), 0).unwrap();
    assert_eq!(wy::session_reader::count(&page), 2);
    assert!(page.to_string().contains("REQUEST_PERSISTENCE"));
}

#[test]
fn empty_selection_cannot_be_encoded_as_entire_session() {
    assert!(
        session_work::selection(&json!({"selection":{"kind":"requests","ordinals":"bad"}}))
            .is_err()
    );
    assert_eq!(
        session_work::selection(&json!({})).unwrap(),
        WorkSelection::EntireSession
    );
}
