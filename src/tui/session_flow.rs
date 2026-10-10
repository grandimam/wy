//! Request-led session reader: only requests have borders; replies, notes and
//! changes remain subordinate disclosures with links bound to captured edits.
use super::{
    document::{Document, Link, View},
    history_views::Page,
    theme::*,
};
use crate::{arr, history, insights, n, s, security};
use ratatui::prelude::*;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Copy)]
struct Part {
    turn: usize,
    start: usize,
    end: usize,
}
fn pages(turns: &[Value]) -> Vec<Vec<Part>> {
    let mut pages = vec![];
    let mut page = vec![];
    let mut used = 0;
    for (turn, t) in turns.iter().enumerate() {
        let count = arr(&t["edit_indices"]).len();
        for start in (0..count.max(1)).step_by(20) {
            let end = (start + 20).min(count);
            let cost = (end - start).max(1);
            if !page.is_empty() && (used + cost > 20 || page.len() >= 8) {
                pages.push(page);
                page = vec![];
                used = 0;
            }
            page.push(Part { turn, start, end });
            used += cost;
        }
    }
    if !page.is_empty() {
        pages.push(page);
    }
    if pages.is_empty() {
        pages.push(vec![]);
    }
    pages
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
    let mut remaining = 32000;
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
        if remaining == 0 {
            lines.push(Line::styled(
                "    More context is available in Open conversation.",
                Style::default().fg(AMBER),
            ));
            break;
        }
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
        let text = security::short(raw, remaining);
        remaining = remaining.saturating_sub(raw.chars().count());
        lines.extend(
            text.lines()
                .map(|line| Line::styled(format!("    {line}"), Style::default().fg(TEXT))),
        );
        if text != raw || e["truncated"] == true {
            lines.push(Line::styled(
                "    Partial excerpt · open the conversation for captured context",
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
    let excerpt = security::short(text, 16000);
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
        let excerpt = security::short(s(&prior["text"]), 16000);
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

pub(super) fn flow(work: Arc<Value>, page: usize) -> Document {
    let mut doc = Document::new(
        View::SessionWork,
        work["session"]["title"].as_str().unwrap_or("Session work"),
    );
    if work["session"].is_null() {
        doc.text("No captured session", AMBER);
        link(&mut doc, "Choose a session", Link::SessionPicker);
        return doc;
    }
    doc.sources.push((doc.lines.len(), Link::SessionPicker));
    doc.lines.push(Line::from(vec![
        Span::styled(
            format!(
                "{} · {}",
                s(&work["session"]["agent"]),
                insights::local_date(&work["session"]["last_event"])
            ),
            Style::default().fg(MUTED),
        ),
        Span::styled("    Change session", Style::default().fg(ACCENT)),
    ]));
    let pages = pages(arr(&work["turns"]));
    let page = page.min(pages.len() - 1);
    if arr(&work["turns"]).is_empty() {
        doc.gap();
        doc.text("No captured requests or supported edits", AMBER);
    }
    for part in &pages[page] {
        group(&mut doc, &work, *part);
    }
    if pages.len() > 1 {
        doc.gap();
        doc.text(format!("Page {} of {}", page + 1, pages.len()), MUTED);
        if page > 0 {
            link(&mut doc, "← Previous", Link::Page(page - 1));
        }
        if page + 1 < pages.len() {
            link(&mut doc, "Next →", Link::Page(page + 1));
        }
    }
    doc.heading("Original context");
    link(
        &mut doc,
        "Open conversation",
        Link::SessionChat(s(&work["session"]["storage_key"]).into()),
    );
    let mut details = vec![Line::from(
        "Recorded edit order, not a reconstructed runtime call graph. Shell, manual and unsupported edits may be missing; known failed edits are excluded. No current files were substituted.",
    )];
    for warning in arr(&work["warnings"]) {
        details.push(Line::from(s(warning).to_owned()));
    }
    doc.disclosure("session-coverage".into(), "Capture details", details);
    doc.pagination = Some(Page::Work {
        work: work.clone(),
        index: page,
    });
    doc.artifact = Some(Arc::new(
        json!({"context":"session_work","session":work["session"]}),
    ));
    doc
}
