use crate::{arr, s, security, storage::Store};
use anyhow::{Result, ensure};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Component, Path},
};

fn relative(root: &Path, cwd: &str, file: &str) -> Option<String> {
    let path = Path::new(file);
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    let canonical_cwd = Path::new(cwd).canonicalize().ok()?;
    let path = if path.is_absolute() {
        path.strip_prefix(cwd)
            .map(|p| canonical_cwd.join(p))
            .unwrap_or_else(|_| path.to_path_buf())
    } else {
        canonical_cwd.join(path)
    };
    let file = path
        .strip_prefix(root)
        .ok()?
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    security::allowed(&file).then_some(file)
}

pub fn records(root: &Path, session: &Value, key: &str) -> Vec<Value> {
    let mut result = vec![];
    for event in arr(&session["events"]) {
        if arr(&event["code_edits"]).is_empty() {
            continue;
        }
        let output = arr(&session["events"]).iter().rev().find(|e| {
            e["kind"] == "tool_output"
                && event["call_id"].is_string()
                && e["call_id"] == event["call_id"]
        });
        let output_text = output.map(|e| s(&e["text"])).unwrap_or("");
        if event["failed"] == true
            || output.is_some_and(|e| e["failed"] == true)
            || [
                "apply_patch verification failed",
                "Failed to apply patch",
                "Error parsing function call",
                "isError\":true",
            ]
            .iter()
            .any(|failure| output_text.contains(failure))
        {
            continue;
        }
        let state = if s(&event["tool"]) == "file_change"
            || output_text.contains("Success. Updated the following files:")
            || output_text.contains("has been updated successfully")
            || output_text.contains("File created successfully")
        {
            "applied"
        } else {
            "recorded"
        };
        for edit in arr(&event["code_edits"]) {
            let Some(file) = relative(root, s(&session["cwd"]), s(&edit["file"])) else {
                continue;
            };
            let mut record = edit.clone();
            record["file"] = json!(file);
            record["id"] = json!(security::digest(&format!(
                "{key}:{}:{file}:{}",
                s(&event["id"]),
                s(&edit["text"])
            )));
            record["agent"] = session["agent"].clone();
            record["session_id"] = session["id"].clone();
            record["session_key"] = json!(key);
            record["session_path"] = session["path"].clone();
            record["event_id"] = event["id"].clone();
            record["source_line"] = event["source_line"].clone();
            record["timestamp"] = event["timestamp"].clone();
            record["state"] = json!(state);
            result.push(record);
        }
    }
    result
}

/// Latest recorded edit per file, from up to three recent coding sessions.
/// An unknown date is displayed as unknown, never invented from a file's mtime.
pub fn recent_code(root: &Path, sessions: &[Value]) -> Vec<Value> {
    let cutoff = Utc::now() - Duration::days(7);
    let mut all = vec![];
    for session in sessions {
        let key = format!(
            "{}:{}:{}",
            s(&session["agent"]),
            s(&session["id"]),
            &security::digest(&session.to_string())[..12]
        );
        all.extend(records(root, session, &key).into_iter().filter(|r| {
            DateTime::parse_from_rfc3339(s(&r["timestamp"])).map_or(true, |date| date >= cutoff)
        }));
    }
    all.sort_by(|a, b| {
        DateTime::parse_from_rfc3339(s(&b["timestamp"]))
            .ok()
            .cmp(&DateTime::parse_from_rfc3339(s(&a["timestamp"])).ok())
            .then_with(|| crate::n(&b["source_line"]).cmp(&crate::n(&a["source_line"])))
    });
    let mut sessions = HashSet::new();
    let mut files = HashSet::new();
    let mut result = vec![];
    for record in all {
        let key = s(&record["session_key"]).to_owned();
        if !sessions.contains(&key) && sessions.len() >= 3 {
            continue;
        }
        sessions.insert(key);
        if files.insert(s(&record["file"]).to_owned()) {
            result.push(record);
        }
        if result.len() == 50 {
            break;
        }
    }
    result
}

pub fn edit_ref(record: &Value) -> Value {
    json!({"id":record["id"],"file":record["file"],"session_key":record["session_key"],
        "event_id":record["event_id"],"agent":record["agent"],"timestamp":record["timestamp"]})
}

/// Resolve against the immutable saved session, including when a newer session has arrived.
pub fn saved_edit(root: &Path, reference: &Value) -> Result<(Value, Value)> {
    let key = s(&reference["session_key"]);
    let session = Store::open(root)?.get("session", key)?;
    crate::validate("Session", &session)?;
    ensure!(
        super::belongs(s(&session["cwd"]), root),
        "The recorded edit belongs to a different repository"
    );
    let record = records(root, &session, key)
        .into_iter()
        .find(|r| {
            r["id"] == reference["id"]
                && r["file"] == reference["file"]
                && r["event_id"] == reference["event_id"]
        })
        .ok_or_else(|| {
            anyhow::anyhow!("The selected session edit is unavailable; refresh the files")
        })?;
    Ok((record, session))
}
