//! Link current diff hunks to the recorded agent edits that produced them.
//!
//! Matching compares the exact (trimmed) text of changed lines in each hunk with
//! the lines a recorded edit added or removed. A match shows that the agent's
//! transcript recorded that edit; it does not prove who made the final change.
//! Shell commands, formatters and human edits leave no edit record.
use super::recent;
use crate::{arr, s};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};

/// Changed lines of a hunk or an edit, split by direction.
#[derive(Default)]
struct Lines {
    added: HashSet<String>,
    removed: HashSet<String>,
}

/// Lines made only of punctuation (`}`, `);`) match almost anywhere, so ignore them.
fn significant(line: &str) -> Option<String> {
    let line = line.trim();
    line.chars()
        .any(char::is_alphanumeric)
        .then(|| line.to_owned())
}

fn edit_lines(edit: &Value) -> Lines {
    let mut lines = Lines::default();
    for line in s(&edit["text"]).lines() {
        if edit["format"] == "code" {
            lines.added.extend(significant(line));
        } else if let Some(text) = line.strip_prefix('+').filter(|_| !line.starts_with("+++")) {
            lines.added.extend(significant(text));
        } else if let Some(text) = line.strip_prefix('-').filter(|_| !line.starts_with("---")) {
            lines.removed.extend(significant(text));
        }
    }
    lines
}

/// Hunks of a unified diff: index of each `@@` line and its changed lines.
fn hunks(diff: &str) -> Vec<(usize, Lines)> {
    let mut result: Vec<(usize, Lines)> = vec![];
    for (index, line) in diff.lines().enumerate() {
        if line.starts_with("@@") {
            result.push((index, Lines::default()));
        } else if result.is_empty() && line.starts_with('+') && !line.starts_with("+++") {
            result.push((index, Lines::default()));
            result.last_mut().unwrap().1.added.extend(significant(line.strip_prefix('+').unwrap()));
        } else if let Some((_, hunk)) = result.last_mut() {
            if let Some(text) = line.strip_prefix('+').filter(|_| !line.starts_with("+++")) {
                hunk.added.extend(significant(text));
            } else if let Some(text) = line.strip_prefix('-').filter(|_| !line.starts_with("---")) {
                hunk.removed.extend(significant(text));
            }
        }
    }
    result
}

/// Compare one direction: additions when the hunk has any, otherwise removals.
fn sides<'a>(hunk: &'a Lines, edit: &'a Lines) -> (&'a HashSet<String>, &'a HashSet<String>) {
    if hunk.added.is_empty() {
        (&hunk.removed, &edit.removed)
    } else {
        (&hunk.added, &edit.added)
    }
}

/// Recorded agent edits (of `file`, or all files), oldest first, across the given sessions.
pub fn edits(root: &Path, sessions: &[Value], file: Option<&str>) -> Vec<Value> {
    let mut result = vec![];
    for session in sessions {
        let key = format!(
            "{}:{}:{}",
            s(&session["agent"]),
            s(&session["id"]),
            &crate::security::digest(&session.to_string())[..12]
        );
        result.extend(
            recent::records(root, session, &key)
                .into_iter()
                .filter(|r| file.is_none_or(|f| r["file"] == f)),
        );
    }
    result.sort_by(|a, b| {
        s(&a["timestamp"])
            .cmp(s(&b["timestamp"]))
            .then_with(|| crate::n(&a["source_line"]).cmp(&crate::n(&b["source_line"])))
    });
    result
}

/// For each hunk of `diff`: its `@@` line index, a status and the best-matching edit.
///
/// Status: `agent` (every changed line appears in recorded edits), `partial`
/// (some do; the rest changed outside a recorded edit) or `none`.
pub fn hunk_sources(diff: &str, edits: &[Value]) -> Vec<Value> {
    let parsed: Vec<_> = edits.iter().map(edit_lines).collect();
    hunks(diff)
        .into_iter()
        .map(|(line, hunk)| {
            let mut covered = HashSet::new();
            let mut best: Option<(usize, usize)> = None;
            for (index, lines) in parsed.iter().enumerate() {
                let (wanted, have) = sides(&hunk, lines);
                let matched: Vec<_> = wanted.intersection(have).cloned().collect();
                if matched.is_empty() {
                    continue;
                }
                // Prefer the edit covering most lines; on a tie, the latest one.
                if best.is_none_or(|(_, count)| matched.len() >= count) {
                    best = Some((index, matched.len()));
                }
                covered.extend(matched);
            }
            let total = sides(&hunk, &Lines::default()).0.len();
            let status = if covered.is_empty() {
                "none"
            } else if covered.len() == total {
                "agent"
            } else {
                "partial"
            };
            json!({"line":line,"status":status,"covered":covered.len(),"total":total,
                "edit":best.map(|(i, _)| edits[i].clone())})
        })
        .collect()
}

/// Added and removed line counts of a recorded edit.
pub fn edit_size(edit: &Value) -> (usize, usize) {
    if edit["format"] == "code" {
        return (s(&edit["text"]).lines().count(), 0);
    }
    let lines = s(&edit["text"]).lines();
    lines.fold((0, 0), |(a, r), line| {
        if line.starts_with('+') && !line.starts_with("+++") {
            (a + 1, r)
        } else if line.starts_with('-') && !line.starts_with("---") {
            (a, r + 1)
        } else {
            (a, r)
        }
    })
}

/// The conversation around an edit: the user request that started the turn,
/// the agent's messages up to the edit, and its next message after it.
pub fn turn(session: &Value, event_id: &Value) -> Vec<Value> {
    let events = arr(&session["events"]);
    let Some(at) = events.iter().position(|e| e["id"] == *event_id) else {
        return vec![];
    };
    let message = |e: &Value| ["user", "assistant", "summary", "rationale"].contains(&s(&e["kind"]));
    let start = events[..at]
        .iter()
        .rposition(|e| e["kind"] == "user")
        .unwrap_or(0);
    let mut result: Vec<Value> = events[start..at]
        .iter()
        .filter(|e| message(e))
        .cloned()
        .collect();
    result.push(events[at].clone());
    if let Some(next) = events[at + 1..]
        .iter()
        .take_while(|e| e["kind"] != "user")
        .find(|e| e["kind"] == "assistant")
    {
        result.push(next.clone());
    }
    result
}

/// Each hunk of a unified diff: index of its `@@` line and its full text.
fn hunk_blocks(diff: &str) -> Vec<(usize, String)> {
    let mut result: Vec<(usize, String)> = vec![];
    for (index, line) in diff.lines().enumerate() {
        if line.starts_with("@@") {
            result.push((index, format!("{line}\n")));
        } else if let Some((_, text)) = result.last_mut() {
            text.push_str(line);
            text.push('\n');
        } else if line.starts_with('+') && !line.starts_with("+++") {
            // A diff with no hunk headers (e.g. a whole new file): treat it as one hunk.
            result.push((index, format!("@@ +1 @@\n{line}\n")));
        }
    }
    result
}

/// "line 12 · fn get()" from a hunk header `@@ -a,b +12,4 @@ fn get()`.
fn hunk_label(header: &str) -> String {
    let start = header
        .split_whitespace()
        .nth(2)
        .and_then(|r| r.strip_prefix('+'))
        .and_then(|r| r.split(',').next())
        .unwrap_or("?");
    let context = header.splitn(3, "@@").nth(2).unwrap_or("").trim();
    if context.is_empty() {
        format!("line {start}")
    } else {
        format!("line {start} · {context}")
    }
}

/// Split a message into its first sentence (the headline) and the rest.
pub fn headline(text: &str) -> (String, String) {
    let text = text.trim();
    let end = text
        .char_indices()
        .find(|&(i, c)| {
            c == '\n'
                || (matches!(c, '.' | '!' | '?')
                    && text[i + c.len_utf8()..]
                        .chars()
                        .next()
                        .is_none_or(char::is_whitespace))
        })
        .map(|(i, c)| i + if c == '\n' { 0 } else { c.len_utf8() })
        .unwrap_or(text.len());
    (text[..end].trim().to_owned(), text[end..].trim().to_owned())
}

/// Recognize confirmations explicitly; a short instruction can still be substantive.
pub fn confirmation(text:&str)->bool {
    let normalized=text.split_whitespace().map(|word|word.trim_matches([',','.','!','?'])).collect::<Vec<_>>().join(" ").to_lowercase();
    ["yes","yes do it","yes please","do it","continue","continue with this","go ahead","ok","okay","implement it","proceed","please do","sounds good","sure","yep"].contains(&normalized.as_str())
}

/// The agent's reasons for the changes to one file, with the hunks in file order.
///
/// Each reason is the agent message written just before an edit, with the user
/// request that started that turn and the agent's next message. Hunks produced by
/// the same message share one reason; `hunks[i].reason` is its index (null when no
/// recorded edit matches) and `first` marks the hunk where that reason first appears.
/// Reasons are numbered by that first appearance, so the view reads top to bottom.
pub fn reasons(root: &Path, sessions: &[Value], file: &str, diff: Option<&str>) -> Value {
    let edits = edits(root, sessions, Some(file));
    // (edit, change) pairs: current hunks when the file has a diff, else the recorded edits.
    let mut pairs: Vec<(Option<Value>, Value)> = vec![];
    if let Some(diff) = diff {
        let sources = hunk_sources(diff, &edits);
        for ((line, text), source) in hunk_blocks(diff).into_iter().zip(sources) {
            let (added, removed) = edit_size(&json!({"format":"patch","text":text}));
            let header = text.lines().next().unwrap_or("");
            let change = json!({"id":format!("{file}:{line}"),"label":hunk_label(header),"text":text,
                "format":"patch","added":added,"removed":removed,"status":source["status"]});
            pairs.push((source["edit"].as_object().map(|_| source["edit"].clone()), change));
        }
    } else {
        for edit in &edits {
            let (added, removed) = edit_size(edit);
            let change = json!({"id":format!("{file}:{}",s(&edit["id"])),"label":"recorded edit",
                "text":edit["text"],"format":edit["format"],"added":added,"removed":removed,"status":"recorded"});
            pairs.push((Some(edit.clone()), change));
        }
    }
    let mut groups: Vec<Value> = vec![];
    let mut hunks = vec![];
    for (edit, mut change) in pairs {
        let session = edit
            .as_ref()
            .and_then(|e| sessions.iter().find(|x| x["id"] == e["session_id"] && x["agent"] == e["agent"]));
        let (Some(edit), Some(session)) = (edit, session) else {
            change["reason"] = Value::Null;
            change["first"] = json!(false);
            hunks.push(change);
            continue;
        };
        let turn = turn(session, &edit["event_id"]);
        let at = turn
            .iter()
            .position(|e| e["id"] == edit["event_id"])
            .unwrap_or(turn.len());
        let message = turn[..at].iter().rev().find(|e| e["kind"] == "assistant");
        let request = turn[..at].iter().find(|e| e["kind"] == "user");
        // Preserve earlier substantive context for confirmations, without claiming causation.
        let events = arr(&session["events"]);
        let prior = request
            .filter(|r| confirmation(s(&r["text"])))
            .and_then(|r| events.iter().position(|e| e["id"] == r["id"]))
            .and_then(|i| events[..i].iter().rev().find(|e| e["kind"] == "user" && super::provenance::original(e) && !s(&e["text"]).trim().is_empty() && !confirmation(s(&e["text"])) && !["<environment_context>","<permissions","# AGENTS.md","<turn_aborted>"].iter().any(|prefix|s(&e["text"]).trim_start().starts_with(prefix))));
        let after = turn[at..].iter().find(|e| e["kind"] == "assistant");
        let anchor = message
            .or(request)
            .map(|e| s(&e["id"]))
            .unwrap_or(s(&edit["event_id"]));
        let key = format!("{}:{}:{anchor}", s(&session["agent"]), s(&session["id"]));
        let index = match groups.iter().position(|g| g["key"] == key) {
            Some(index) => index,
            None => {
                let model = message
                    .and_then(|m| m["model"].as_str())
                    .or_else(|| {
                        arr(&session["events"])
                            .iter()
                            .find(|e| e["id"] == edit["event_id"])
                            .and_then(|e| e["model"].as_str())
                    });
                // How many context compactions happened before this edit in its session.
                // The reader marks the point where the agent's memory was cut.
                let compactions = arr(&session["events"])
                    .iter()
                    .take_while(|e| e["id"] != edit["event_id"])
                    .filter(|e| e["provenance"]["source_type"] == "compaction_summary")
                    .count();
                groups.push(json!({"key":key,"agent":session["agent"],"model":model,"session_id":session["id"],
                    "message":message,"rationale":turn[..at].iter().rev().find(|e|e["kind"]=="rationale"),"request":request,"prior_request":prior,"after":after,"edit":edit,"compactions":compactions}));
                groups.len() - 1
            }
        };
        change["reason"] = json!(index);
        change["first"] = json!(!hunks.iter().any(|h: &Value| h["reason"] == json!(index)));
        hunks.push(change);
    }
    json!({"reasons":groups,"hunks":hunks})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn patch(text: &str) -> Value {
        json!({"format":"patch","text":text})
    }
    const DIFF: &str = "--- a/src/cache.rs\n+++ b/src/cache.rs\n@@ -10,3 +10,4 @@\n fn get() {\n-    fetch()\n+    cache.get(key)\n+        .unwrap_or_else(fetch)\n }\n@@ -40,1 +41,2 @@\n+    log::debug!(\"hit\");\n }\n";

    #[test]
    fn hunks_match_recorded_edits_by_exact_changed_lines() {
        let agent = patch("@@\n-    fetch()\n+    cache.get(key)\n+        .unwrap_or_else(fetch)\n }");
        let result = hunk_sources(DIFF, &[agent]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["line"], 2);
        assert_eq!(result[0]["status"], "agent");
        assert_eq!(result[1]["status"], "none");
        assert!(result[1]["edit"].is_null());
    }
    #[test]
    fn partial_matches_and_punctuation_are_not_overclaimed() {
        // The agent wrote one of the two lines; a lone `}` never counts as a match.
        let partial = patch("+    cache.get(key)\n+ }");
        let result = hunk_sources(DIFF, &[partial]);
        assert_eq!(result[0]["status"], "partial");
        assert_eq!(result[0]["covered"], 1);
        assert_eq!(result[0]["total"], 2);
        let braces = patch("+}\n+);");
        assert_eq!(hunk_sources(DIFF, &[braces])[0]["status"], "none");
    }
    #[test]
    fn latest_edit_wins_ties_and_sizes_count_changed_lines() {
        let first = json!({"format":"code","text":"    cache.get(key)\n        .unwrap_or_else(fetch)","event_id":"1"});
        let second = patch("+    cache.get(key)\n+        .unwrap_or_else(fetch)");
        let mut second = second;
        second["event_id"] = json!("2");
        let result = hunk_sources(DIFF, &[first, second]);
        assert_eq!(result[0]["edit"]["event_id"], "2");
        assert_eq!(edit_size(&patch("+a\n+b\n-c\n context")), (2, 1));
    }
    #[test]
    fn turns_hold_the_request_messages_before_an_edit_and_the_reply() {
        let session = json!({"id":"s","events":[
            {"id":"1","kind":"user","text":"Add a cache"},
            {"id":"2","kind":"assistant","text":"I'll cache responses."},
            {"id":"3","kind":"change","text":"patch"},
            {"id":"4","kind":"tool_output","text":"ok"},
            {"id":"5","kind":"assistant","text":"Done."},
            {"id":"6","kind":"change","text":"patch"},
            {"id":"7","kind":"user","text":"Next"}]});
        let ids = |events: Vec<Value>| events.iter().map(|e| s(&e["id"]).to_owned()).collect::<Vec<_>>();
        assert_eq!(ids(turn(&session, &json!("3"))), ["1", "2", "3", "5"]);
    }
    #[test]
    fn headlines_split_at_the_first_sentence_not_at_dotted_names() {
        assert_eq!(
            headline("I'll cache responses. Repeated reads reuse them."),
            ("I'll cache responses.".into(), "Repeated reads reuse them.".into())
        );
        assert_eq!(headline("Use cache.get(key) here"), ("Use cache.get(key) here".into(), String::new()));
        assert_eq!(headline("First line\nsecond").0, "First line");
    }
    #[test]
    fn hunks_are_grouped_under_the_message_that_explains_them() {
        let dir = tempfile::tempdir().unwrap();
        // Review roots are canonical (macOS temp paths are symlinks).
        let root = &dir.path().canonicalize().unwrap();
        let patch = "*** Begin Patch\n*** Update File: src/cache.rs\n@@\n-    fetch()\n+    cache.get(key)\n+        .unwrap_or_else(fetch)\n*** End Patch";
        let session = json!({"id":"s","agent":"codex","cwd":root,"events":[
            {"id":"1","kind":"user","text":"Avoid repeated fetches"},
            {"id":"2","kind":"assistant","text":"I'll keep responses in memory. Repeated reads reuse them."},
            {"id":"3","kind":"change","tool":"apply_patch","text":patch,"timestamp":"t1","source_line":3,
             "code_edits":super::super::edits::extract("apply_patch", patch)},
            {"id":"4","kind":"assistant","text":"Done."}]});
        let result = reasons(root, &[session], "src/cache.rs", Some(DIFF));
        let reasons = arr(&result["reasons"]);
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0]["message"]["id"], "2");
        assert_eq!(reasons[0]["request"]["id"], "1");
        assert_eq!(reasons[0]["after"]["id"], "4");
        assert_eq!(reasons[0]["compactions"], 0);
        let hunks = arr(&result["hunks"]);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0]["label"], "line 10");
        assert_eq!(hunks[0]["status"], "agent");
        assert_eq!(hunks[0]["reason"], 0);
        assert_eq!(hunks[0]["first"], true);
        // The logging hunk has no recorded edit, so it carries no reason.
        assert!(hunks[1]["reason"].is_null());
        assert_eq!(hunks[1]["added"], 1);
    }
}
