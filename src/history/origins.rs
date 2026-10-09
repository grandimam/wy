//! Resolve explicit references to immutable, repository-scoped original messages.
use super::provenance;
use crate::{arr, s, security::digest, storage::Store};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

fn matches_reference(event: &Value, reference: &Value) -> bool {
    provenance::original(event)
        && ["user", "assistant"].contains(&s(&event["kind"]))
        && (!reference["message_id"].is_string()
            || reference["message_id"] == event["provenance"]["message_id"])
        && (!reference["turn_id"].is_string()
            || reference["turn_id"] == event["provenance"]["turn_id"])
}

fn captures(
    root: &Path,
    store: &Store,
    agent: &str,
    id: &str,
    refs: &[Value],
    boundary: &Value,
) -> Result<Vec<(String, Value)>> {
    let mut saved = store.session_snapshots(agent, id)?;
    // A resumed/branched session may refer to a transcript older than the review's
    // discovery budget. Only look for its explicit ID inside this repository.
    if refs.iter().any(|reference| {
        !saved.iter().any(|(_, session)| {
            let events = arr(&session["events"]);
            events
                .iter()
                .any(|event| matches_reference(event, reference))
                && (!boundary.is_string()
                    || events
                        .iter()
                        .any(|e| e["provenance"]["turn_id"] == *boundary))
        })
    }) {
        if let Some(entry) = super::discover(root, agent, None, None)?
            .into_iter()
            .find(|e| e["id"] == id)
        {
            let session = super::collect(Path::new(s(&entry["path"])))?;
            ensure!(
                super::belongs(s(&session["cwd"]), root)
                    && session["id"] == id
                    && session["agent"] == agent,
                "Original transcript does not match the requested repository and session"
            );
            let key = format!("{agent}:{id}:{}", &digest(&session.to_string())[..12]);
            store.put("session", &key, &session)?;
            if !saved.iter().any(|(existing, _)| existing == &key) {
                saved.insert(0, (key, session));
            }
        }
    }
    Ok(saved)
}

pub fn enrich(root: &Path, evidence: &mut Value) -> Result<()> {
    let source = s(&evidence["provenance"]["source_type"]).to_owned();
    if ["original_turn", "tool_record"].contains(&source.as_str()) {
        return Ok(());
    }
    let refs = arr(&evidence["provenance"]["original_refs"]).to_vec();
    evidence["originals"] = json!([]);
    evidence["origin_status"] = json!("unavailable");
    evidence["origin_reason"] = json!(if source == "compaction_summary" {
        "no original-turn reference was retained"
    } else {
        "provenance was not classified; re-import the transcript"
    });
    if refs.is_empty() {
        return Ok(());
    }
    let root = &root.canonicalize()?;
    let store = Store::open(root)?;
    let agent = s(&evidence["agent"]).to_owned();
    let own_session = s(&evidence["session_id"]).to_owned();
    let lineage = evidence["lineage"].clone();
    let mut cache = HashMap::new();
    let mut capture_count = 0;
    let mut capture_bytes = 0;
    let mut originals = vec![];
    let mut missing = vec![];
    if evidence["provenance"]["references_truncated"] == true {
        missing.push("original-reference capture limit exceeded");
    }
    for reference in refs.iter().take(32) {
        let session_id = reference["session_id"].as_str().unwrap_or(&own_session);
        if !reference["message_id"].is_string() && !reference["turn_id"].is_string() {
            missing.push("original reference has no message or turn identity");
            continue;
        }
        let crossing = session_id != own_session;
        if crossing
            && (lineage["parent_session_id"] != session_id
                || !lineage["through_turn_id"].is_string())
        {
            missing.push("session ancestry or fork boundary unavailable");
            continue;
        }
        if !cache.contains_key(session_id) {
            let target_refs: Vec<_> = refs
                .iter()
                .take(32)
                .filter(|r| r["session_id"].as_str().unwrap_or(&own_session) == session_id)
                .cloned()
                .collect();
            let boundary = if crossing {
                lineage["through_turn_id"].clone()
            } else {
                Value::Null
            };
            match captures(root, &store, &agent, session_id, &target_refs, &boundary) {
                Ok(value) => {
                    capture_count += value.len();
                    capture_bytes += value
                        .iter()
                        .map(|(_, s)| s.to_string().len())
                        .sum::<usize>();
                    if capture_count > 20 || capture_bytes > 40_000_000 {
                        missing.push("original-reference lookup limit exceeded");
                        break;
                    }
                    cache.insert(session_id.to_owned(), value);
                }
                Err(_) => {
                    missing.push("original transcript unavailable or capture limit exceeded");
                    continue;
                }
            }
        }
        let mut matches = BTreeMap::new();
        let mut conflicting = false;
        for (key, session) in &cache[session_id] {
            if crate::validate("Session", session).is_err() {
                missing.push("saved original capture is invalid");
                continue;
            }
            if !super::belongs(s(&session["cwd"]), root) {
                continue;
            }
            let events = arr(&session["events"]);
            let boundary = if crossing {
                events
                    .iter()
                    .rposition(|e| {
                        e["provenance"]["turn_id"] == lineage["through_turn_id"]
                            && e["provenance"]["basis"] == "native_event"
                    })
                    .map(|at| at + 1)
            } else {
                Some(events.len())
            };
            for (index, event) in events.iter().enumerate() {
                if !matches_reference(event, reference) {
                    continue;
                }
                let metadata = &event["provenance"];
                // Retained messages are copies appended at compaction time. They
                // cannot move a fork cutoff forward or supply their own chronology.
                if crossing && metadata["turn_id"] != lineage["through_turn_id"] {
                    let permitted = boundary.is_some_and(|end| {
                        if metadata["basis"] == "native_event" {
                            index < end
                        } else {
                            events.iter().take(end).any(|e| {
                                e["provenance"]["basis"] == "native_event"
                                    && e["provenance"]["turn_id"].is_string()
                                    && e["provenance"]["turn_id"] == metadata["turn_id"]
                            })
                        }
                    });
                    if !permitted {
                        continue;
                    }
                }
                let identity = format!(
                    "{}:{}:{}",
                    s(&metadata["turn_id"]),
                    s(&metadata["message_id"]),
                    s(&event["kind"])
                );
                let resolved = json!({"session_key":key,"agent":agent,"session_id":session_id,"event_id":event["id"],"turn_id":metadata["turn_id"],"message_id":metadata["message_id"],
                    "content_hash":provenance::content_hash(event),"relation":reference["relation"],"file":session["path"],"source_line":event["source_line"]});
                if let Some(previous) = matches.get(&identity) {
                    let previous: &Value = previous;
                    if previous["content_hash"] != resolved["content_hash"] {
                        conflicting = true;
                    }
                } else {
                    matches.insert(identity, resolved);
                }
            }
        }
        if conflicting {
            missing.push("original reference is ambiguous across saved captures");
        } else if matches.is_empty() {
            missing
                .push("referenced original was not captured within the recorded session boundary");
        } else {
            for reference in matches.into_values() {
                if !originals.iter().any(|r: &Value| {
                    r["session_key"] == reference["session_key"]
                        && r["event_id"] == reference["event_id"]
                }) {
                    originals.push(reference);
                }
            }
        }
    }
    if refs.len() > 32 {
        missing.push("original-reference lookup limit exceeded");
    }
    evidence["origin_status"] = json!(if originals.is_empty() {
        "unavailable"
    } else if missing.is_empty() {
        "available"
    } else {
        "partial"
    });
    missing.sort_unstable();
    missing.dedup();
    evidence["origin_reason"] = json!(missing.join("; "));
    evidence["originals"] = json!(originals);
    Ok(())
}

pub fn open(root: &Path, reference: &Value) -> Result<Value> {
    let root = &root.canonicalize()?;
    let store = Store::open(root)?;
    let session = store
        .get("session", s(&reference["session_key"]))
        .context("Original turn unavailable — saved capture is missing or unreadable")?;
    crate::validate("Session", &session)
        .context("Original turn unavailable — saved capture is invalid")?;
    ensure!(
        super::belongs(s(&session["cwd"]), root)
            && session["agent"] == reference["agent"]
            && session["id"] == reference["session_id"],
        "Original turn unavailable — saved session does not match"
    );
    let event = arr(&session["events"])
        .iter()
        .find(|e| {
            e["id"] == reference["event_id"]
                && provenance::original(e)
                && provenance::content_hash(e) == s(&reference["content_hash"])
        })
        .ok_or_else(|| {
            anyhow::anyhow!("Original turn unavailable — saved message is missing or changed")
        })?;
    Ok(super::event_evidence(&session, event))
}

pub fn status(evidence: &Value) -> Option<String> {
    if ["original_turn", "tool_record"].contains(&s(&evidence["provenance"]["source_type"])) {
        return None;
    }
    Some(match s(&evidence["origin_status"]) {
        "available" => "Original turns available · source links preserve context, not proof of every summary claim".into(),
        "partial" => format!("Some original turns unavailable — {}. Original rationale unknown where sources are missing.",s(&evidence["origin_reason"])),
        _ => format!("Original turn unavailable — {}. Original rationale unknown.",evidence["origin_reason"].as_str().unwrap_or("no verified source reference")),
    })
}
