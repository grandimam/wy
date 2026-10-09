use super::{Target, theme::*};
use crate::{arr, n, presentation, s};
use ratatui::prelude::*;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum View {
    Empty,
    Diff,
    Explanation,
    Evidence,
    Help,
}
impl View {
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "Changes",
            Self::Diff => "Diff",
            Self::Explanation => "Why this change?",
            Self::Evidence => "Evidence",
            Self::Help => "Help",
        }
    }
}
#[derive(Clone)]
pub(super) struct Document {
    pub kind: View,
    pub title: String,
    pub lines: Vec<Line<'static>>,
    pub target: Option<Target>,
    pub artifact: Option<Arc<Value>>,
    pub scroll: u16,
    pub horizontal: u16,
    pub notice: Option<(String, Color)>,
}
impl Document {
    pub fn new(kind: View, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            lines: vec![],
            target: None,
            artifact: None,
            scroll: 0,
            horizontal: 0,
            notice: None,
        }
    }
    pub fn code(&self) -> bool {
        self.kind == View::Diff
    }
    pub fn text(&mut self, text: impl AsRef<str>, color: Color) {
        self.lines.extend(
            text.as_ref()
                .lines()
                .map(|l| Line::styled(l.to_owned(), Style::default().fg(color))),
        );
    }
    pub fn gap(&mut self) {
        self.lines.push(Line::default());
    }
    pub fn heading(&mut self, text: impl Into<String>) {
        if self.lines.last().is_some_and(|line| !line.spans.is_empty()) {
            self.gap();
        }
        self.lines.push(Line::styled(
            text.into(),
            Style::default().fg(ACCENT).bold(),
        ));
    }
}

pub(super) fn totals(review: &Value) -> (usize, usize) {
    arr(&review["changes"]).iter().fold((0, 0), |(a, r), c| {
        (
            a + arr(&c["added_lines"]).len(),
            r + n(&c["removed_line_count"]),
        )
    })
}
pub(super) fn empty(review: &Value) -> Document {
    let mut doc = Document::new(View::Empty, "Changes");
    if arr(&review["changes"]).is_empty() {
        doc.heading("No changed files");
        doc.text("After your agent edits code, press r to refresh.", MUTED);
    } else {
        doc.heading("Select a changed file");
        doc.text("Browse its diff, then choose Why this change?", MUTED);
    }
    doc
}

pub(super) fn diff(review: &Value, target: Target) -> Document {
    let mut doc = Document::new(View::Diff, target.label());
    if let Some(change) = arr(&review["changes"])
        .iter()
        .find(|c| c["file"] == target.file)
    {
        for line in s(&change["diff"]).lines() {
            let style = if line.starts_with("---") || line.starts_with("+++") {
                Style::default().fg(MUTED)
            } else if line.starts_with('+') {
                Style::default().fg(GREEN).bg(ADD_BG)
            } else if line.starts_with('-') {
                Style::default().fg(RED).bg(REMOVE_BG)
            } else if line.starts_with("@@") {
                Style::default().fg(ACCENT).bg(PANEL)
            } else {
                Style::default().fg(TEXT)
            };
            doc.lines
                .push(Line::styled(line.replace('\t', "    "), style));
        }
        if target.symbol.is_some() {
            let end = arr(&change["symbols"])
                .iter()
                .find(|symbol| n(&symbol["start_line"]) == target.line)
                .map(|symbol| n(&symbol["end_line"]))
                .unwrap_or(target.line);
            if let Some(index) = s(&change["diff"]).lines().position(|line| {
                if !line.starts_with("@@ ") {
                    return false;
                }
                let Some(range) = line
                    .split_whitespace()
                    .nth(2)
                    .and_then(|r| r.strip_prefix('+'))
                else {
                    return false;
                };
                let (start, count) = range.split_once(',').unwrap_or((range, "1"));
                let start = start.parse::<usize>().unwrap_or(0);
                let count = count.parse::<usize>().unwrap_or(1);
                start <= end && start.saturating_add(count.max(1) - 1) >= target.line
            }) {
                doc.scroll = index.min(u16::MAX as usize) as u16;
            }
        }
        if change["diff_truncated"] == true {
            doc.text("Diff truncated at the review's capture limit.", AMBER);
        }
    } else {
        doc.text(
            "No captured diff for this file. Press r to refresh changed files.",
            MUTED,
        );
    }
    doc.target = Some(target);
    doc
}
pub(super) fn artifact_target(artifact: &Value) -> Option<Target> {
    let focus = &artifact["packet"]["focus_target"];
    let selector = s(&focus["target"]).rsplit_once(':');
    let file = focus["file"]
        .as_str()
        .or_else(|| selector.map(|(f, _)| f))
        .or_else(|| artifact["packet"]["focus_file"].as_str())?;
    Some(Target {
        file: file.into(),
        symbol: focus["symbol"]
            .as_str()
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
        line: n(&focus["start_line"]).max(1),
    })
}
pub(super) fn explanation(artifact: Arc<Value>) -> Document {
    let target = artifact_target(&artifact);
    let title = target
        .as_ref()
        .map(Target::label)
        .unwrap_or_else(|| "All changes".into());
    let mut doc = Document::new(View::Explanation, title);
    doc.target = target;
    let cited = presentation::citations(&artifact);
    let result = &artifact["explanation"];
    let judgments = arr(&result["judgments"]);
    let recorded: Vec<_> = judgments
        .iter()
        .filter(|j| j["status"] == "recorded")
        .collect();
    doc.heading("Agent's recorded reason");
    if recorded.is_empty() {
        let has_history = arr(&artifact["packet"]["evidence"])
            .iter()
            .any(|e| e["kind"] == "session");
        doc.text(
            if has_history {
                "No explicit reason was found in the captured conversation."
            } else {
                "No agent conversation was available for this answer."
            },
            AMBER,
        );
        doc.text(
            "The explanation below is an interpretation of the available evidence.",
            MUTED,
        );
    } else {
        for judgment in recorded {
            doc.text(s(&judgment["choice"]), TEXT);
            doc.text(format!("“{}”", s(&judgment["quote"])), GREEN);
            judgment_claim(&mut doc, judgment, &cited);
        }
    }
    if !s(&result["answer"]["text"]).is_empty() {
        doc.heading("Answer");
        claim(&mut doc, &result["answer"], &cited);
    }
    if !s(&result["problem"]["text"]).is_empty() {
        doc.heading("The request or constraint");
        claim(&mut doc, &result["problem"], &cited);
    }
    let inferred: Vec<_> = judgments
        .iter()
        .filter(|j| j["status"] != "recorded")
        .collect();
    if !inferred.is_empty() {
        doc.heading("What may explain the approach");
    }
    for judgment in inferred {
        doc.text(s(&judgment["choice"]), TEXT);
        judgment_claim(&mut doc, judgment, &cited);
    }
    if !arr(&result["steps"]).is_empty() {
        doc.heading("How this connects to the code");
    }
    for step in arr(&result["steps"]) {
        claim(&mut doc, step, &cited);
    }
    if !arr(&result["tradeoffs"]).is_empty() {
        doc.heading("Tradeoffs");
    }
    for tradeoff in arr(&result["tradeoffs"]) {
        claim(&mut doc, tradeoff, &cited);
    }
    if !arr(&result["unknowns"]).is_empty() {
        doc.heading("Still unclear");
    }
    for unknown in arr(&result["unknowns"]) {
        let mut text = s(unknown).to_owned();
        for evidence in arr(&artifact["packet"]["evidence"]) {
            let label = cited
                .iter()
                .position(|e| e["id"] == evidence["id"])
                .map(|i| format!("ref {}", i + 1))
                .unwrap_or_else(|| s(&evidence["file"]).into());
            let id = s(&evidence["id"]);
            if !id.is_empty() {
                text = text.replace(id, &label);
            }
        }
        doc.text(format!("• {text}"), AMBER);
    }
    doc.heading("Sources · press a reference number to open");
    for (i, evidence) in cited.iter().enumerate() {
        let label = if evidence["kind"] == "session" {
            format!(
                "{} · {} conversation",
                s(&evidence["agent"]),
                s(&evidence["role"])
            )
        } else {
            format!("{}:{}", s(&evidence["file"]), n(&evidence["start_line"]))
        };
        doc.text(format!("[{}] {label}", i + 1), MUTED);
    }
    if cited.len() > 9 {
        doc.text("/evidence NUMBER opens any reference.", MUTED);
    }
    if !arr(&artifact["stale_files"]).is_empty() || artifact["stale_head"] == true {
        doc.gap();
        doc.text(
            "Code changed while this answer was generated. R updates the answer.",
            AMBER,
        );
    }
    let omitted = n(&artifact["packet"]["omitted_items"]);
    let truncated = arr(&artifact["packet"]["evidence"])
        .iter()
        .filter(|e| e["truncated"] == true)
        .count();
    if omitted > 0 || truncated > 0 {
        doc.text(
            format!("Captured context: {omitted} excerpts omitted, {truncated} shortened."),
            MUTED,
        );
    }
    doc.artifact = Some(artifact);
    doc
}
fn judgment_claim(doc: &mut Document, judgment: &Value, cited: &[Value]) {
    let claim_value = serde_json::json!({"text":judgment["reason"], "basis":judgment["status"], "evidence_ids":judgment["evidence_ids"]});
    claim(doc, &claim_value, cited);
}
fn claim(doc: &mut Document, claim: &Value, cited: &[Value]) {
    let (label, color) = match s(&claim["basis"]) {
        "observed" => ("From evidence", ACCENT),
        "recorded" => ("Recorded in the conversation", GREEN),
        "assessment" => ("Interpretation", VIOLET),
        "inferred" => ("Inferred · not an agent statement", AMBER),
        "proposed" => ("Suggestion", VIOLET),
        _ => ("Not established", AMBER),
    };
    let refs = arr(&claim["evidence_ids"])
        .iter()
        .filter_map(|id| cited.iter().position(|e| e["id"] == *id))
        .map(|i| format!(" [{}]", i + 1))
        .collect::<String>();
    doc.text(s(&claim["text"]), TEXT);
    doc.text(format!("{label}{refs}"), color);
    doc.gap();
}
pub(super) fn evidence(artifact: Arc<Value>, index: usize) -> Option<Document> {
    let cited = presentation::citations(&artifact);
    let e = cited.get(index)?;
    let mut doc = Document::new(View::Evidence, format!("[{}] {}", index + 1, s(&e["file"])));
    doc.heading(format!("CAPTURED {}", s(&e["kind"]).to_uppercase()));
    doc.text(
        format!(
            "{}:{} · {}",
            s(&e["file"]),
            n(&e["start_line"]),
            s(&e["id"])
        ),
        MUTED,
    );
    doc.text(
        "Saved with this explanation. This excerpt may differ from current source.",
        MUTED,
    );
    for key in ["agent", "role", "session_id", "event_id"] {
        if let Some(value) = e[key].as_str() {
            doc.text(format!("{key}: {value}"), MUTED);
        }
    }
    doc.gap();
    let text = e["text"]
        .as_str()
        .or_else(|| e["excerpt"].as_str())
        .unwrap_or("No excerpt captured.");
    if e["kind"] == "code" {
        for (i, line) in text.lines().enumerate() {
            doc.text(
                format!("{:>5}  {line}", n(&e["start_line"]).max(1) + i),
                TEXT,
            );
        }
    } else {
        doc.text(text, TEXT);
    }
    doc.target = if ["code", "diff"].contains(&s(&e["kind"])) {
        Some(Target {
            file: s(&e["file"]).into(),
            symbol: e["symbol"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            line: n(&e["start_line"]).max(1),
        })
    } else {
        artifact_target(&artifact)
    };
    if e["truncated"] == true {
        doc.gap();
        doc.text(
            "This captured excerpt was truncated; surrounding context may be missing.",
            AMBER,
        );
    }
    doc.artifact = Some(artifact);
    Some(doc)
}
pub(super) fn help() -> Document {
    let mut doc = Document::new(View::Help, "Diff → Why this change? → Conversation");
    for (title, body) in [
        (
            "Start with a change",
            "Select a file or function in the tree to see its diff.
w  Why this change? — find what led to this implementation
The answer distinguishes the agent's recorded statements from inferences.",
        ),
        (
            "Follow the answer",
            "1–9  open the cited code or conversation
i  ask a follow-up about the answer
d  return to the diff · w  reopen the answer
R  request an updated answer · p  last saved answer
Esc  back / cancel a running request",
        ),
        (
            "Navigate",
            "↑/↓ or j/k  select files or scroll the focused pane
←/→ or h/l  expand the tree or pan a diff
Space  expand changed symbols · Enter  read the diff
Tab  switch panes · f  filter file paths
PageUp/PageDown  scroll · Home/End  start/end
r  refresh changes · b  toggle files · m  mark reviewed
q / Ctrl+Q / Ctrl+C  quit",
        ),
        (
            "Settings & commands",
            "/agent codex|claude
/source both|codex|claude|none
/why FILE:SYMBOL QUESTION
/ask QUESTION
/reason QUESTION — question about all changes
/evidence NUMBER
/cancel",
        ),
        (
            "About the evidence",
            "Why this change? uses your selected agent to read captured code and observable conversations. It may use your account's allowance. A recorded statement is evidence of what the agent said; inferred reasons are a new assessment. Unrecorded reasoning cannot be recovered from the diff.",
        ),
    ] {
        doc.heading(title);
        doc.text(body, TEXT);
    }
    doc
}
