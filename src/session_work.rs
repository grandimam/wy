//! Immutable session work. Never reconstruct historical code from today's tree.
use crate::{arr, history, insights, s, security, storage::Store};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
};

pub fn snapshot_key(session: &Value) -> String {
    format!(
        "{}:{}:{}",
        s(&session["agent"]),
        s(&session["id"]),
        &security::digest(&session.to_string())[..12]
    )
}

/// Latest known source-event date, not transcript mtime or wy capture time.
/// No supported importer proves which external agent is currently active.
pub fn latest(sessions: &[Value]) -> Option<&Value> {
    sessions
        .iter()
        .filter(|s| !arr(&s["events"]).is_empty())
        .max_by_key(|session| {
            chrono::DateTime::parse_from_rfc3339(s(&insights::session_dates(session).1)).ok()
        })
}

pub fn title(session: &Value) -> String {
    arr(&session["events"])
        .iter()
        .find(|e| {
            e["kind"] == "user"
                && history::provenance::original(e)
                && !s(&e["text"]).trim().is_empty()
        })
        .map(|e| security::short(s(&e["text"]), 100).replace('\n', " "))
        .unwrap_or_else(|| format!("{} session {}", s(&session["agent"]), s(&session["id"])))
}

pub fn build(review: &Value, session: &Value) -> Value {
    let key = snapshot_key(session);
    let edits = history::session_edits(Path::new(s(&review["root"])), session, &key);
    let mut files: BTreeSet<String> = edits.iter().map(|e| s(&e["file"]).to_owned()).collect();
    files.extend(
        arr(&session["events"])
            .iter()
            .flat_map(insights::decisions)
            .map(|d| s(&d["file"]).to_owned()),
    );
    let turns = request_turns(session, &edits);
    let steps:Vec<_>=turns.iter().flat_map(|turn|arr(&turn["edit_indices"]).iter().map(|index| {
        let request=&turn["request"];
        json!({"edit":edits[crate::n(index)],"request":if history::provenance::original(request){json!(security::short(s(&request["text"]),700))}else{Value::Null},"request_id":request["id"]})
    })).collect();
    json!({"scope_kind":"session","id":review["id"],"root":review["root"],
        "session":{"storage_key":key,"id":session["id"],"agent":session["agent"],"title":title(session),"last_event":insights::session_dates(session).1},
        "changes":files.into_iter().map(|f|json!({"file":f})).collect::<Vec<_>>(),
        "edits":edits,"steps":steps,"turns":turns,"warnings":session["warnings"]})
}

/// Keep every user-bounded turn, including planning/notes with no edits. Unknown
/// user provenance starts a new boundary but cannot borrow an earlier request.
fn request_turns(session: &Value, edits: &[Value]) -> Vec<Value> {
    let mut by_event: HashMap<(String, usize), Vec<usize>> = HashMap::new();
    for (i, e) in edits.iter().enumerate() {
        by_event
            .entry((s(&e["event_id"]).into(), crate::n(&e["source_line"])))
            .or_default()
            .push(i);
    }
    let blank = || json!({"request":null,"prior_request":null,"messages":[],"activity":[],"edit_indices":[]});
    let mut current = blank();
    let mut turns = vec![];
    let mut prior = Value::Null;
    for event in arr(&session["events"]) {
        if event["kind"] == "user" {
            if !current["request"].is_null()
                || !arr(&current["messages"]).is_empty()
                || !arr(&current["activity"]).is_empty()
                || !arr(&current["edit_indices"]).is_empty()
            {
                turns.push(current);
            }
            current = blank();
            current["request"] = event.clone();
            if history::provenance::original(event) {
                if history::attribution::confirmation(s(&event["text"])) {
                    current["prior_request"] = prior.clone();
                } else if !s(&event["text"]).trim().is_empty() {
                    prior = event.clone();
                }
            } else {
                prior = Value::Null;
            }
        } else if ["assistant", "rationale", "summary"].contains(&s(&event["kind"])) {
            current["messages"]
                .as_array_mut()
                .unwrap()
                .push(event.clone());
        } else {
            current["activity"].as_array_mut().unwrap().push(event.clone());
        }
        if let Some(indices) =
            by_event.remove(&(s(&event["id"]).into(), crate::n(&event["source_line"])))
        {
            current["edit_indices"]
                .as_array_mut()
                .unwrap()
                .extend(indices.into_iter().map(|i| json!(i)));
        }
    }
    if !current["request"].is_null()
        || !arr(&current["activity"]).is_empty()
        || !arr(&current["messages"]).is_empty()
        || !arr(&current["edit_indices"]).is_empty()
    {
        turns.push(current);
    }
    turns
}

pub fn empty(review: &Value) -> Value {
    json!({"scope_kind":"session","id":review["id"],"root":review["root"],"session":null,"changes":[],"edits":[],"steps":[],"turns":[],"warnings":[]})
}

pub fn load(root: &Path, work: &Value) -> Result<Value> {
    let key = s(&work["session"]["storage_key"]);
    ensure!(!key.is_empty(), "No selected session");
    let session = Store::open(root)?.get("session", key)?;
    crate::validate("Session", &session)?;
    ensure!(
        history::belongs(s(&session["cwd"]), root),
        "Session belongs to another repository"
    );
    ensure!(
        snapshot_key(&session) == key
            && session["agent"] == work["session"]["agent"]
            && session["id"] == work["session"]["id"],
        "Session snapshot identity changed"
    );
    Ok(session)
}

pub fn edit_evidence(edit: &Value) -> Value {
    json!({"id":format!("session-edit-{}",s(&edit["id"])),"kind":"session_edit","file":edit["file"],"format":edit["format"],
        "text":edit["text"],"state":edit["state"],"operation":edit["operation"],"truncated":edit["truncated"],
        "session_key":edit["session_key"],"session_id":edit["session_id"],"agent":edit["agent"],"event_id":edit["event_id"],"timestamp":edit["timestamp"],
        "edit_ref":history::edit_ref(edit),"provenance":{"source_type":"tool_record","basis":"captured_edit","original_refs":[]}})
}

/// Bounded selected-session evidence only. No current source reads or other sessions.
pub fn packet(work: &Value, session: &Value) -> Value {
    let mut evidence = vec![];
    let mut used = 0;
    let mut omitted = 0;
    for edit in arr(&work["edits"]) {
        let mut e = edit_evidence(edit);
        let text = security::short(&security::redact(s(&e["text"])), 10000);
        if used + text.len() > 65000 || evidence.len() >= 50 {
            omitted += 1;
            continue;
        }
        used += text.len();
        e["truncated"] = json!(e["truncated"] == true || text != s(&e["text"]));
        e["text"] = json!(text);
        evidence.push(e);
    }
    // Explicit decision records first; remaining public context in source order.
    let mut events: Vec<_> = arr(&session["events"])
        .iter()
        .filter(|e| ["user", "assistant", "summary", "rationale"].contains(&s(&e["kind"])))
        .collect();
    events.sort_by_key(|e| insights::decisions(e).is_empty());
    for event in events {
        let mut e = history::event_evidence(session, event);
        let text = security::short(&security::redact(s(&e["text"])), 6000);
        if used + text.len() > 100000 || evidence.len() >= 80 {
            omitted += 1;
            continue;
        }
        used += text.len();
        e["truncated"] = json!(e["truncated"] == true || text != s(&e["text"]));
        e["text"] = json!(text);
        e["session_key"] = work["session"]["storage_key"].clone();
        evidence.push(e);
    }
    json!({"scope_kind":"session","session":work["session"],"evidence":evidence,"omitted_items":omitted,"warnings":work["warnings"],
        "limitations":"Captured edits are historical excerpts, not reconstructed complete files. Recorded tool inputs may not have executed. Shell, manual, failed and unsupported edits may be missing. No current code or other sessions were used."})
}

/// Whole-file comparison is justified only for a complete, successful final write.
/// Partial patches are never applied to current code to invent a historical base.
pub fn compare(root: &Path, work: &Value, edit: &Value) -> Value {
    let current = security::read_source(root, s(&edit["file"])).map(|t| security::redact(&t));
    let latest = arr(&work["edits"])
        .iter()
        .rev()
        .find(|e| e["file"] == edit["file"]);
    let status = if current.is_none() {
        "Current code unavailable"
    } else if edit["truncated"] == true
        || edit["format"] != "code"
        || edit["state"] != "applied"
        || latest.is_none_or(|e| e != edit)
    {
        "Partial or unconfirmed capture · full-file comparison unavailable"
    } else if current.as_deref().unwrap().trim_end_matches('\n')
        == s(&edit["text"]).trim_end_matches('\n')
    {
        "Matches captured text"
    } else {
        "Changed since captured edit"
    };
    json!({"status":status,"current":current,"truncated":false,
        "note":"Comparison uses redacted text and ignores final newlines; it does not establish reversal, supersession or authorship."})
}
