//! The account lens: what the agent did, in intent-sized steps, with the reason it
//! gave (verbatim or none), what it checked, and the captured edits involved.
//! Formed offline from the pinned snapshot. When unsure, do not split: a larger
//! step is more honest than an invented boundary.
use crate::{arr, history, insights, n, s, security};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

const REASON_MARKERS: [&str; 15] = [
    "because",
    "so that",
    ", so ",
    "since ",
    "to avoid",
    "to keep",
    "to ensure",
    "to prevent",
    "to make sure",
    "in order to",
    "rather than",
    "instead of",
    "the reason",
    "this way",
    "otherwise",
];
const ANNOUNCE_MARKERS: [&str; 10] = [
    "i'll ",
    "i will ",
    "now i",
    "next,",
    "next i",
    "let me ",
    "let's ",
    "i'm going to",
    "adding ",
    "i am going to",
];

/// Shell segments: `cd x && cargo test | tail` → ["cd x", "cargo test", "tail"].
/// A validation tool must start a segment; prose inside a heredoc never counts.
fn segments(command: &str) -> Vec<&str> {
    let first = command.lines().next().unwrap_or("");
    let splitter = regex::Regex::new(r"&&|\|\||;|\|").unwrap();
    splitter
        .split(first)
        .map(str::trim)
        .filter(|sg| !sg.is_empty())
        .collect()
}
const TEST_TOOLS: [&str; 12] = [
    "cargo test",
    "npm test",
    "npm run test",
    "pnpm test",
    "yarn test",
    "pytest",
    "go test",
    "jest",
    "vitest",
    "rspec",
    "phpunit",
    "python -m pytest",
];
const BUILD_TOOLS: [&str; 10] = [
    "cargo build",
    "cargo check",
    "cargo clippy",
    "npm run build",
    "tsc",
    "go build",
    "go vet",
    "make",
    "mvn",
    "gradle",
];
fn starts_tool(segment: &str, tool: &str) -> bool {
    let segment = segment
        .trim_start_matches("time ")
        .trim_start_matches("sudo ");
    segment == tool
        || segment
            .strip_prefix(tool)
            .is_some_and(|rest| rest.starts_with([' ', '\t']))
}
fn validation(event: &Value) -> Option<&'static str> {
    if event["kind"] == "test" {
        return Some("test");
    }
    if event["kind"] != "tool_call" && event["kind"] != "change" {
        return None;
    }
    let command = shell_command(s(&event["text"]));
    segments(&command).into_iter().find_map(|sg| {
        if TEST_TOOLS.iter().any(|tool| starts_tool(sg, tool)) {
            Some("test")
        } else if BUILD_TOOLS.iter().any(|tool| starts_tool(sg, tool)) {
            Some("build")
        } else {
            None
        }
    })
}

/// The segment of a shell line that validates: `cd x && cargo test --locked` → `cargo test --locked`.
fn validation_command(command: &str) -> String {
    segments(command)
        .into_iter()
        .find(|sg| {
            TEST_TOOLS
                .iter()
                .chain(BUILD_TOOLS.iter())
                .any(|tool| starts_tool(sg, tool))
        })
        .unwrap_or(command.lines().next().unwrap_or(""))
        .to_owned()
}

fn outcome(session: &Value, event: &Value) -> &'static str {
    if event["failed"] == true {
        return "failed";
    }
    let output = arr(&session["events"]).iter().rev().find(|e| {
        e["kind"] == "tool_output"
            && event["call_id"].is_string()
            && e["call_id"] == event["call_id"]
    });
    let Some(output) = output else {
        // Codex item.completed carries command and output in one event.
        let text = s(&event["text"]);
        return if text.lines().count() > 1 {
            if looks_failed(text) { "failed" } else { "ok" }
        } else {
            "unknown"
        };
    };
    if output["failed"] == true || looks_failed(s(&output["text"])) {
        "failed"
    } else {
        "ok"
    }
}
fn looks_failed(text: &str) -> bool {
    let tail: String = text.lines().rev().take(40).collect::<Vec<_>>().join("\n");
    [
        "error[",
        "FAILED",
        "test result: FAILED",
        "failed",
        "Error:",
        "panicked",
    ]
    .iter()
    .any(|m| tail.contains(m))
        && !tail.contains("0 failed")
}

/// The command portion of a recorded shell call, whatever the transport shape:
/// a plain line, or JSON arguments with a `command` string or argv array.
fn shell_command(text: &str) -> String {
    let first = text.lines().next().unwrap_or("");
    if first.trim_start().starts_with('{')
        && let Ok(args) = serde_json::from_str::<Value>(first)
    {
        let command = &args["command"];
        if let Some(line) = command.as_str() {
            return line.to_owned();
        }
        if command.is_array() {
            return arr(command).iter().map(s).collect::<Vec<_>>().join(" ");
        }
        if let Some(line) = args["cmd"].as_str() {
            return line.to_owned();
        }
    }
    first.to_owned()
}
/// Redirections and in-place editors write files that no edit record captures.
fn writes_files(text: &str) -> bool {
    let command = shell_command(text);
    let command = command.as_str();
    let redirect = regex::Regex::new(r#"(^|[^<>])>{1,2}\s*["']?[\w./-]"#).unwrap();
    redirect.is_match(command)
        || regex::Regex::new(
            r"\b(tee|sed -i|perl -i|mv|cp|install|touch|git checkout --|git apply|patch)\b",
        )
        .unwrap()
        .is_match(command)
}

fn first_sentence(text: &str, limit: usize) -> String {
    let text = text
        .trim()
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    let text = text
        .trim_start_matches(['#', '*', '-', ' '])
        .trim_end_matches('*');
    let end = text
        .char_indices()
        .find(|(i, c)| matches!(c, '.' | '!' | '?' | ':') && text[*i + 1..].starts_with(' '))
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    security::short(&text[..end], limit).replace('\n', " ")
}

/// Turn "Next I'll persist the event id so that duplicates are ignored." into
/// "Persist the event id": strip the announcing preamble and the trailing reason
/// clause, then capitalise. Says what the step did, not what the agent said.
fn imperative(sentence: &str) -> String {
    let mut text = sentence.trim().to_owned();
    let lower = text.to_lowercase();
    for prefix in [
        "now, ",
        "now ",
        "next, ",
        "next ",
        "first, ",
        "first ",
        "then ",
        "okay, ",
        "ok, ",
        "good — ",
        "good, ",
        "right — ",
        "alright, ",
    ] {
        if lower.starts_with(prefix) {
            text = text[prefix.len()..].to_owned();
            break;
        }
    }
    let lower = text.to_lowercase();
    for prefix in [
        "i'll ",
        "i will ",
        "i'm going to ",
        "i am going to ",
        "let me ",
        "let's ",
        "i'll go ahead and ",
        "i'm ",
    ] {
        if lower.starts_with(prefix) {
            text = text[prefix.len()..].to_owned();
            break;
        }
    }
    let lower = text.to_lowercase();
    let cut = [
        " so that ",
        " because ",
        ", so ",
        " since ",
        " in order to ",
        " to avoid ",
        " to keep ",
        " — ",
        " - ",
        ": ",
    ]
    .iter()
    .filter_map(|m| lower.find(m))
    .min()
    .unwrap_or(text.len());
    text.truncate(cut);
    let text = text.trim().trim_end_matches(['.', ',', ';']).to_owned();
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => text,
    }
}

/// A short status message ("Now I'll add the mailer.") announces a step. Long
/// prose that happens to contain "I'll" is discussion, and makes a poor title.
fn announces(text: &str) -> bool {
    let lower = text.to_lowercase();
    text.len() <= 400 && !text.contains("\n#") && ANNOUNCE_MARKERS.iter().any(|m| lower.contains(m))
}
fn gives_reason(text: &str) -> bool {
    let lower = text.to_lowercase();
    REASON_MARKERS.iter().any(|m| lower.contains(m))
}
/// The sentences that carry the reason, verbatim, so a long message quotes the
/// justification rather than everything around it. Falls back to the whole text.
fn reason_excerpt(text: &str) -> String {
    let mut sentences = vec![];
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '!' | '?' | '\n') {
            let end = i + c.len_utf8();
            sentences.push(&text[start..end]);
            start = end;
        }
    }
    if start < text.len() {
        sentences.push(&text[start..]);
    }
    let picked: Vec<&str> = sentences
        .iter()
        .copied()
        .filter(|sn| gives_reason(sn) && !sn.trim().is_empty())
        .collect();
    let excerpt = if picked.is_empty() {
        text.to_owned()
    } else {
        picked.join("").trim().to_owned()
    };
    security::short(&excerpt, 700)
}

struct Draft {
    turn: usize,
    events: Vec<Value>,
    edit_indices: Vec<usize>,
    files: BTreeSet<String>,
    announce: Option<Value>,
    validated_since_edit: bool,
}
impl Draft {
    fn new(turn: usize) -> Self {
        Self {
            turn,
            events: vec![],
            edit_indices: vec![],
            files: BTreeSet::new(),
            announce: None,
            validated_since_edit: false,
        }
    }
    fn substantive(&self) -> bool {
        !self.edit_indices.is_empty()
            || self.events.iter().any(|e| {
                ["assistant", "rationale", "test", "tool_call", "change"].contains(&s(&e["kind"]))
            })
    }
}

/// Segment the selected work into steps. `work` is the immutable session work
/// (`session_work::build`), whose `edits` are indexed by (event_id, source_line).
pub fn steps(session: &Value, work: &Value) -> Vec<Value> {
    let edits = arr(&work["edits"]);
    let mut by_event: HashMap<(String, usize), Vec<usize>> = HashMap::new();
    for (i, e) in edits.iter().enumerate() {
        by_event
            .entry((s(&e["event_id"]).into(), n(&e["source_line"])))
            .or_default()
            .push(i);
    }
    let mut drafts: Vec<Draft> = vec![];
    let mut current = Draft::new(0);
    let mut turn = 0usize;
    let mut requests = vec![];
    let close = |current: &mut Draft, drafts: &mut Vec<Draft>, turn: usize| {
        if current.substantive() {
            drafts.push(std::mem::replace(current, Draft::new(turn)));
        } else {
            // Carry request-only context forward rather than emitting an empty step.
            let kept = std::mem::take(&mut current.events);
            *current = Draft::new(turn);
            current.events = kept;
        }
    };
    for event in arr(&session["events"]) {
        let kind = s(&event["kind"]);
        if kind == "user" {
            if history::provenance::original(event) && !s(&event["text"]).trim().is_empty() {
                requests.push(event.clone());
            }
            turn = requests.len().saturating_sub(1);
            close(&mut current, &mut drafts, turn);
            current.events.push(event.clone());
            continue;
        }
        let indices = by_event
            .remove(&(s(&event["id"]).into(), n(&event["source_line"])))
            .unwrap_or_default();
        let files: BTreeSet<String> = indices
            .iter()
            .map(|i| s(&edits[*i]["file"]).to_owned())
            .collect();
        let has_record = !insights::decisions(event).is_empty();
        if kind == "assistant" && history::provenance::original(event) {
            if has_record && !current.edit_indices.is_empty() {
                close(&mut current, &mut drafts, turn);
            } else if !current.edit_indices.is_empty() && announces(s(&event["text"])) {
                if let Some(previous) = current.announce.replace(event.clone()) {
                    current.events.push(previous);
                }
                continue;
            }
        }
        if !indices.is_empty() && !current.edit_indices.is_empty() {
            let disjoint = files.is_disjoint(&current.files);
            let announced = current.announce.is_some();
            if disjoint && (announced || current.validated_since_edit) {
                let announce = current.announce.take();
                close(&mut current, &mut drafts, turn);
                if let Some(a) = announce {
                    current.events.push(a);
                }
            } else if let Some(a) = current.announce.take() {
                current.events.push(a);
            }
        }
        if let Some(a) = current.announce.take() {
            current.events.push(a);
        }
        if validation(event).is_some() && !current.edit_indices.is_empty() {
            current.validated_since_edit = true;
        }
        if !indices.is_empty() {
            current.validated_since_edit = false;
        }
        current.events.push(event.clone());
        current.edit_indices.extend(indices);
        current.files.extend(files);
    }
    if let Some(a) = current.announce.take() {
        current.events.push(a);
    }
    if current.substantive() {
        drafts.push(current);
    }
    drafts
        .into_iter()
        .enumerate()
        .map(|(i, d)| finish(session, work, &requests, i, d))
        .collect()
}

fn finish(session: &Value, work: &Value, requests: &[Value], ordinal: usize, d: Draft) -> Value {
    let edits = arr(&work["edits"]);
    let agent = s(&session["agent"]);
    let mut recorded = vec![];
    let mut rationale = vec![];
    let mut nearby = vec![];
    let mut checked = vec![];
    let mut gaps = vec![];
    let mut title = String::new();
    // A reason counts only when it was given before the step's last edit. Text
    // after the final edit is a summary of what was done, not why it was started.
    let last_edit = d
        .events
        .iter()
        .rposition(|e| !arr(&e["code_edits"]).is_empty())
        .unwrap_or(usize::MAX);
    for (position, event) in d.events.iter().enumerate() {
        match s(&event["kind"]) {
            "assistant" if history::provenance::original(event) => {
                let text = s(&event["text"]);
                let records = insights::decisions(event);
                // Reasons belong to edits. A step with no edits is discussion; its
                // sentences are context, never a "recorded reason" for code.
                let before_edits = last_edit != usize::MAX && position < last_edit;
                if !records.is_empty() {
                    for r in &records {
                        if title.is_empty() {
                            title = security::short(s(&r["decision"]), 90).replace('\n', " ");
                        }
                        recorded.push(json!({"quote": r["reason"], "event_id": event["id"],
                            "kind": "decision_record", "agent": agent, "timestamp": event["timestamp"]}));
                    }
                } else if text.trim().is_empty() {
                } else if gives_reason(text) && before_edits {
                    recorded.push(
                        json!({"quote": reason_excerpt(text), "event_id": event["id"],
                        "kind": "statement", "agent": agent, "timestamp": event["timestamp"]}),
                    );
                    if title.is_empty() && announces(text) {
                        title = imperative(&first_sentence(text, 120));
                    }
                } else {
                    if title.is_empty()
                        && announces(text)
                        && (before_edits || last_edit == usize::MAX)
                    {
                        title = imperative(&first_sentence(text, 120));
                    }
                    nearby.push(json!({"text": text, "event_id": event["id"], "after_edits": !before_edits}));
                }
            }
            "rationale" => rationale.push(json!({"text": event["text"], "event_id": event["id"]})),
            "summary" => nearby
                .push(json!({"text": event["text"], "event_id": event["id"], "secondary": true})),
            "change" if arr(&event["code_edits"]).is_empty() => {
                for file in arr(&event["files"]) {
                    gaps.push(format!(
                        "{} changed without captured content (shell or unsupported tool)",
                        s(file)
                    ));
                }
            }
            "tool_call" | "test" | "search" | "read" if writes_files(s(&event["text"])) => {
                gaps.push(format!(
                    "shell command may have changed files without captured content: {}",
                    security::short(&shell_command(s(&event["text"])), 120).replace('\n', " ")
                ));
            }
            _ => {}
        }
        if let Some(kind) = validation(event) {
            checked.push(json!({"kind": kind,
                "command": security::short(&validation_command(&shell_command(s(&event["text"]))), 160),
                "outcome": outcome(session, event), "event_id": event["id"]}));
        }
    }
    let files: Vec<String> = d.files.iter().cloned().collect();
    if title.is_empty() {
        let first_words = d
            .events
            .iter()
            .find(|e| {
                e["kind"] == "assistant"
                    && history::provenance::original(e)
                    && !s(&e["text"]).trim().is_empty()
            })
            .map(|e| imperative(&first_sentence(s(&e["text"]), 90)))
            .filter(|t| !t.is_empty());
        title = match (files.is_empty(), first_words) {
            (true, Some(words)) => format!("Discuss: {words}"),
            (true, None) => "Planning and notes".into(),
            (false, _) => format!(
                "Edit {}",
                security::short(
                    &files
                        .iter()
                        .map(|f| f.rsplit('/').next().unwrap_or(f))
                        .collect::<Vec<_>>()
                        .join(", "),
                    70
                )
            ),
        };
    }
    for i in &d.edit_indices {
        let edit = &edits[*i];
        if edit["truncated"] == true {
            gaps.push(format!("{} captured edit truncated", s(&edit["file"])));
        }
        if edit["state"] == "recorded" {
            gaps.push(format!(
                "{} edit recorded, execution not confirmed",
                s(&edit["file"])
            ));
        }
    }
    if !d.edit_indices.is_empty() && checked.is_empty() {
        gaps.push("No build or test run recorded for this step".into());
    }
    gaps.sort();
    gaps.dedup();
    // A step starts with the agent's first act, not with the request that preceded it.
    let times: Vec<&str> = d
        .events
        .iter()
        .filter(|e| e["kind"] != "user")
        .filter_map(|e| e["timestamp"].as_str())
        .collect();
    let request = requests
        .get(d.turn)
        .map(|r| security::short(s(&r["text"]), 200))
        .unwrap_or_default();
    json!({
        "ordinal": ordinal, "title": title, "turn": d.turn, "request": request,
        "started": times.first().copied(), "ended": times.last().copied(),
        "edit_indices": d.edit_indices, "files": files,
        "why": {
            "status": if !recorded.is_empty() { "recorded" } else if !nearby.is_empty() || !rationale.is_empty() { "nearby-only" } else { "none" },
            "recorded": recorded, "rationale": rationale, "nearby": nearby
        },
        "checked": checked, "gaps": gaps,
        "event_ids": d.events.iter().map(|e| e["id"].clone()).collect::<Vec<_>>(),
        "session_key": work["session"]["storage_key"], "agent": agent
    })
}

/// Which steps produced a hunk, via its attributed edit indices.
pub fn for_hunk(steps: &[Value], hunk: &Value) -> Vec<usize> {
    let indices: Vec<usize> = arr(&hunk["edit_indices"]).iter().map(n).collect();
    steps
        .iter()
        .filter(|st| {
            arr(&st["edit_indices"])
                .iter()
                .any(|i| indices.contains(&n(i)))
        })
        .map(|st| n(&st["ordinal"]))
        .collect()
}

/// Hunks a step produced.
pub fn hunks_for(step: &Value, hunks: &[Value]) -> Vec<String> {
    let indices: Vec<usize> = arr(&step["edit_indices"]).iter().map(n).collect();
    hunks
        .iter()
        .filter(|h| {
            arr(&h["edit_indices"])
                .iter()
                .any(|i| indices.contains(&n(i)))
        })
        .map(|h| s(&h["id"]).to_owned())
        .collect()
}

pub fn status_label(step: &Value) -> &'static str {
    match s(&step["why"]["status"]) {
        "recorded" => "recorded",
        "nearby-only" => "nearby only",
        _ => "no recorded reason",
    }
}

/// Steps from several evidence sessions, merged in time order. Each session's
/// edits sit at `offset` in the shared edit list, so hunk ↔ step links hold.
pub fn merged(evidence: &[(Value, Value, usize)]) -> Vec<Value> {
    let mut all = vec![];
    for (session, work, offset) in evidence {
        let label = format!(
            "{} · {}",
            s(&session["agent"]),
            security::short(s(&work["session"]["title"]), 30).replace('\n', " ")
        );
        for mut st in steps(session, work) {
            st["edit_indices"] = json!(
                arr(&st["edit_indices"])
                    .iter()
                    .map(|i| n(i) + offset)
                    .collect::<Vec<_>>()
            );
            st["session_label"] = json!(label);
            all.push(st);
        }
    }
    all.sort_by(|a, b| {
        let key = |st: &Value| {
            chrono::DateTime::parse_from_rfc3339(s(&st["started"]))
                .ok()
                .map(|d| d.timestamp_millis())
        };
        key(a).cmp(&key(b))
    });
    for (i, st) in all.iter_mut().enumerate() {
        st["ordinal"] = json!(i);
    }
    all
}
