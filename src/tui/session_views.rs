//! Selected-session work first; raw conversations and today's code are drill-downs.
use super::{
    document::{Document, Link, View},
    theme::*,
};
use crate::{arr, insights, s, session_work};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

pub(super) const SESSIONS_PER_PAGE: usize = 10;

fn link(doc: &mut Document, text: impl AsRef<str>, link: Link) {
    doc.sources.push((doc.lines.len(), link));
    doc.text(text, ACCENT);
}
pub(super) fn picker(review: &Value, sessions: &[Value], selected: Option<&Value>) -> Document {
    let mut doc = Document::new(View::Sessions, "Sessions");
    if review["lazy_sessions"] == true {
        for reference in arr(&review["sessions"]) {
            let key = s(&reference["storage_key"]);
            let marker = if selected.is_some_and(|w| w["session"]["storage_key"] == key) { "Selected · " } else { "" };
            link(&mut doc, format!("{marker}{} · {}", s(&reference["agent"]), s(&reference["id"])), Link::Session(key.into()));
        }
        if doc.sources.is_empty() { doc.text("No discovered sessions. /coverage shows discovery issues.", AMBER); }
        doc.source_selection = (!doc.sources.is_empty()).then_some(0);
        return doc;
    }
    let latest = session_work::latest(sessions).map(session_work::snapshot_key);
    let mut ordered: Vec<_> = sessions.iter().collect();
    ordered.sort_by_key(|session| {
        std::cmp::Reverse(
            chrono::DateTime::parse_from_rfc3339(s(&insights::session_dates(session).1)).ok(),
        )
    });
    for session in ordered {
        let key = session_work::snapshot_key(session);
        if !arr(&review["sessions"])
            .iter()
            .any(|r| r["storage_key"] == key)
        {
            continue;
        }
        doc.gap();
        let marker = if selected.is_some_and(|w| w["session"]["storage_key"] == key) {
            "Selected · "
        } else {
            ""
        };
        link(
            &mut doc,
            format!("{marker}{}", session_work::title(session)),
            Link::Session(key.clone()),
        );
        doc.text(
            format!(
                "{} · {}{}",
                s(&session["agent"]),
                insights::local_date(&insights::session_dates(session).1),
                if latest.as_ref() == Some(&key)
                    && chrono::DateTime::parse_from_rfc3339(s(&insights::session_dates(session).1))
                        .is_ok()
                {
                    " · latest captured"
                } else {
                    ""
                }
            ),
            MUTED,
        );
    }
    if doc.sources.is_empty() {
        doc.text("No captured sessions. /coverage shows capture gaps.", AMBER);
    }
    doc.source_selection = (!doc.sources.is_empty()).then_some(0);
    doc
}

pub(super) use super::session_flow::flow;

pub(super) fn captured_code(doc: &mut Document, edit: &Value) {
    super::code_view::render(doc, s(&edit["text"]), s(&edit["format"]), true, usize::MAX);
    let blocks: Vec<_> = doc
        .sources
        .iter()
        .filter_map(|(_, l)| {
            if let Link::Disclosure(id) = l {
                id.starts_with("code-block-").then_some(id.clone())
            } else {
                None
            }
        })
        .collect();
    for id in blocks {
        if !doc.expanded.contains(&id) {
            doc.toggle_disclosure(&id);
        }
    }
}

pub(super) fn implementation(edit: &Value) -> Document {
    let mut doc = Document::new(View::SessionImplementation, s(&edit["file"]));
    doc.text("As implemented · captured edit", ACCENT);
    if edit["state"] != "applied" {
        doc.text("Recorded input · execution unconfirmed", AMBER);
    }
    if edit["truncated"] == true {
        doc.text("Partial capture", AMBER);
    }
    captured_code(&mut doc, edit);
    if s(&edit["text"]).is_empty() {
        doc.text("Code excerpt unavailable", AMBER);
    }
    doc.gap();
    link(
        &mut doc,
        "Compare with current code",
        Link::CompareSessionEdit(crate::history::edit_ref(edit)),
    );
    link(&mut doc, "Original context", Link::Turn(edit.clone()));
    doc.artifact = Some(Arc::new(
        json!({"context":"session_work","session":{"storage_key":edit["session_key"]}}),
    ));
    doc
}

pub(super) fn comparison(root: &Path, work: &Value, edit: &Value) -> Document {
    let comparison = session_work::compare(root, work, edit);
    let mut doc = Document::new(
        View::SessionComparison,
        format!("{} · comparison", s(&edit["file"])),
    );
    doc.text(s(&comparison["status"]), AMBER);
    doc.heading("As implemented · captured edit");
    captured_code(&mut doc, edit);
    doc.heading("Current code · read now");
    if let Some(text) = comparison["current"].as_str() {
        for (i, line) in text.lines().enumerate() {
            doc.code_line(format!("{:>5} │ ", i + 1), line, TEXT);
        }
    } else {
        doc.text("Unavailable", AMBER);
    }
    if comparison["truncated"] == true {
        doc.text("Current code excerpt truncated", AMBER);
    }
    doc.text(s(&comparison["note"]), MUTED);
    doc.artifact = Some(Arc::new(
        json!({"context":"session_work","session":work["session"]}),
    ));
    doc
}

pub(super) fn working_changes(review: &Value) -> Document {
    let mut doc = Document::new(
        View::SessionComparison,
        "Working-tree changes · attribution unknown",
    );
    doc.text(
        "These changes are not assigned to the selected session.",
        AMBER,
    );
    for change in arr(&review["changes"]).iter().take(40) {
        doc.heading(s(&change["file"]));
        super::code_view::render(&mut doc, s(&change["diff"]), "patch", false, 400);
        if change["diff_truncated"] == true {
            doc.text("Diff truncated at capture", AMBER);
        }
    }
    if arr(&review["changes"]).len() > 40 {
        doc.text("Only the first 40 files are shown", AMBER);
    }
    if arr(&review["changes"]).is_empty() {
        doc.text("No uncommitted changes", MUTED);
    }
    doc
}
