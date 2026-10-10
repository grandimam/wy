//! A flat, request-scoped conversation with optional supporting disclosures.
use super::{
    document::{Document, Link, View},
    history_views::Page,
    theme::*,
};
use crate::{arr, history, insights, n, s};
use ratatui::prelude::*;
use serde_json::{Value, json};
use std::sync::Arc;

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
        return vec![];
    }
    let mut lines = vec![];
    if kind == "notes" {
        lines.push(Line::styled(
            "Captured notes · tentative; may include ideas not used",
            Style::default().fg(AMBER),
        ));
    }
    if kind == "summary" {
        lines.push(Line::styled(
            "Secondary context · not an original response",
            Style::default().fg(AMBER),
        ));
    }
    for e in selected {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        if kind == "response" && !history::provenance::original(e) {
            lines.push(Line::styled(
                "Unverified response",
                Style::default().fg(AMBER),
            ));
        }
        lines.extend(
            s(&e["text"])
                .lines()
                .map(|line| Line::styled(line.to_owned(), Style::default().fg(TEXT))),
        );
        if e["truncated"] == true {
            lines.push(Line::styled(
                "Partial excerpt · captured text may be incomplete",
                Style::default().fg(AMBER),
            ));
        }
    }
    lines
}
fn request(doc: &mut Document, turn: &Value) {
    let r = &turn["request"];
    let original =
        r["kind"] == "user" && history::provenance::original(r) && !s(&r["text"]).trim().is_empty();
    doc.heading("YOU");
    doc.text(
        if original {
            s(&r["text"])
        } else {
            "Original request unavailable"
        },
        TEXT,
    );
    if r["truncated"] == true {
        doc.text("[Request capture shortened]", AMBER);
    }
}
fn group(doc: &mut Document, work: &Value, index: usize) {
    let turn = &work["turns"][index];
    let id = format!("session-turn-{index}-0");
    request(doc, turn);
    let messages = arr(&turn["messages"]);
    let response = message_lines(messages, "response");
    if !response.is_empty() {
        doc.heading("AGENT");
        doc.lines.extend(response);
    }
    doc.gap();

    let indices = arr(&turn["edit_indices"]);
    let mut lines = vec![];
    let mut links = vec![];
    let mut unconfirmed = 0;
    let mut partial = 0;
    for index in indices {
        let Some(edit) = arr(&work["edits"]).get(n(index)) else {
            continue;
        };
        links.push((lines.len(), Link::SessionEdit(history::edit_ref(edit))));
        lines.push(Line::styled(
            format!("  {} · {} ›", s(&edit["file"]), s(&edit["operation"])),
            Style::default().fg(ACCENT),
        ));
        unconfirmed += usize::from(edit["state"] != "applied");
        partial += usize::from(edit["truncated"] == true);
    }
    if !lines.is_empty() {
        let count = lines.len();
        let changes = format!("{id}-changes");
        doc.linked_disclosure(
            changes.clone(),
            &format!(
                "Changes · {count} edit{}",
                if count == 1 { "" } else { "s" }
            ),
            lines,
            links,
        );
        if count <= 3 {
            doc.toggle_disclosure(&changes);
        }
        // Edit execution uncertainty remains next to the changes it qualifies.
        if unconfirmed > 0 {
            doc.text(
                format!("Execution unconfirmed for {unconfirmed} of {count} edits"),
                AMBER,
            );
        }
        if partial > 0 {
            doc.text(
                format!("Partial capture for {partial} of {count} edits"),
                AMBER,
            );
        }
    }
    let notes = message_lines(messages, "notes");
    if !notes.is_empty() {
        doc.disclosure(format!("{id}-notes"), "Agent notes", notes);
    }

    let prior = &turn["prior_request"];
    if prior["kind"] == "user"
        && history::provenance::original(prior)
        && !s(&prior["text"]).trim().is_empty()
    {
        let mut lines = vec![Line::styled(
            "Earlier request in this session · context only",
            Style::default().fg(MUTED),
        )];
        lines.extend(
            s(&prior["text"])
                .lines()
                .map(|line| Line::styled(line.to_owned(), Style::default().fg(TEXT))),
        );
        if prior["truncated"] == true {
            lines.push(Line::styled(
                "[Earlier request capture shortened]",
                Style::default().fg(AMBER),
            ));
        }
        doc.disclosure(format!("{id}-earlier"), "Earlier request", lines);
    }
    if !arr(&turn["activity"]).is_empty() {
        let mut lines = vec![];
        for event in arr(&turn["activity"]) {
            let (label, color, _) = super::document::conversation_role(event);
            lines.push(Line::styled(label, Style::default().fg(color)));
            lines.extend(super::document::event_body(event, usize::MAX));
            lines.push(Line::default());
        }
        doc.disclosure(format!("{id}-activity"), "Tool activity", lines);
    }
    let summaries = message_lines(messages, "summary");
    if !summaries.is_empty() {
        doc.disclosure(format!("{id}-summary"), "Session summary", summaries);
    }
}

pub(super) fn flow(work: Arc<Value>, selected: usize) -> Document {
    let mut doc = Document::new(View::SessionWork, "Detail");
    if work["session"].is_null() {
        doc.text("No captured session", AMBER);
        link(&mut doc, "Choose a session", Link::SessionPicker);
        return doc;
    }
    let turns = arr(&work["turns"]);
    let selected = selected.min(crate::session_reader::count(&work).saturating_sub(1));
    let local = if work["indexed_reader"] == true {
        0
    } else {
        selected
    };
    let metadata = format!(
        "{} · {}",
        s(&work["session"]["agent"]),
        insights::local_date(&work["session"]["last_event"])
    );
    if let Some(turn) = turns.get(local) {
        doc.title = format!("Detail · {} · {metadata}", request_label(turn, selected));
        doc.text(
            format!(
                "Request {} of {}",
                selected + 1,
                crate::session_reader::count(&work)
            ),
            MUTED,
        );
        link(
            &mut doc,
            "Explain this request’s decisions",
            Link::RequestDecisions(selected),
        );
        group(&mut doc, &work, local);
    } else {
        doc.title = format!("Detail · {metadata}");
        doc.text("No captured requests or supported edits", AMBER);
    }
    let warnings: Vec<_> = arr(&work["warnings"])
        .iter()
        .filter(|w| !s(w).trim().is_empty())
        .flat_map(|w| {
            s(w).lines()
                .map(|line| Line::styled(line.to_owned(), Style::default().fg(AMBER)))
        })
        .collect();
    if !warnings.is_empty() {
        doc.gap();
        doc.disclosure(
            "session-capture-warning".into(),
            "⚠ Capture warning",
            warnings,
        );
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
