//! Show observable session statements without generating an explanation.
use crate::{arr, s, security, storage::Store};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

fn visible(event: &Value) -> bool {
    ["user", "assistant", "summary"].contains(&s(&event["kind"]))
        && ![
            "<environment_context>",
            "<permissions",
            "# AGENTS.md",
            "<turn_aborted>",
        ]
        .iter()
        .any(|p| s(&event["text"]).trim_start().starts_with(p))
}
fn file_pattern(file: &str, basename_unique: bool) -> regex::Regex {
    let full = format!(r"(?:^|[^\w.-]){}", regex::escape(file));
    let name = if basename_unique {
        format!(
            r"|(?:^|[^\w./-]){}",
            regex::escape(file.rsplit('/').next().unwrap_or(file))
        )
    } else {
        String::new()
    };
    regex::Regex::new(&format!(r"(?i)(?:{full}{name})(?:$|[^\w./-])"))
        .expect("escaped file pattern")
}
pub fn event_evidence(session: &Value, event: &Value) -> Value {
    let identity = json!([
        session["id"],
        session["path"],
        event["kind"],
        event["text"],
        super::provenance::metadata(event)
    ]);
    json!({"id":format!("event-{}-{}-{}",s(&session["agent"]),&security::digest(&identity.to_string())[..16],s(&event["id"])),
        "kind":"session","agent":session["agent"],"session_id":session["id"],"event_id":event["id"],
        "role":event["kind"],"file":session["path"],"start_line":event["source_line"],"text":event["text"],
        "timestamp":event["timestamp"],"call_id":event["call_id"],"provenance":super::provenance::metadata(event),"lineage":session["lineage"],"truncated":event["truncated"]==true})
}
pub fn note_evidence(root: &Path, reference: &Value) -> Result<Value> {
    let session = Store::open(root)?.get("session", s(&reference["session_key"]))?;
    crate::validate("Session", &session)?;
    ensure!(
        super::belongs(s(&session["cwd"]), root),
        "Recorded notes belong to a different repository"
    );
    let event = arr(&session["events"])
        .iter()
        .find(|e| e["id"] == reference["event_id"] && visible(e))
        .ok_or_else(|| anyhow::anyhow!("Recorded note is unavailable; refresh project history"))?;
    let mut evidence = event_evidence(&session, event);
    super::origins::enrich(root, &mut evidence)?;
    Ok(evidence)
}

/// Gaps describe missing signals in the displayed excerpts, not the agent's private reasoning.
pub fn notes(
    review: &Value,
    sessions: &[Value],
    file: &str,
    symbol: Option<&str>,
    edit: Option<&Value>,
) -> Value {
    let basename = file.rsplit('/').next().unwrap_or(file);
    let names: BTreeSet<_> = review["file_hashes"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(f, _)| f.as_str())
        .chain(arr(&review["changes"]).iter().map(|c| s(&c["file"])))
        .chain(arr(&review["recent_code"]).iter().map(|c| s(&c["file"])))
        .collect();
    let unique = names
        .iter()
        .filter(|f| f.rsplit('/').next() == Some(basename))
        .count()
        <= 1;
    let pattern = file_pattern(file, unique);
    let anchor = edit.or_else(|| {
        arr(&review["recent_code"])
            .iter()
            .find(|e| e["file"] == file)
    });
    let mut candidates = vec![];
    for session in sessions {
        let reference = arr(&review["sessions"])
            .iter()
            .find(|r| r["id"] == session["id"] && r["agent"] == session["agent"]);
        let key = reference.map(|r| s(&r["storage_key"])).unwrap_or("");
        if key.is_empty() {
            continue;
        }
        if edit.is_some_and(|e| e["session_key"] != key) {
            continue;
        }
        let events = arr(&session["events"]);
        let anchors: Vec<_> = events
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                anchor.is_some_and(|a| a["session_key"] == key && a["event_id"] == e["id"])
                    || e["kind"] == "change"
                        && arr(&e["files"])
                            .iter()
                            .any(|f| s(f) == file || s(f).ends_with(&format!("/{file}")))
            })
            .map(|(i, _)| i)
            .collect();
        let mut selected = BTreeSet::new();
        // Start with explicit references. Nearby notes are bounded by the user's turn.
        for (i, event) in events.iter().enumerate() {
            if visible(event) && pattern.is_match(s(&event["text"])) {
                selected.insert(i);
            }
        }
        for &at in anchors.iter().rev().take(2) {
            let start = (0..at)
                .rev()
                .find(|&i| events[i]["kind"] == "user" && visible(&events[i]));
            let end = ((at + 1)..events.len())
                .find(|&i| events[i]["kind"] == "user")
                .unwrap_or(events.len());
            if let Some(start) = start {
                selected.insert(start);
            }
            if let Some(i) = (start.unwrap_or(0)..at)
                .rev()
                .find(|&i| events[i]["kind"] == "assistant" && visible(&events[i]))
            {
                selected.insert(i);
            }
            if let Some(i) =
                ((at + 1)..end).find(|&i| events[i]["kind"] == "assistant" && visible(&events[i]))
            {
                selected.insert(i);
            }
        }
        if selected.is_empty() {
            continue;
        }
        let preferred = anchor.is_some_and(|a| a["session_key"] == key);
        let newest = selected
            .iter()
            .next_back()
            .map(|&i| s(&events[i]["timestamp"]))
            .unwrap_or("");
        candidates.push((preferred, newest, session, key, selected));
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(a.1)));
    let mut evidence = vec![];
    let mut refs = vec![];
    if let Some((_, _, session, key, selected)) = candidates.first() {
        let events = arr(&session["events"]);
        let mut recent: Vec<_> = selected.iter().rev().take(6).copied().collect();
        recent.sort_unstable();
        for i in recent {
            evidence.push(event_evidence(session, &events[i]));
            refs.push(json!({"session_key":key,"event_id":events[i]["id"]}));
        }
    }
    for item in &mut evidence {
        // In-memory/synthetic reviews can still display provenance without storage.
        if let Some(root) = review["root"].as_str() {
            if let Err(error) = super::origins::enrich(Path::new(root), item) {
                item["origin_status"] = json!("unavailable");
                item["origin_reason"] = json!(security::redact(&error.to_string()));
            }
        }
    }
    let assistant = evidence
        .iter()
        .filter(|e| e["role"] == "assistant" && super::provenance::original(e))
        .map(|e| s(&e["text"]))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    let has = |terms: &[&str]| terms.iter().any(|t| assistant.contains(t));
    let mut gaps = vec![];
    if assistant.is_empty() {
        if evidence.iter().any(|e| !super::provenance::original(e)) {
            gaps.push("Original turn unavailable or unverified. Original rationale unknown; a summary cannot establish the agent's original reason.");
        }
        gaps.push(
            "An agent explanation of this change was not found in the captured conversation.",
        );
    } else {
        if !has(&[
            "because",
            "so that",
            "in order to",
            "to avoid",
            "chosen",
            "chose",
            "reason",
            "so we",
            "so you",
        ]) {
            gaps.push("What made this approach a good fit for the request?");
        }
        if !has(&[
            "alternative",
            "instead",
            "rather than",
            "tradeoff",
            "trade-off",
            "downside",
            "at the cost",
            "versus",
        ]) {
            gaps.push("What alternatives and tradeoffs were considered?");
        }
        if symbol.is_some_and(|name| {
            !assistant.contains(&name.rsplit('.').next().unwrap_or(name).to_lowercase())
        }) {
            gaps.push("How do these file-level notes explain the selected function?");
        }
    }
    json!({"evidence":evidence,"note_refs":refs,"gaps":gaps,"matched":!assistant.is_empty()})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(id: &str, kind: &str, text: &str) -> Value {
        json!({"id":id,"kind":kind,"text":text,"timestamp":id,"provenance":{"source_type":if ["user","assistant"].contains(&kind){"original_turn"}else{"tool_record"},"basis":"test_native_event","original_refs":[]}})
    }
    fn review() -> Value {
        json!({"changes":[{"file":"src/cache.rs"},{"file":"tests/cache.rs"}],
            "sessions":[{"id":"coding","agent":"codex","storage_key":"immutable"}],
            "recent_code":[{"file":"src/cache.rs","event_id":"3","session_key":"immutable"}]})
    }
    fn session(events: Vec<Value>) -> Value {
        json!({"id":"coding","agent":"codex","path":"session.jsonl","events":events})
    }
    #[test]
    fn nearby_notes_respect_turns_and_keep_exact_sources() {
        let session = session(vec![
            event("1", "user", "Make repeated reads faster."),
            event(
                "2",
                "assistant",
                "Use a cache because repeated requests can reuse a response.",
            ),
            event("3", "change", "patch"),
            event("4", "assistant", "Implemented the cache."),
            event("5", "user", "Now write documentation."),
            event("6", "assistant", "I chose a table instead of prose."),
            event("7", "analysis", "PRIVATE src/cache.rs rationale"),
        ]);
        let result = notes(
            &review(),
            &[session],
            "src/cache.rs",
            Some("Cache.refresh"),
            None,
        );
        let text = result.to_string();
        assert!(text.contains("because repeated requests"));
        assert!(!text.contains("documentation"));
        assert!(!text.contains("PRIVATE"));
        assert!(!text.contains("instead of prose"));
        assert_eq!(
            result["note_refs"],
            json!([
                {"session_key":"immutable","event_id":"1"},
                {"session_key":"immutable","event_id":"2"},
                {"session_key":"immutable","event_id":"4"},
            ])
        );
        assert_eq!(arr(&result["gaps"]).len(), 2);
    }
    #[test]
    fn file_mentions_are_path_aware_and_user_requests_are_not_agent_reasons() {
        let mut review = review();
        review["recent_code"] = json!([]);
        let result = notes(
            &review,
            &[session(vec![
                event(
                    "1",
                    "assistant",
                    "cache.rs changed because tests needed it.",
                ),
                event(
                    "2",
                    "assistant",
                    "tests/cache.rs was changed instead of the production file.",
                ),
                event("3", "user", "Please improve src/cache.rs:10."),
            ])],
            "src/cache.rs",
            None,
            None,
        );
        assert_eq!(arr(&result["evidence"]).len(), 1);
        assert_eq!(result["evidence"][0]["role"], "user");
        assert_eq!(result["matched"], false);
        assert!(s(&result["gaps"][0]).contains("not found"));
        assert!(!file_pattern("src/cache.rs", true).is_match("tests/cache.rs"));
        assert!(file_pattern("src/cache.rs", true).is_match("/project/src/cache.rs"));
        assert!(file_pattern("src/cache.rs", true).is_match("`cache.rs`"));
        assert!(!file_pattern("src/cache.rs", true).is_match("src/cache.rs.old"));
    }
    #[test]
    fn pinned_edits_do_not_borrow_a_reason_from_a_different_session() {
        let result = notes(
            &review(),
            &[session(vec![event(
                "1",
                "assistant",
                "src/cache.rs because latency",
            )])],
            "src/cache.rs",
            None,
            Some(&json!({"session_key":"other-session"})),
        );
        assert!(arr(&result["evidence"]).is_empty());
        assert_eq!(result["matched"], false);
    }
}
