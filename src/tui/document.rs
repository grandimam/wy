use super::{Target, theme::*};
use crate::{arr, history::attribution, n, presentation, s};
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
    Commits,
    Commit,
    Original,
    Turn,
    Help,
}
/// What a clickable or selectable line opens.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Link {
    /// A numbered source of the current document.
    Source(usize),
    /// The conversation turn that recorded an edit.
    Turn(Value),
    /// Scroll the reader to a line (a reason's first change, or back to the reason).
    Line(usize),
}
#[derive(Clone)]
pub(super) struct Document {
    pub kind: View,
    pub title: String,
    pub lines: Vec<Line<'static>>,
    pub target: Option<Target>,
    pub artifact: Option<Arc<Value>>,
    pub session_edit: Option<Value>,
    pub sources: Vec<(usize, Link)>,
    pub source_selection: Option<usize>,
    pub scroll: u16,
    pub horizontal: u16,
    pub notice: Option<(String, Color)>,
    pub commits: Vec<String>,
    pub originals: Vec<Value>,
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
            commits: vec![],
            originals: vec![],
        }
    }
    pub fn code(&self) -> bool {
        matches!(self.kind, View::Diff | View::SessionCode)
    }
    pub fn historical(&self) -> bool {
        matches!(self.kind, View::Commits | View::Commit | View::Original | View::Turn)
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
    /// Speaker line: role, then the agent name as a tag.
    pub fn speaker_line(&mut self, who: &str, agent: &str) {
        let mut spans = vec![Span::styled(
            who.to_owned(),
            Style::default().fg(TEXT).bold(),
        )];
        if !agent.is_empty() {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(agent.to_owned(), Style::default().fg(MUTED)));
        }
        self.lines.push(Line::from(spans));
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
                Style::default().fg(GREEN)
            } else if line.starts_with('-') {
                Style::default().fg(RED)
            } else if line.starts_with("@@") {
                Style::default().fg(ACCENT)
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
        // File headers repeat the title; the reader starts at the first hunk.
        let skipped = diff_header(s(&change["diff"]));
        for line in s(&change["diff"]).lines().skip(skipped) {
            let style = if line.starts_with('+') {
                Style::default().fg(GREEN)
            } else if line.starts_with('-') {
                Style::default().fg(RED)
            } else if line.starts_with("@@") {
                Style::default().fg(ACCENT)
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
                doc.scroll = index.saturating_sub(skipped).min(u16::MAX as usize) as u16;
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
/// The reader: this file's changes in order, each reason placed just above the
/// hunks it explains. `brief` collapses every reason to its headline.
pub(super) fn recorded(
    review: &Value,
    sessions: &[Value],
    code: &Document,
    brief: bool,
) -> Document {
    let target = code.target.clone().expect("code has a target");
    let notes = crate::history::notes(
        review,
        sessions,
        &target.file,
        code.session_edit.as_ref(),
    );
    let focus = target.symbol.as_ref().map(|symbol| serde_json::json!({"target":target.selector(),"file":target.file,"symbol":symbol,"start_line":target.line}));
    let artifact = Arc::new(
        serde_json::json!({"context":"recorded_session","review_id":review["id"],
        "packet":{"focus_file":target.file,"focus_target":focus,"focus_session_edit":code.session_edit,
            "evidence":notes["evidence"],"note_refs":notes["note_refs"],"gaps":notes["gaps"]}}),
    );
    let mut doc = Document::new(View::Recorded, target.label());
    doc.target = Some(target.clone());
    doc.session_edit = code.session_edit.clone();
    let root = std::path::Path::new(s(&review["root"]));
    let diff = change_diff(review, &target.file);
    if diff.is_none() {
        doc.notice = Some((
            "No current Git diff · showing the recorded session edit, which may differ from the file now".into(),
            AMBER,
        ));
    }
    let mut found = attribution::reasons(root, sessions, &target.file, diff);
    // A recorded edit kept from an earlier review may no longer have its session loaded.
    if diff.is_none() && arr(&found["hunks"]).is_empty() {
        if let Some(edit) = recent_edit(review, &target.file) {
            let (added, removed) = attribution::edit_size(edit);
            found["hunks"] = serde_json::json!([{"id":edit["id"],"label":"recorded edit","text":edit["text"],
                "format":edit["format"],"added":added,"removed":removed,"status":"recorded","reason":null,"first":false}]);
        }
    }
    let reasons = arr(&found["reasons"]);
    let hunks = arr(&found["hunks"]);
    // Without a matching edit, fall back to conversation that mentions this file.
    let evidence = arr(&notes["evidence"]);
    if reasons.is_empty() && !evidence.is_empty() {
        doc.heading("Related conversation");
        doc.text("Matched by file mentions, not by a recorded edit.", MUTED);
        doc.gap();
        related(&mut doc, evidence);
        doc.heading("Changes");
        doc.gap();
    }
    if reasons.is_empty() {
        for gap in arr(&notes["gaps"]) {
            doc.text(s(gap), AMBER);
            doc.gap();
        }
    }
    let mut first_lines = vec![0; reasons.len()];
    for hunk in hunks {
        if let Some(index) = hunk["reason"].as_u64().map(|i| i as usize) {
            if hunk["first"] == true {
                first_lines[index] = doc.lines.len();
                why(&mut doc, index + 1, &reasons[index], brief);
            } else {
                doc.sources.push((doc.lines.len(), Link::Line(first_lines[index])));
                doc.lines.push(Line::from(vec![
                    Span::styled("↑ ", Style::default().fg(ACCENT)),
                    badge(index + 1),
                    Span::styled(" same reason as above", Style::default().fg(MUTED)),
                ]));
            }
        }
        change(&mut doc, hunk, reasons.get(hunk["reason"].as_u64().unwrap_or(u64::MAX) as usize));
    }
    if hunks.is_empty() {
        doc.text("No changes recorded for this file.", MUTED);
    }
    doc.artifact = Some(artifact);
    doc
}
/// The model that wrote a message, falling back to the agent CLI name.
fn who_wrote<'a>(event: &'a Value, agent: &'a str) -> &'a str {
    event["model"].as_str().filter(|m| !m.is_empty()).unwrap_or(agent)
}
fn change_diff<'a>(review: &'a Value, file: &str) -> Option<&'a str> {
    arr(&review["changes"])
        .iter()
        .find(|c| c["file"] == file)
        .and_then(|c| c["diff"].as_str())
}
/// The reason number as a solid badge, so it stands out from code and prose.
fn badge(number: usize) -> Span<'static> {
    Span::styled(
        format!(" {number} "),
        Style::default()
            .fg(ACCENT)
            .add_modifier(Modifier::REVERSED | Modifier::BOLD),
    )
}
/// One reason block: a numbered badge and the headline, then (unless brief)
/// the rest of its message and the request that led to it.
fn why(doc: &mut Document, number: usize, reason: &Value, brief: bool) {
    let message = &reason["message"];
    let agent = reason["model"].as_str().unwrap_or(s(&reason["agent"]));
    let text = s(&message["text"]);
    let (title, rest) = if text.is_empty() {
        ("No message before this change".to_owned(), String::new())
    } else {
        attribution::headline(text)
    };
    let bar = Span::styled("┃ ", Style::default().fg(ACCENT));
    doc.lines.push(Line::from(vec![
        badge(number),
        Span::raw(" "),
        Span::styled(title, Style::default().fg(ACCENT).bold()),
        Span::styled(format!("  {agent}"), Style::default().fg(MUTED)),
    ]));
    if message.is_object() && !crate::history::provenance::original(message) {
        doc.lines.push(Line::from(vec![
            bar.clone(),
            Span::styled(crate::history::provenance::label(message), Style::default().fg(AMBER)),
        ]));
    }
    if !brief {
        if !rest.is_empty() {
            for line in crate::security::short(&rest, 900).lines() {
                doc.lines.push(Line::from(vec![
                    bar.clone(),
                    Span::styled(line.to_owned(), Style::default().fg(TEXT)),
                ]));
            }
        }
        if let Some(request) = reason["request"]["text"].as_str() {
            doc.lines.push(Line::from(vec![
                bar.clone(),
                Span::styled(
                    format!("You asked: “{}”", crate::security::short(request.trim(), 240)),
                    Style::default().fg(MUTED),
                ),
            ]));
        }
    }
    doc.gap();
}
/// The latest request and up to three agent messages that mention the file.
fn related(doc: &mut Document, evidence: &[Value]) {
    let last_user = evidence.iter().rposition(|e| e["role"] == "user");
    let assistants: Vec<_> = evidence
        .iter()
        .enumerate()
        .filter(|(_, e)| e["role"] == "assistant")
        .map(|(i, _)| i)
        .collect();
    let recent = &assistants[assistants.len().saturating_sub(3)..];
    for (i, event) in evidence.iter().enumerate() {
        if Some(i) != last_user && !recent.contains(&i) && event["role"] != "summary" {
            continue;
        }
        let original = crate::history::provenance::original(event);
        let user = event["role"] == "user";
        let who = if !original { "Captured context" } else if user { "You" } else { "Agent" };
        doc.speaker_line(who, if user { "" } else { who_wrote(event, s(&event["agent"])) });
        if !original {
            doc.text(crate::history::provenance::label(event), AMBER);
        }
        if let Some(status) = crate::history::origins::status(event) {
            doc.text(status, AMBER);
        }
        let full = s(&event["text"]);
        let excerpt = crate::security::short(full, if user { 500 } else { 1400 });
        let more = if excerpt.len() < full.len() { "…" } else { "" };
        doc.text(format!("“{excerpt}{more}”"), if user { MUTED } else { TEXT });
        doc.gap();
    }
}
/// One hunk: `@@ line 10 · fn get()  +2 −1`, a status when needed, then its lines.
/// The header opens the turn that made the change when a recorded edit matched.
fn change(doc: &mut Document, hunk: &Value, reason: Option<&Value>) {
    let mut spans = vec![
        Span::styled("@@ ", Style::default().fg(ACCENT)),
        Span::styled(s(&hunk["label"]).to_owned(), Style::default().fg(ACCENT).bold()),
        Span::styled(
            format!("  +{} −{}", n(&hunk["added"]), n(&hunk["removed"])),
            Style::default().fg(MUTED),
        ),
    ];
    match s(&hunk["status"]) {
        "partial" => spans.push(Span::styled("  · edited after", Style::default().fg(AMBER))),
        "none" => spans.push(Span::styled("  · no recorded reason", Style::default().fg(MUTED))),
        _ => {}
    }
    if let Some(edit) = reason.map(|r| &r["edit"]).filter(|e| e.is_object()) {
        spans.push(Span::styled("  turn ›", Style::default().fg(MUTED)));
        doc.sources.push((doc.lines.len(), Link::Turn(edit.clone())));
    }
    doc.lines.push(Line::from(spans));
    for line in s(&hunk["text"]).lines().skip(usize::from(hunk["format"] == "patch")) {
        let style = if line.starts_with('+') || hunk["format"] == "code" {
            Style::default().fg(GREEN)
        } else if line.starts_with('-') {
            Style::default().fg(RED)
        } else {
            Style::default().fg(TEXT)
        };
        doc.lines.push(Line::styled(line.replace('\t', "    "), style));
    }
    doc.gap();
}
/// Lines before the first hunk (`diff --git`, `---`, `+++`).
fn diff_header(diff: &str) -> usize {
    diff.lines().take_while(|l| !l.starts_with("@@")).count()
}
/// The conversation around one recorded edit, with the edit itself in place.
pub(super) fn turn(session: &Value, edit: &Value) -> Document {
    let mut doc = Document::new(View::Turn, format!("Turn · {}", s(&edit["file"])));
    doc.notice = Some(("Recorded conversation · Esc goes back".into(), MUTED));
    for event in attribution::turn(session, &edit["event_id"]) {
        if event["id"] == edit["event_id"] {
            let (added, removed) = attribution::edit_size(edit);
            doc.lines.push(Line::from(vec![
                Span::styled("✎ Edit ", Style::default().fg(ACCENT).bold()),
                Span::styled(s(&edit["file"]).to_owned(), Style::default().fg(TEXT)),
                Span::styled(format!("  +{added} −{removed}"), Style::default().fg(MUTED)),
            ]));
            for line in s(&edit["text"]).lines() {
                let style = if line.starts_with('+') || edit["format"] == "code" {
                    Style::default().fg(GREEN)
                } else if line.starts_with('-') {
                    Style::default().fg(RED)
                } else {
                    Style::default().fg(MUTED)
                };
                doc.lines.push(Line::styled(line.replace('\t', "    "), style));
            }
            doc.gap();
            continue;
        }
        let original = crate::history::provenance::original(&event);
        let user = event["kind"] == "user";
        let who = if !original { "Captured context" } else if user { "You" } else { "Agent" };
        doc.speaker_line(who, if user { "" } else { who_wrote(&event, s(&session["agent"])) });
        if !original {
            doc.text(crate::history::provenance::label(&event), AMBER);
        }
        doc.text(
            crate::security::short(s(&event["text"]), 4000),
            if user { MUTED } else { TEXT },
        );
        doc.gap();
    }
    if doc.lines.is_empty() {
        doc.text("This turn is no longer in the saved conversation.", AMBER);
    }
    doc.target = Some(Target { file: s(&edit["file"]).into(), symbol: None, line: 1 });
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
    let mut checked = (*artifact).clone();
    crate::history::provenance::sanitize_artifact(&mut checked);
    let artifact = Arc::new(checked);
    let target = artifact_target(&artifact);
    let title = target
        .as_ref()
        .map(Target::label)
        .unwrap_or_else(|| "All changes".into());
    let mut doc = Document::new(View::Explanation, title);
    if let Some(warning) = artifact["provenance_warning"].as_str() {
        doc.text(warning, AMBER);
        doc.gap();
    }
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
        doc.sources.push((doc.lines.len(), Link::Source(i)));
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
    if e["kind"] == "session" {
        provenance(&mut doc, e, true);
    }
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
    for key in ["agent", "model", "role", "session_id", "event_id"] {
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
fn provenance(doc: &mut Document, evidence: &Value, links: bool) {
    doc.text(
        crate::history::provenance::label(evidence),
        if crate::history::provenance::original(evidence) {
            MUTED
        } else {
            AMBER
        },
    );
    if let Some(status) = crate::history::origins::status(evidence) {
        doc.text(status, AMBER);
    }
    if links {
        for reference in arr(&evidence["originals"]) {
            let index = doc.originals.len();
            doc.originals.push(reference.clone());
            doc.sources.push((doc.lines.len(), Link::Source(index)));
            let relation = if reference["relation"] == "retained_context" {
                "retained original context"
            } else {
                "original turn"
            };
            doc.text(
                format!(
                    "[{}] Open {relation} · {} · {}",
                    index + 1,
                    s(&reference["session_id"]),
                    s(&reference["turn_id"])
                ),
                ACCENT,
            );
        }
    }
}
pub(super) fn original(evidence: &Value) -> Document {
    let mut doc = Document::new(View::Original, "Captured original message");
    doc.notice = Some(("Original source · Esc returns to the summary".into(), MUTED));
    provenance(&mut doc, evidence, false);
    doc.text(
        format!(
            "{}:{} · {} · {}",
            s(&evidence["agent"]),
            s(&evidence["session_id"]),
            s(&evidence["provenance"]["turn_id"]),
            s(&evidence["provenance"]["message_id"])
        ),
        MUTED,
    );
    doc.text(
        format!("{}:{}", s(&evidence["file"]), n(&evidence["start_line"])),
        MUTED,
    );
    doc.gap();
    doc.text(s(&evidence["text"]), TEXT);
    if evidence["truncated"] == true {
        doc.text(
            "Original excerpt shortened by the capture limit; surrounding context may be missing.",
            AMBER,
        );
    }
    doc
}
pub(super) fn commits(entries: &[Value]) -> Document {
    let mut doc = Document::new(View::Commits, "Recent commits");
    doc.notice = Some((
        "↑/↓ select · Enter opens saved conversations · /commit HASH · Esc back".into(),
        MUTED,
    ));
    if entries.is_empty() {
        doc.text(
            "No commits yet. Review your changes before making the first commit.",
            MUTED,
        );
    }
    for (index, entry) in entries.iter().enumerate() {
        doc.sources.push((doc.lines.len(), Link::Source(index)));
        doc.commits.push(s(&entry["commit"]).into());
        doc.text(
            format!("{}  {}", s(&entry["short"]), s(&entry["subject"])),
            TEXT,
        );
    }
    if !entries.is_empty() {
        doc.source_selection = Some(0);
    }
    doc
}

pub(super) fn commit_context(context: &Value) -> Document {
    let hash = s(&context["commit"]);
    let mut doc = Document::new(
        View::Commit,
        format!("Commit {}", hash.chars().take(12).collect::<String>()),
    );
    doc.notice = Some((
        "Saved conversations · local lookup · Esc back · g browse commits".into(),
        MUTED,
    ));
    doc.text(hash, MUTED);
    for link in arr(&context["links"]) {
        let basis = if link["association"] == "snapshot-match" {
            "Matched review base and source snapshot"
        } else {
            "Explicitly linked review"
        };
        doc.text(format!("{basis} · {}", s(&link["review_id"])), ACCENT);
    }
    for (index, session) in arr(&context["sessions"]).iter().enumerate() {
        doc.heading(format!(
            "Capture {} · {}:{}",
            index + 1,
            s(&session["agent"]),
            s(&session["id"])
        ));
        doc.text(
            format!(
                "Saved with {}",
                arr(&session["review_ids"])
                    .iter()
                    .map(s)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            MUTED,
        );
        for event in arr(&session["events"]) {
            doc.heading(format!("{} · {}", s(&event["kind"]), s(&event["id"])));
            provenance(&mut doc, event, true);
            doc.text(
                format!("{}:{}", s(&session["path"]), n(&event["source_line"])),
                MUTED,
            );
            doc.text(s(&event["text"]), TEXT);
        }
        if arr(&session["events"]).is_empty() {
            doc.text("No observable events in this saved capture.", MUTED);
        }
    }
    doc
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
g  browse commits · /commit HASH  read saved conversations
Drag pane dividers or [ / ] to move the focused pane's divider
/layout reset  restore default pane sizes
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
/commits
/commit HASH
/link HASH [REVIEW-ID] — explicitly attach a saved review
/cancel",
        ),
        (
            "About the evidence",
            "Agent notes and sources are local: no model call. Enrich uses your selected agent and may use your account's allowance. Recorded statements show what the agent said; inferred reasons are a new assessment.",
        ),
    ] {
        doc.heading(title);
        doc.text(body, TEXT);
    }
    doc
}
