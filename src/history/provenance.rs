//! Evidence provenance is assigned by the importer, never by an explaining model.
use crate::{
    arr, s,
    security::{digest, redact, short},
};
use serde_json::{Value, json};

pub fn unknown() -> Value {
    json!({"source_type":"unknown","basis":"legacy_or_unclassified","original_refs":[]})
}
pub fn metadata(event: &Value) -> Value {
    event
        .get("provenance")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(unknown)
}
pub fn original(event: &Value) -> bool {
    event["provenance"]["source_type"] == "original_turn"
}
pub fn recorded_evidence(e: &Value) -> bool {
    e["kind"] == "session" && e["role"] == "assistant" && original(e)
}
pub fn suspected_summary(text: &str) -> bool {
    let text = text.trim_start().to_lowercase();
    [
        "this session is being continued from a previous conversation",
        "the previous conversation was compacted",
        "<summary>",
        "you are continuing a conversation that was compacted",
        "a previous model",
        "another language model began",
        "another language model started",
    ]
    .iter()
    .any(|p| text.starts_with(p))
}
pub fn classify(row: &Value, kind: &str, text: &str, turn: &str) -> Value {
    let p = &row["payload"];
    let summary = row["isCompactSummary"] == true
        || p["isCompactSummary"] == true
        || p["phase"] == "summary"
        || p["channel"] == "summary"
        || row["item"]["phase"] == "summary"
        || row["type"] == "compacted"
        || kind == "summary";
    let source = if summary {
        "compaction_summary"
    } else if suspected_summary(text) {
        "unknown"
    } else if ["user", "assistant"].contains(&kind) {
        "original_turn"
    } else {
        "tool_record"
    };
    let message = row
        .get("uuid")
        .or_else(|| p.get("id"))
        .or_else(|| row["item"].get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    let turn_id = p
        .get("turn_id")
        .or_else(|| p["internal_chat_message_metadata_passthrough"].get("turn_id"))
        .or_else(|| row.get("turn_id"))
        .and_then(Value::as_str)
        .unwrap_or(turn);
    let mut refs = vec![];
    let references = arr(row
        .get("source_refs")
        .or_else(|| p.get("source_refs"))
        .unwrap_or(&Value::Null));
    for r in references.iter().take(32) {
        if r["turn_id"].is_string() || r["message_id"].is_string() {
            refs.push(json!({"session_id":r["session_id"],"turn_id":r["turn_id"],"message_id":r["message_id"],"relation":"source"}));
        }
    }
    json!({"source_type":source,"basis":if summary {"provider_compaction_marker"} else if source=="unknown" {"suspected_summary"} else {"native_event"},
        "turn_id":if turn_id.is_empty(){Value::Null}else{json!(turn_id)},"message_id":message,
        "parent_message_id":row["parentUuid"],"original_refs":refs,"references_truncated":references.len()>32})
}

pub fn observe(row: &Value, session: &mut Value, turn: &mut String) {
    let p = &row["payload"];
    if row["type"] == "turn_context" || row["type"] == "event_msg" && p["type"] == "task_started" {
        if let Some(id) = p["turn_id"].as_str() {
            *turn = id.to_owned();
        }
    }
    if row["type"] == "session_meta" {
        let parent = p.get("forked_from_id").or_else(|| p.get("forkedFromId"));
        let resumed = p.get("resumed_from_id");
        if let Some(parent) = parent.or(resumed).filter(|v| v.is_string()) {
            let boundary = p
                .get("forked_from_turn_id")
                .or_else(|| p.get("lastTurnId"))
                .cloned()
                .unwrap_or(Value::Null);
            session["lineage"] = json!({"relation":if resumed.is_some(){"resume"}else{"fork"},"parent_session_id":parent,"through_turn_id":boundary,
                "status":if boundary.is_string(){"boundary_recorded"}else{"boundary_unavailable"}});
        }
    }
}

/// Preserve only explicitly retained, complete public messages. Replacement history
/// itself may be generated context and is not imported as original speech.
pub fn compaction(row: &Value, session: &Value, line: usize, turn: &str) -> Vec<Value> {
    let boundary = row["type"] == "system" && row["subtype"] == "compact_boundary";
    let opaque = row["type"] == "item.completed"
        && ["context_compaction", "contextCompaction"].contains(&s(&row["item"]["type"]));
    if row["type"] != "compacted" && !boundary && !opaque {
        return vec![];
    }
    let p = &row["payload"];
    let mut events = vec![];
    let mut refs = vec![];
    for (key, role) in [
        ("user_messages", "user"),
        ("assistant_messages", "assistant"),
    ] {
        for (index, item) in arr(&p["retained_context"][key]).iter().take(32).enumerate() {
            if item["complete"] != true
                || ["analysis", "summary"].contains(&s(&item["phase"]))
                || ["analysis", "summary"].contains(&s(&item["channel"]))
                || item["isCompactSummary"] == true
                || !item["message_id"].is_string()
                || !item["turn_id"].is_string()
            {
                continue;
            }
            let text = s(&item["text"]);
            if text.is_empty() || suspected_summary(text) {
                continue;
            }
            let reference = json!({"session_id":session["id"],"turn_id":item["turn_id"],"message_id":item["message_id"],"relation":"retained_context"});
            refs.push(reference);
            events.push(json!({"id":format!("event-{line}-retained-{role}-{index}"),"kind":role,"text":short(&redact(text),16000),"source_line":line,
                "timestamp":row["timestamp"],"files":[],"code_edits":[],"provenance":{"source_type":"original_turn","basis":"complete_retained_message",
                "turn_id":item["turn_id"],"message_id":item["message_id"],"original_refs":[]},"truncated":text.chars().count()>16000}));
        }
    }
    let replacement_summary = arr(&p["replacement_history"])
        .iter()
        .filter(|item| {
            item["type"] == "message"
                && ["user", "assistant"].contains(&s(&item["role"]))
                && item["phase"] != "analysis"
                && item["channel"] != "analysis"
        })
        .map(|item| super::visible(&item["content"]))
        .filter(|text| suspected_summary(text))
        .collect::<Vec<_>>()
        .join("\n\n");
    let text = if s(&p["message"]).is_empty() {
        replacement_summary.as_str()
    } else {
        s(&p["message"])
    };
    // Even a marker with opaque/missing summary text must make the gap visible.
    let mut provenance = classify(row, "summary", text, turn);
    provenance["references_truncated"] = json!(
        provenance["references_truncated"] == true
            || arr(&p["retained_context"]["user_messages"]).len() > 32
            || arr(&p["retained_context"]["assistant_messages"]).len() > 32
    );
    provenance["original_refs"]
        .as_array_mut()
        .unwrap()
        .extend(refs);
    events.push(json!({"id":format!("event-{line}"),"kind":"summary","text":if text.is_empty(){"Compaction recorded; readable summary text was not retained.".into()}else{short(&redact(text),16000)},
        "source_line":line,"timestamp":row["timestamp"],"files":[],"code_edits":[],"provenance":provenance,"truncated":text.chars().count()>16000}));
    events
}

pub fn label(event: &Value) -> String {
    match s(&event["provenance"]["source_type"]) {
        "original_turn" => "Original turn · captured message".into(),
        "tool_record" => "Captured tool record".into(),
        "compaction_summary" => "Secondary evidence · Compacted summary".into(),
        _ => "Unknown provenance · Original turn unavailable — re-import to classify".into(),
    }
}
pub fn content_hash(event: &Value) -> String {
    digest(s(&event["text"]))
}

pub fn secondary_only(evidence: &[Value]) -> bool {
    evidence.iter().any(|e| {
        e["kind"] == "session" && !original(e) && e["provenance"]["source_type"] != "tool_record"
    }) && !evidence.iter().any(recorded_evidence)
}
/// Old saved answers must not keep an unverified Recorded badge after upgrading.
pub fn sanitize_artifact(artifact: &mut Value) {
    let evidence = arr(&artifact["packet"]["evidence"]).to_vec();
    let secondary = secondary_only(&evidence);
    let mut revoked = false;
    if let Some(judgments) = artifact["explanation"]["judgments"].as_array_mut() {
        for j in judgments {
            let cited: Vec<_> = evidence
                .iter()
                .filter(|e| arr(&j["evidence_ids"]).contains(&e["id"]))
                .cloned()
                .collect();
            if (j["status"] == "recorded"
                && !evidence
                    .iter()
                    .any(|e| e["id"] == j["quote_id"] && recorded_evidence(e)))
                || ((secondary || secondary_only(&cited)) && j["status"] == "inferred")
            {
                revoked = true;
                j["status"] = json!("unknown");
                j["quote"] = json!("");
                j["quote_id"] = json!("");
                j["reason"] = json!(
                    "Original turn unavailable or unverified in this saved answer. Original rationale unknown; re-import the transcript and explain again."
                );
            }
        }
    }
    if revoked {
        artifact["provenance_warning"] = json!(
            "Earlier assessment · original turn unavailable or unverified. Original rationale unknown; saved prose is not proof of original intent."
        );
    }
}
