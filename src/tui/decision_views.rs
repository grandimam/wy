//! Decisions lead; code evidence and original context stay within the deep dive.
//!
//! Overview: understand the work in one screen. Detail: understand one choice
//! without opening the transcript. Capture metadata stays behind disclosures;
//! decision-specific gaps stay visible.
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
/// Rationale provenance: label and color. Recorded is the only status that
/// rests on an original statement; the others are honest about that.
fn provenance(d: &Value) -> (&'static str, Color) {
    match s(&d["status"]) {
        "recorded" => ("Recorded rationale", GREEN),
        "inferred" => ("Inferred rationale", AMBER),
        _ => ("Unknown rationale", MUTED),
    }
}
fn code_kind(session_scope: bool) -> &'static str {
    if session_scope {
        "session_edit"
    } else {
        "diff"
    }
}
fn is_session_scope(brief: &Value) -> bool {
    brief["scope_kind"] == "session"
}
fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub(super) enum BriefState {
    NotAnalyzed,
    Generating,
    Ready,
    Failed(String),
    Unavailable,
}

/// Files with cited code evidence in the packet versus files in scope.
fn coverage(brief: &Value) -> Option<(u64, u64)> {
    if brief["mode"] != "assessment" {
        return None;
    }
    let session_scope = is_session_scope(brief);
    let covered = arr(&brief["packet"]["evidence"])
        .iter()
        .filter(|e| e["kind"] == code_kind(session_scope))
        .map(|e| s(&e["file"]))
        .collect::<std::collections::HashSet<_>>()
        .len() as u64;
    let total = brief["changed_files"].as_u64()?;
    Some((covered, total))
}

pub(super) fn overview(brief: Arc<Value>, state: BriefState, agent: &str) -> Document {
    let mut doc = Document::new(View::DecisionOverview, "Decisions");
    let session_scope = is_session_scope(&brief);
    let decisions = arr(&brief["decisions"]);
    let assessed = brief["mode"] == "assessment";
    let scope_count = if session_scope {
        brief["captured_edits"].as_u64().unwrap_or(0)
    } else {
        brief["changed_files"].as_u64().unwrap_or(0)
    };
    let has_work = scope_count > 0;

    // Header: what work, how much of it, and where the brief stands.
    doc.text(
        if session_scope {
            brief["session"]["title"]
                .as_str()
                .unwrap_or("No captured session")
        } else {
            "Uncommitted changes"
        },
        TEXT,
    );
    let mut meta = vec![];
    if session_scope {
        meta.push(s(&brief["session"]["agent"]).to_owned());
        let ordinals = arr(&brief["selection"]["ordinals"]);
        if ordinals.is_empty() {
            meta.push("Scope: Entire session".into());
        } else {
            meta.push(format!(
                "Scope: Selected requests {}",
                ordinals
                    .iter()
                    .map(|i| (crate::n(i) + 1).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        meta.push(plural(scope_count, "captured edit", "captured edits"));
    } else {
        meta.push(plural(scope_count, "changed file", "changed files"));
    }
    doc.text(meta.join(" · "), MUTED);
    doc.gap();

    // Status line: one sentence, state-specific.
    match &state {
        BriefState::Generating => doc.text(
            "Generating decision brief… · x cancels · keep browsing",
            AMBER,
        ),
        BriefState::Failed(error) => {
            doc.text(format!("Generation failed: {error} · retry below"), AMBER)
        }
        BriefState::Unavailable => doc.text(
            if session_scope {
                "No captured code edits in this work. Current files will not be substituted."
            } else {
                "No uncommitted changes to assess."
            },
            AMBER,
        ),
        BriefState::Ready => doc.text(
            format!(
                "{} · identified by {}",
                plural(
                    decisions.len() as u64,
                    "consequential choice",
                    "consequential choices"
                ),
                brief["agent"].as_str().unwrap_or(agent)
            ),
            TEXT,
        ),
        BriefState::NotAnalyzed => {
            if decisions.is_empty() {
                doc.text("Not generated yet", MUTED);
            } else {
                doc.text(
                    format!(
                        "{} · brief not generated yet",
                        plural(
                            decisions.len() as u64,
                            "recorded decision",
                            "recorded decisions"
                        )
                    ),
                    TEXT,
                );
            }
        }
    }

    // Empty-list explanations.
    if has_work && decisions.is_empty() {
        doc.gap();
        if assessed {
            doc.text("No consequential choices identified", TEXT);
            // The model's own one-line reason, when it gave one.
            match arr(&brief["unknowns"]).first() {
                Some(why) => doc.text(s(why), MUTED),
                None => doc.text(
                    "The available evidence shows no choice affecting behavior, dependencies or interfaces.",
                    MUTED,
                ),
            }
        } else if !matches!(state, BriefState::Generating) {
            doc.text("No generated decision brief yet", TEXT);
            doc.text(
                "No structured decision records were captured. Generate a brief to identify consequential choices and separate recorded reasons from inferred ones.",
                MUTED,
            );
        }
    }

    // The list. Each entry: choice, provenance, one-line reason, one trade-off.
    for (i, d) in decisions.iter().enumerate() {
        doc.gap();
        link(
            &mut doc,
            format!("{}. {}", i + 1, s(&d["choice"])),
            Link::Decision(i),
        );
        let (label, color) = provenance(d);
        doc.text(format!("   {label}"), color);
        let reason = crate::security::short(s(&d["reason"]), 240);
        if d["status"] == "unknown" {
            doc.text("   No explanation was captured.", MUTED);
            if assessed && !reason.trim().is_empty() {
                doc.text(format!("   Possible rationale: {reason}"), MUTED);
            }
        } else {
            doc.text(format!("   {reason}"), TEXT);
        }
        if let Some(t) = arr(&d["tradeoffs"]).first() {
            doc.text(
                format!("   Trade-off: {}", crate::security::short(s(t), 160)),
                TEXT,
            );
        }
    }

    // Coverage: visible, not buried, because it changes how much to trust the list.
    let mut partial_note = None;
    if let Some((covered, total)) = coverage(&brief) {
        let omitted = brief["packet"]["omitted_items"].as_u64().unwrap_or(0);
        let truncated = arr(&brief["packet"]["evidence"])
            .iter()
            .any(|e| e["truncated"] == true);
        if covered < total {
            partial_note = Some(format!(
                "⚠ {covered} of {total} files had code evidence · choices in the rest may be missing"
            ));
        } else if omitted > 0 || truncated {
            partial_note =
                Some("⚠ Some evidence excerpts were partial or omitted by the size budget".into());
        }
    }
    if let Some(note) = &partial_note {
        doc.gap();
        doc.text(note, AMBER);
    }

    // Primary action.
    if !matches!(state, BriefState::Generating | BriefState::Unavailable) && has_work {
        doc.gap();
        link(
            &mut doc,
            if matches!(state, BriefState::Failed(_)) {
                "Retry decision brief generation"
            } else if assessed {
                "Regenerate decision brief"
            } else {
                "Generate decision brief"
            },
            Link::DiscoverDecisions,
        );
        if !assessed {
            doc.text(
                format!(
                    "Generate with: {agent} · sends this work's captured code and conversation. Nothing runs automatically."
                ),
                MUTED,
            );
        }
    }

    // Scope navigation.
    if session_scope {
        doc.gap();
        if brief["selection"]["kind"] == "requests" {
            link(&mut doc, "Use entire session", Link::SessionDecisions);
        } else {
            doc.text(
                "Narrow to specific requests: select a request in Sessions, or /decisions requests START-END",
                MUTED,
            );
        }
        link(&mut doc, "Choose other work", Link::SessionPicker);
    }

    // Capture metadata and general caveats, collapsed.
    let mut details = if session_scope {
        vec![Line::from(format!(
            "Session: {} · {}",
            s(&brief["session"]["agent"]),
            s(&brief["session"]["id"])
        ))]
    } else {
        vec![Line::from(format!(
            "Base: {}",
            brief["comparison_base"].as_str().unwrap_or("empty tree")
        ))]
    };
    details.push(Line::from(if assessed {
        "AI-selected, ordered by assessed consequence. References are checked, not semantic accuracy."
    } else {
        "Historical records associated by file; applicability unverified. Ordered by affected-file count."
    }));
    // Skip the first unknown when it was already shown as the empty-list reason.
    let skip = usize::from(assessed && decisions.is_empty() && has_work);
    for gap in arr(&brief["unknowns"]).iter().skip(skip) {
        details.push(Line::from(s(gap).to_owned()));
    }
    if assessed {
        if !s(&brief["packet"]["limitations"]).is_empty() {
            details.push(Line::from(s(&brief["packet"]["limitations"]).to_owned()));
        }
        let omitted = brief["packet"]["omitted_items"].as_u64().unwrap_or(0);
        if omitted > 0 {
            details.push(Line::from(format!(
                "{omitted} evidence items omitted by the packet budget"
            )));
        }
    }
    doc.gap();
    doc.disclosure("decision-scope".into(), "Capture details", details);
    doc.source_selection = (!doc.sources.is_empty()).then_some(0);
    doc.artifact = Some(brief);
    doc
}

/// One-line description of a captured edit for the evidence list.
fn edit_summary(e: &Value) -> String {
    let mut parts = vec![];
    let op = s(&e["operation"]);
    if !op.is_empty() {
        parts.push(op.to_owned());
    }
    if e["state"] == "recorded" {
        parts.push("execution unconfirmed".into());
    }
    if e["truncated"] == true {
        parts.push("truncated".into());
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" · {}", parts.join(" · "))
    }
}

pub(super) fn detail(brief: Arc<Value>, index: usize) -> Option<Document> {
    let d = arr(&brief["decisions"]).get(index)?;
    let assessed = brief["mode"] == "assessment";
    let session_scope = is_session_scope(&brief);
    let ids = arr(&d["evidence_ids"]);
    let evidence = arr(&brief["packet"]["evidence"]);
    let status = s(&d["status"]);
    let mut doc = Document::new(View::DecisionDetail, s(&d["choice"]));
    let (label, color) = provenance(d);
    doc.text(label, color);
    if !assessed && !session_scope {
        doc.text("Historical record · applicability unverified", AMBER);
    }

    // Why: for recorded rationale, lead with the actual statement, not the paraphrase.
    doc.heading("Why");
    match status {
        "recorded" => {
            let quote = s(&d["quote"]);
            if !quote.is_empty() {
                doc.text(format!("“{}”", quote.trim()), TEXT);
                let source = evidence.iter().position(|e| e["id"] == d["quote_id"]);
                match source {
                    Some(i) => link(
                        &mut doc,
                        format!("— {} · open original statement", s(&evidence[i]["agent"])),
                        Link::Source(i),
                    ),
                    None => doc.text("— original assistant statement", MUTED),
                }
                if s(&d["reason"]).trim() != quote.trim() {
                    doc.text(s(&d["reason"]), MUTED);
                }
            } else {
                // Offline records: the record's own reason is the statement.
                doc.text(s(&d["reason"]), TEXT);
                doc.text("Self-reported in the agent's decision record.", MUTED);
            }
        }
        "inferred" => {
            doc.text(s(&d["reason"]), TEXT);
            doc.text(
                "A retrospective explanation. The agent did not state this reason.",
                MUTED,
            );
        }
        _ => {
            doc.text(
                "Not captured. No statement in this work explains the choice.",
                TEXT,
            );
            let reason = s(&d["reason"]).trim();
            if assessed && !reason.is_empty() {
                doc.heading("Possible rationale · assessment");
                doc.text(reason, TEXT);
            }
        }
    }

    // What this means: consequences, the part a paraphrased reason doesn't give you.
    let implications = arr(&d["implications"]);
    if !implications.is_empty() || (assessed && !s(&d["significance"]).is_empty()) {
        doc.heading("What this means");
        for text in implications {
            doc.text(format!("• {}", s(text)), TEXT);
        }
        if implications.is_empty() {
            doc.text(s(&d["significance"]), TEXT);
        } else if assessed && !s(&d["significance"]).is_empty() {
            doc.text(s(&d["significance"]), MUTED);
        }
    }

    // Alternatives and trade-offs, each labelled with its attribution.
    let attribution = if assessed {
        " · assessed, not recorded"
    } else {
        " · self-reported"
    };
    for (key, title) in [
        ("alternatives", "Alternatives"),
        ("tradeoffs", "Trade-offs"),
    ] {
        let items = arr(&d[key]);
        if items.is_empty() {
            doc.heading(title);
            doc.text("Not established", MUTED);
            continue;
        }
        doc.heading(format!("{title}{attribution}"));
        for text in items {
            doc.text(format!("• {}", s(text)), TEXT);
        }
    }

    // Evidence: which edits, with a one-line summary each.
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
            format!("{}{}", s(&e["file"]), edit_summary(e)),
            Link::Source(i),
        );
    }
    doc.heading("Original context");
    let mut context = 0;
    for (i, e) in evidence
        .iter()
        .enumerate()
        .filter(|(_, e)| ids.contains(&e["id"]) && e["kind"] == "session")
    {
        context += 1;
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
    if context == 0 {
        doc.text(
            if status == "unknown" {
                "No statement in this work mentions these files."
            } else {
                "No original context cited"
            },
            MUTED,
        );
    }

    let mut details = vec![Line::from(if assessed {
        "Alternatives, trade-offs and implications are assessments unless explicitly attributed to a recorded statement."
    } else {
        "Records are self-reported, not independently verified."
    })];
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
