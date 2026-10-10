//! Decisions lead; code evidence and original context stay within the deep dive.
use super::{
    document::{Document, Link, View},
    theme::*,
};
use crate::{arr, s};
use ratatui::prelude::*;
use serde_json::Value;
use std::sync::Arc;

fn link(doc: &mut Document, label: impl AsRef<str>, link: Link) {
    doc.sources.push((doc.lines.len(), link));
    doc.text(label, ACCENT);
}
fn provenance(d: &Value) -> &'static str {
    match s(&d["status"]) {
        "recorded" => "Recorded",
        "inferred" => "Inferred",
        _ => "Unknown rationale",
    }
}

pub(super) fn overview(brief: Arc<Value>) -> Document {
    let mut doc = Document::new(View::DecisionOverview, "Decisions");
    let session_scope = brief["scope_kind"] == "session";
    doc.text(
        if session_scope {
            brief["session"]["title"]
                .as_str()
                .unwrap_or("No captured session")
        } else {
            "Uncommitted changes"
        },
        MUTED,
    );
    let decisions = arr(&brief["decisions"]);
    if brief["changed_files"] == 0 {
        doc.heading(if session_scope {
            "No captured code edits"
        } else {
            "No uncommitted changes"
        });
    } else if decisions.is_empty() {
        doc.heading("No decisions identified");
        doc.text(
            if brief["mode"] == "assessment" {
                "No consequential choices were identified in the available evidence."
            } else {
                "No recorded decisions found."
            },
            MUTED,
        );
    }
    for (i, d) in decisions.iter().enumerate() {
        doc.gap();
        link(
            &mut doc,
            format!("{}. {}", i + 1, s(&d["choice"])),
            Link::Decision(i),
        );
        doc.text(provenance(d), AMBER);
        doc.text(crate::security::short(s(&d["reason"]), 280), TEXT);
        if let Some(t) = arr(&d["tradeoffs"]).first() {
            doc.text(
                format!("Trade-off: {}", crate::security::short(s(t), 180)),
                TEXT,
            );
        }
    }
    if if session_scope {
        brief["captured_edits"].as_u64().unwrap_or(0) > 0
    } else {
        brief["changed_files"] != 0
    } {
        doc.gap();
        link(
            &mut doc,
            if brief["mode"] == "assessment" {
                "Refresh decisions with AI"
            } else {
                "Identify decisions with AI"
            },
            Link::DiscoverDecisions,
        );
        doc.text(
            "Sends selected code and history to your reasoning CLI.",
            MUTED,
        );
    }
    if session_scope {
        doc.gap();
        link(&mut doc, "Choose a session", Link::SessionPicker);
    }
    let mut details = if session_scope {
        vec![
            Line::from(format!(
                "Session: {} · {}",
                s(&brief["session"]["agent"]),
                s(&brief["session"]["id"])
            )),
            Line::from(format!("{} captured edits", brief["captured_edits"])),
        ]
    } else {
        vec![
            Line::from(format!(
                "Base: {}",
                brief["comparison_base"].as_str().unwrap_or("empty tree")
            )),
            Line::from(format!("{} changed files", brief["changed_files"])),
        ]
    };
    details.push(Line::from(if brief["mode"] == "assessment" {
        "AI-selected, ordered by assessed consequence. References are checked, not semantic accuracy."
    } else { "Historical records associated by file; applicability unverified. Ordered by affected-file count." }));
    for gap in arr(&brief["unknowns"]) {
        details.push(Line::from(s(gap).to_owned()));
    }
    if brief["mode"] == "assessment" {
        if !s(&brief["packet"]["limitations"]).is_empty() {
            details.push(Line::from(s(&brief["packet"]["limitations"]).to_owned()));
        }
        let omitted = brief["packet"]["omitted_items"].as_u64().unwrap_or(0);
        if omitted > 0 {
            details.push(Line::from(format!(
                "{omitted} evidence items omitted by the packet budget"
            )));
        }
        let covered = arr(&brief["packet"]["evidence"])
            .iter()
            .filter(|e| {
                e["kind"]
                    == if session_scope {
                        "session_edit"
                    } else {
                        "diff"
                    }
            })
            .map(|e| s(&e["file"]))
            .collect::<std::collections::HashSet<_>>()
            .len();
        if Some(covered as u64) != brief["changed_files"].as_u64() {
            doc.gap();
            doc.text(
                format!(
                    "Partial evidence: {covered}/{} files",
                    brief["changed_files"]
                ),
                AMBER,
            );
        } else if omitted > 0
            || arr(&brief["packet"]["evidence"])
                .iter()
                .any(|e| e["truncated"] == true)
        {
            doc.gap();
            doc.text("Some evidence excerpts are partial or omitted", AMBER);
        }
    }
    doc.gap();
    doc.disclosure("decision-scope".into(), "Details", details);
    doc.source_selection = (!doc.sources.is_empty()).then_some(0);
    doc.artifact = Some(brief);
    doc
}

pub(super) fn detail(brief: Arc<Value>, index: usize) -> Option<Document> {
    let d = arr(&brief["decisions"]).get(index)?;
    let mut doc = Document::new(View::DecisionDetail, s(&d["choice"]));
    doc.text(provenance(d), AMBER);
    if brief["mode"] == "recorded" && brief["scope_kind"] != "session" {
        doc.text("Historical record · applicability unverified", AMBER);
    }
    doc.heading("Why");
    doc.text(s(&d["reason"]), TEXT);
    if brief["mode"] == "assessment" && !s(&d["significance"]).is_empty() {
        doc.heading("Why it matters");
        doc.text(s(&d["significance"]), TEXT);
    }
    for (key, title) in [
        ("alternatives", "Alternatives"),
        ("tradeoffs", "Trade-offs"),
    ] {
        doc.heading(title);
        if arr(&d[key]).is_empty() {
            doc.text("Not established", AMBER);
        }
        for text in arr(&d[key]) {
            doc.text(format!("• {}", s(text)), TEXT);
        }
    }
    let ids = arr(&d["evidence_ids"]);
    let evidence = arr(&brief["packet"]["evidence"]);
    doc.heading("Evidence");
    for file in arr(&d["files"]) {
        if !evidence
            .iter()
            .any(|e| ids.contains(&e["id"]) && e["file"] == *file && e["kind"] != "session")
        {
            doc.text(format!("No captured code edit for {}", s(file)), AMBER);
        }
    }
    for (i, e) in evidence
        .iter()
        .enumerate()
        .filter(|(_, e)| ids.contains(&e["id"]) && e["kind"] != "session")
    {
        link(
            &mut doc,
            format!(
                "{}{}",
                s(&e["file"]),
                if e["truncated"] == true {
                    " (truncated)"
                } else {
                    ""
                }
            ),
            Link::Source(i),
        );
    }
    doc.heading("Original context");
    for (i, e) in evidence
        .iter()
        .enumerate()
        .filter(|(_, e)| ids.contains(&e["id"]) && e["kind"] == "session")
    {
        link(
            &mut doc,
            format!(
                "{} · {}",
                s(&e["agent"]),
                crate::history::provenance::label(e)
            ),
            Link::Source(i),
        );
    }
    if !evidence
        .iter()
        .any(|e| ids.contains(&e["id"]) && e["kind"] == "session")
    {
        doc.text("No original context cited", MUTED);
    }
    let mut details = vec![Line::from(if brief["mode"] == "assessment" {
        "Alternatives and trade-offs are assessments unless explicitly attributed to a recorded statement."
    } else {
        "Records are self-reported, not independently verified."
    })];
    if !s(&d["quote"]).is_empty() {
        details.push(Line::from(format!(
            "Recorded justification: {}",
            s(&d["quote"])
        )));
    }
    for text in arr(&d["recorded_evidence"]) {
        details.push(Line::from(format!("Unverified reference: {}", s(text))));
    }
    doc.gap();
    doc.disclosure("decision-details".into(), "Details", details);
    doc.gap();
    link(&mut doc, "← All decisions", Link::DecisionHome);
    if index + 1 < arr(&brief["decisions"]).len() {
        link(&mut doc, "Next decision →", Link::Decision(index + 1));
    }
    doc.artifact = Some(brief);
    Some(doc)
}
