//! Request-led session reader: only requests have borders; replies, notes and
//! changes remain subordinate disclosures with links bound to captured edits.
use super::{
    document::{Document, Link, View},
    history_views::Page,
    theme::*,
};
use crate::{arr, history, insights, n, s};
use ratatui::prelude::*;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Copy)]
struct Part {
    turn: usize,
    start: usize,
    end: usize,
}
pub(super) const REQUESTS_PER_PAGE: usize = crate::session_reader::PAGE_SIZE;

pub(super) fn request_label(turn: &Value, index: usize) -> String {
    let id = s(&turn["request"]["id"]).trim();
    if id.is_empty() {
        format!("Request {} · ID unavailable", index + 1)
    } else {
        id.to_owned()
    }
}
fn link(doc: &mut Document, text: impl AsRef<str>, action: Link) {
    doc.sources.push((doc.lines.len(), action));
    doc.text(text, ACCENT);
}
fn summary(e: &Value) -> bool {
    e["kind"] == "summary" || e["provenance"]["source_type"] == "compaction_summary"
}
fn message_lines(messages: &[Value], kind: &str) -> Vec<Line<'static>> {
    let selected: Vec<_> = messages
        .iter()
        .filter(|e| !s(&e["text"]).trim().is_empty())
        .filter(|e| match kind {
            "response" => e["kind"] == "assistant" && !summary(e),
            "notes" => e["kind"] == "rationale",
            _ => summary(e),
        })
        .collect();
    if selected.is_empty() {
        return vec![Line::styled(
            match kind {
                "notes" => "    No readable agent notes captured for this request.",
                "summary" => "    No readable summary captured.",
                _ => "    No agent response captured for this request.",
            },
            Style::default().fg(MUTED),
        )];
    }
    let mut lines = vec![];
    if kind == "notes" {
        lines.push(Line::styled(
            "    Captured notes · tentative; may include ideas not used",
            Style::default().fg(AMBER),
        ));
    }
    if kind == "summary" {
        lines.push(Line::styled(
            "    Secondary context · not an original response",
            Style::default().fg(AMBER),
        ));
    }
    for e in selected {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        if kind == "response" && !history::provenance::original(e) {
            lines.push(Line::styled(
                "    Unverified response",
                Style::default().fg(AMBER),
            ));
        }
        let raw = s(&e["text"]);
        lines.extend(
            raw.lines()
                .map(|line| Line::styled(format!("    {line}"), Style::default().fg(TEXT))),
        );
        if e["truncated"] == true {
            lines.push(Line::styled(
                "    Partial excerpt · captured text may be incomplete",
                Style::default().fg(AMBER),
            ));
        }
    }
    lines
}
fn request(doc: &mut Document, part: Part, turn: &Value) {
    let r = &turn["request"];
    let original =
        r["kind"] == "user" && history::provenance::original(r) && !s(&r["text"]).trim().is_empty();
    let text = if original {
        s(&r["text"])
    } else {
        "Original request unavailable"
    };
    let excerpt = text;
    let mut body = vec![Line::default()];
    body.extend(
        excerpt
            .lines()
            .map(|line| Line::styled(line.to_owned(), Style::default().fg(TEXT))),
    );
    if excerpt != text || r["truncated"] == true {
        body.push(Line::styled(
            "[Request excerpt shortened]",
            Style::default().fg(AMBER),
        ));
    }
    body.extend([Line::default(), Line::default()]);
    let id = format!("reason-session-{}-{}-request", part.turn, part.start);
    doc.disclosure(
        id.clone(),
        if part.start > 0 {
            "Your request · continued"
        } else {
            "Your request"
        },
        body,
    );
    doc.toggle_disclosure(&id);
}
fn group(doc: &mut Document, work: &Value, part: Part) {
    let turn = &work["turns"][part.turn];
    let id = format!("session-turn-{}-{}", part.turn, part.start);
    doc.gap();
    request(doc, part, turn);
    doc.gap();
    let prior = &turn["prior_request"];
    if prior["kind"] == "user" && history::provenance::original(prior) {
        let mut lines = vec![Line::styled(
            "    Earlier request in this session · context only",
            Style::default().fg(MUTED),
        )];
        let excerpt = s(&prior["text"]);
        lines.extend(
            excerpt
                .lines()
                .map(|l| Line::styled(format!("    {l}"), Style::default().fg(TEXT))),
        );
        if excerpt != s(&prior["text"]) || prior["truncated"] == true {
            lines.push(Line::styled(
                "    [Earlier request excerpt shortened]",
                Style::default().fg(AMBER),
            ));
        }
        doc.disclosure(format!("{id}-earlier"), "  Earlier request", lines);
    }
    let messages = arr(&turn["messages"]);
    doc.disclosure(
        format!("{id}-response"),
        "  Agent response",
        message_lines(messages, "response"),
    );
    doc.disclosure(
        format!("{id}-notes"),
        "  Agent notes",
        message_lines(messages, "notes"),
    );
    if !arr(&turn["activity"]).is_empty() {
        let mut lines = vec![];
        for event in arr(&turn["activity"]) {
            let (label, color, _) = super::document::conversation_role(event);
            lines.push(Line::styled(format!("    {label}"), Style::default().fg(color)));
            lines.extend(super::document::event_body(event, usize::MAX));
            lines.push(Line::default());
        }
        doc.disclosure(format!("{id}-activity"), "  Tool activity", lines);
    }
    if messages.iter().any(summary) {
        doc.disclosure(
            format!("{id}-summary"),
            "  Session summary",
            message_lines(messages, "summary"),
        );
    }
    doc.gap();
    let indices = arr(&turn["edit_indices"]);
    let visible = &indices[part.start..part.end];
    let title = if visible.len() == indices.len() {
        format!(
            "  Changes · {} edit{}",
            visible.len(),
            if visible.len() == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "  Changes · {}–{} of {} edits",
            part.start + 1,
            part.end,
            indices.len()
        )
    };
    let mut lines = vec![];
    let mut links = vec![];
    let mut unconfirmed = 0;
    let mut partial = 0;
    for index in visible {
        let Some(edit) = arr(&work["edits"]).get(n(index)) else {
            continue;
        };
        links.push((lines.len(), Link::SessionEdit(history::edit_ref(edit))));
        lines.push(Line::styled(
            format!("    {} · {} ›", s(&edit["file"]), s(&edit["operation"])),
            Style::default().fg(ACCENT),
        ));
        unconfirmed += usize::from(edit["state"] != "applied");
        partial += usize::from(edit["truncated"] == true);
    }
    if lines.is_empty() {
        doc.text("  No supported code edits captured", MUTED);
    } else {
        let changes = format!("{id}-changes");
        doc.linked_disclosure(changes.clone(), &title, lines, links);
        if visible.len() <= 3 {
            doc.toggle_disclosure(&changes);
        }
    }
    if unconfirmed > 0 {
        doc.text(
            format!(
                "  Execution unconfirmed for {unconfirmed} of {} edits",
                visible.len()
            ),
            AMBER,
        );
    }
    if partial > 0 {
        doc.text(
            format!("  Partial capture for {partial} of {} edits", visible.len()),
            AMBER,
        );
    }
}

pub(super) fn flow(work: Arc<Value>, selected: usize) -> Document {
    let mut doc = Document::new(View::SessionWork, "Detail");
    if work["session"].is_null() {
        doc.text("No captured session", AMBER);
        link(&mut doc, "Choose a session", Link::SessionPicker);
        return doc;
    }
    doc.lines.push(Line::from(vec![
        Span::styled(
            format!(
                "{} · {}",
                s(&work["session"]["agent"]),
                insights::local_date(&work["session"]["last_event"])
            ),
            Style::default().fg(MUTED),
        ),

    ]));
    for warning in arr(&work["warnings"]) {
        doc.text(s(warning), AMBER);
    }
    let turns = arr(&work["turns"]);
    let selected = selected.min(crate::session_reader::count(&work).saturating_sub(1));
    let local = if work["indexed_reader"] == true { 0 } else { selected };
    if let Some(turn) = turns.get(local) {
        doc.text(format!("Request · {}", request_label(turn, selected)), MUTED);
        group(&mut doc, &work, Part { turn: local, start: 0, end: arr(&turn["edit_indices"]).len() });
    } else {
        doc.gap();
        doc.text("No captured requests or supported edits", AMBER);
    }
    doc.pagination = Some(Page::Work {
        work: work.clone(),
        index: selected,
    });
    doc.artifact = Some(Arc::new(
        json!({"context":"session_work","session":work["session"]}),
    ));
    doc
}
