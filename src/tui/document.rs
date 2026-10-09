use super::{Target, theme::*};
use crate::{arr, n, presentation, s};
use ratatui::prelude::*;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum View {
    Empty,
    Diff,
    SessionCode,
    Recorded,
    Explanation,
    Evidence,
    Help,
}
impl View {
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "Changes",
            Self::Diff | Self::SessionCode => "Changes",
            Self::Recorded => "Agent notes",
            Self::Explanation => "Enriched",
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
    pub session_edit: Option<Value>,
    pub sources: Vec<(usize, usize)>,
    pub source_selection: Option<usize>,
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
            session_edit: None,
            sources: vec![],
            source_selection: None,
            scroll: 0,
            horizontal: 0,
            notice: None,
        }
    }
    pub fn code(&self) -> bool {
        matches!(self.kind, View::Diff | View::SessionCode)
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
    if super::explorer::files(review).is_empty() {
        doc.heading("No changed files");
        doc.text("No working-tree diff or recent session code. After your agent edits code, press r to refresh.", MUTED);
    } else {
        doc.heading("Select a changed file");
        doc.text(
            "Its changes and recorded agent notes appear together.",
            MUTED,
        );
    }
    doc
}

pub(super) fn recent_edit<'a>(review: &'a Value, file: &str) -> Option<&'a Value> {
    arr(&review["recent_code"])
        .iter()
        .find(|c| c["file"] == file)
}
pub(super) fn preview(review: &Value, target: Target) -> Document {
    if !arr(&review["changes"])
        .iter()
        .any(|c| c["file"] == target.file)
    {
        if let Some(code) = session_code(review, target.clone()) {
            return code;
        }
    }
    diff(review, target)
}
pub(super) fn session_code(review: &Value, target: Target) -> Option<Document> {
    let edit = recent_edit(review, &target.file)?;
    Some(recorded_code(edit))
}
pub(super) fn recorded_code(edit: &Value) -> Document {
    let mut doc = Document::new(View::SessionCode, s(&edit["file"]));
    let date = edit["timestamp"]
        .as_str()
        .map(|t| t.replace('T', " "))
        .unwrap_or_else(|| "date unavailable".into());
    doc.text(format!("{} · {date}", s(&edit["agent"])), ACCENT);
    doc.text(
        format!(
            "Session {} · {}",
            s(&edit["session_id"]),
            s(&edit["event_id"])
        ),
        MUTED,
    );
    doc.text(
        if edit["state"] == "applied" {
            "Tool reported success."
        } else {
            "Recorded tool input · execution not confirmed."
        },
        MUTED,
    );
    if edit["format"] == "patch" {
        doc.text(
            "Patch excerpt · line numbers may be relative to the edit.",
            MUTED,
        );
    }
    doc.gap();
    if s(&edit["text"]).is_empty() {
        doc.text(
            "This session recorded a file change without a code excerpt.",
            AMBER,
        );
    }
    for (i, line) in s(&edit["text"]).lines().enumerate() {
        if edit["format"] == "code" {
            doc.text(
                format!("{:>5}  {}", i + 1, line.replace('\t', "    ")),
                TEXT,
            );
        } else {
            let style = if line.starts_with('+') {
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
    }
    if edit["truncated"] == true {
        doc.text("Recorded code truncated at the capture limit.", AMBER);
    }
    doc.notice = Some((
        "Recorded session edit · may differ from current files".into(),
        AMBER,
    ));
    doc.target = Some(Target {
        file: s(&edit["file"]).into(),
        symbol: None,
        line: 1,
    });
    doc.session_edit = Some(crate::history::edit_ref(edit));
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
    doc.notice = Some(("Current working-tree changes".into(), MUTED));
    doc
}
pub(super) fn is_recorded(artifact: &Value) -> bool {
    artifact["context"] == "recorded_session"
}
pub(super) fn citations(artifact: &Value) -> Vec<Value> {
    if is_recorded(artifact) {
        arr(&artifact["packet"]["evidence"]).to_vec()
    } else {
        presentation::citations(artifact)
    }
}
pub(super) fn recorded(review: &Value, sessions: &[Value], code: &Document) -> Document {
    let target = code.target.clone().expect("code has a target");
    let notes = crate::history::notes(
        review,
        sessions,
        &target.file,
        target.symbol.as_deref(),
        code.session_edit.as_ref(),
    );
    let focus = target.symbol.as_ref().map(|symbol| serde_json::json!({"target":target.selector(),"file":target.file,"symbol":symbol,"start_line":target.line}));
    let artifact = Arc::new(
        serde_json::json!({"context":"recorded_session","review_id":review["id"],
        "packet":{"focus_file":target.file,"focus_target":focus,"focus_session_edit":code.session_edit,
            "evidence":notes["evidence"],"note_refs":notes["note_refs"],"gaps":notes["gaps"]}}),
    );
    let mut doc = Document::new(View::Recorded, target.label());
    doc.target = Some(target);
    doc.session_edit = code.session_edit.clone();
    doc.text("Saved conversation · no model call", MUTED);
    doc.gap();
    let evidence = arr(&notes["evidence"]);
    let last_user = evidence.iter().rposition(|e| e["role"] == "user");
    let assistants: Vec<_> = evidence
        .iter()
        .enumerate()
        .filter(|(_, e)| e["role"] == "assistant")
        .map(|(i, _)| i)
        .collect();
    let start = assistants.len().saturating_sub(3);
    for (i, event) in evidence.iter().enumerate() {
        if Some(i) != last_user && !assistants[start..].contains(&i) {
            continue;
        }
        let full = s(&event["text"]);
        let excerpt =
            crate::security::short(full, if event["role"] == "user" { 500 } else { 1400 });
        let who = if event["role"] == "user" {
            "You asked".into()
        } else {
            format!("{} wrote", s(&event["agent"]))
        };
        doc.text(
            format!(
                "{who}: “{excerpt}{}” [{}]",
                if excerpt.len() < full.len() {
                    "…"
                } else {
                    ""
                },
                i + 1
            ),
            if event["role"] == "user" { MUTED } else { TEXT },
        );
        doc.gap();
    }
    if !evidence.is_empty() {
        doc.text("These excerpts are linked conversation context; they may not cover every current edit.", MUTED);
        doc.gap();
    }
    for gap in arr(&notes["gaps"]) {
        doc.text(format!("? {}", s(gap)), AMBER);
        doc.gap();
    }
    if evidence.is_empty() {
        doc.text("Enrich can assess the code and identify assumptions, while keeping inferred reasons explicit.", MUTED);
    }
    if !evidence.is_empty() {
        doc.heading("Sources · click or press s");
        for (i, event) in evidence.iter().enumerate() {
            doc.sources.push((doc.lines.len(), i));
            doc.text(
                format!(
                    "[{}] {} · {} · {}",
                    i + 1,
                    s(&event["agent"]),
                    s(&event["role"]),
                    s(&event["timestamp"])
                ),
                ACCENT,
            );
        }
    }
    doc.artifact = Some(artifact);
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
    let cited = citations(&artifact);
    let result = &artifact["explanation"];
    if let Some(edit) = artifact["packet"]["focus_session_edit"].as_object() {
        doc.text(
            format!(
                "Recorded session edit · {} · {}",
                s(&edit["agent"]),
                s(&edit["timestamp"])
            ),
            MUTED,
        );
    }
    let judgments = arr(&result["judgments"]);
    let recorded: Vec<_> = judgments
        .iter()
        .filter(|j| j["status"] == "recorded")
        .collect();
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
            "This explanation is an interpretation of the available evidence.",
            MUTED,
        );
        doc.gap();
    } else {
        for judgment in recorded {
            let reference = cited
                .iter()
                .position(|e| e["id"] == judgment["quote_id"])
                .map(|i| format!(" [{}]", i + 1))
                .unwrap_or_default();
            doc.text(
                format!("The agent wrote: “{}”{reference}", s(&judgment["quote"])),
                GREEN,
            );
            doc.gap();
        }
    }
    claim(&mut doc, &result["answer"], &cited);
    for judgment in judgments.iter().filter(|j| j["status"] != "recorded") {
        let prefix = if judgment["status"] == "inferred" {
            "Inferred"
        } else {
            "Reason not established"
        };
        doc.text(
            format!(
                "{prefix}: {}{}",
                s(&judgment["reason"]),
                references(judgment, &cited)
            ),
            AMBER,
        );
        doc.gap();
    }
    for key in ["steps", "tradeoffs"] {
        for item in arr(&result[key]) {
            claim(&mut doc, item, &cited);
        }
    }
    for unknown in arr(&result["unknowns"]) {
        let mut text = s(unknown).to_owned();
        for evidence in arr(&artifact["packet"]["evidence"]) {
            let label = cited
                .iter()
                .position(|e| e["id"] == evidence["id"])
                .map(|i| format!("[{}]", i + 1))
                .unwrap_or_else(|| s(&evidence["file"]).into());
            let id = s(&evidence["id"]);
            if !id.is_empty() {
                text = text.replace(id, &label);
            }
        }
        doc.text(text, AMBER);
        doc.gap();
    }
    doc.heading("Sources · click or press s");
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
        doc.sources.push((doc.lines.len(), i));
        doc.text(format!("[{}] {label}", i + 1), ACCENT);
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
fn references(claim: &Value, cited: &[Value]) -> String {
    arr(&claim["evidence_ids"])
        .iter()
        .filter_map(|id| cited.iter().position(|e| e["id"] == *id))
        .map(|i| format!(" [{}]", i + 1))
        .collect()
}
fn claim(doc: &mut Document, claim: &Value, cited: &[Value]) {
    if s(&claim["text"]).is_empty() {
        return;
    }
    doc.text(
        format!("{}{}", s(&claim["text"]), references(claim, cited)),
        TEXT,
    );
    doc.gap();
}
pub(super) fn evidence(artifact: Arc<Value>, index: usize) -> Option<Document> {
    let cited = citations(&artifact);
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
    let mut doc = Document::new(View::Help, "Changes, agent notes and optional enrichment");
    for (title, body) in [
        (
            "Start with a change",
            "Select a file or function to see its changes and available agent notes immediately.
Changes shows the current Git diff, or a recorded edit when no current diff exists.
Notes quote captured conversation and flag questions left open in those excerpts.
e  Enrich explanation — ask the agent to connect the code and notes.",
        ),
        (
            "Follow the answer",
            "Click a source, or s then ↑/↓ and Enter to open it.
1–9  open a numbered source directly
i  ask a follow-up about the answer
d  focus Changes · o  original notes · v  saved enrichment
R  request updated enrichment · p  last saved answer
Keep browsing while enrichment runs. Files show working, queued or ready.
Return to a file to resume reading. Completed answers survive restarting wy.
Esc  back · x  cancel running and queued enrichments",
        ),
        (
            "Navigate",
            "↑/↓ or j/k  select files or scroll the focused pane
←/→ or h/l  expand the tree or pan a diff
Space  expand changed symbols · Enter  focus code
Tab  switch files, code and explanation · f  filter file paths
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
            "Agent notes and sources are local: no model call. Gap hints look for missing signals in the excerpts, not private reasoning. Enrich uses your selected agent and may use your account's allowance. Recorded statements show what the agent said; inferred reasons are a new assessment.",
        ),
    ] {
        doc.heading(title);
        doc.text(body, TEXT);
    }
    doc
}
