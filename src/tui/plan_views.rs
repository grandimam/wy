//! Plan: the first screen. What changed, what to worry about, was it tested, and
//! how far you have got — each row opens its evidence. Steps: the same session in
//! order, the audit trail you check the plan against. Both hang off one hunk spine.
use super::{
    document::{Document, Link, View},
    theme::*,
};
use crate::{arr, hunks, n, s, security, steps, verdicts};
use ratatui::prelude::*;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::Arc,
};

fn link(doc: &mut Document, label: impl AsRef<str>, link: Link) {
    doc.sources.push((doc.lines.len(), link));
    doc.text(label, ACCENT);
}
fn tier(q: &Value) -> (&'static str, Color) {
    match s(&q["consequence"]) {
        "high" => ("HIGH", RED),
        "medium" => ("MED ", AMBER),
        _ => ("LOW ", MUTED),
    }
}
fn said(q: &Value) -> (&'static str, Color) {
    match s(&q["said"]["status"]) {
        "recorded" => ("recorded", GREEN),
        "inferred" => ("inferred", AMBER),
        _ => ("no reason", MUTED),
    }
}
fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
fn time(step: &Value, key: &str) -> String {
    step[key]
        .as_str()
        .and_then(|t| t.get(11..16))
        .map(str::to_owned)
        .unwrap_or_else(|| "--:--".into())
}
fn short_files(step: &Value, limit: usize) -> String {
    security::short(
        &arr(&step["files"])
            .iter()
            .map(|f| s(f).rsplit('/').next().unwrap_or(s(f)))
            .collect::<Vec<_>>()
            .join(", "),
        limit,
    )
}
fn step_tag(ordinals: &[usize]) -> String {
    if ordinals.is_empty() {
        "⚠ no captured evidence".into()
    } else {
        format!(
            "step {}",
            ordinals
                .iter()
                .map(|i| (i + 1).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

pub(super) enum PlanState {
    NotGenerated,
    Generating,
    Ready,
    Stale,
    Failed(String),
    NoChanges,
}

/// Steps that produced code: the "what changed" groups.
pub(super) fn change_groups(step_list: &[Value]) -> Vec<&Value> {
    step_list
        .iter()
        .filter(|st| !arr(&st["edit_indices"]).is_empty())
        .collect()
}

/// Evidence gaps that need no model: files no captured edit explains, shell writes.
pub(super) fn gaps(hunk_list: &[Value], step_list: &[Value]) -> Vec<(String, String)> {
    let mut rows = vec![];
    let mut unexplained: BTreeMap<&str, usize> = BTreeMap::new();
    for h in hunk_list.iter().filter(|h| h["attribution"] == "none") {
        *unexplained.entry(s(&h["file"])).or_default() += 1;
    }
    let derived: Vec<&str> = unexplained
        .keys()
        .copied()
        .filter(|f| hunks::derived(f))
        .collect();
    let real: Vec<&str> = unexplained
        .keys()
        .copied()
        .filter(|f| !hunks::derived(f))
        .collect();
    if !real.is_empty() {
        let mut shown: Vec<&str> = real.iter().copied().take(3).collect();
        if real.len() > 3 {
            shown.push("…");
        }
        rows.push((
            "unexplained".into(),
            format!(
                "{} changed with no record of why ({})",
                plural(real.len(), "file", "files"),
                shown.join(", ")
            ),
        ));
    }
    for st in step_list {
        for g in arr(&st["gaps"]) {
            if s(g).starts_with("shell command may have changed files") {
                rows.push((
                    format!("shell-{}", n(&st["ordinal"])),
                    format!(
                        "step {} ran a shell command that writes files · no record of the content",
                        n(&st["ordinal"]) + 1
                    ),
                ));
                break;
            }
        }
    }
    if !derived.is_empty() {
        rows.push((
            "derived".into(),
            format!("{} derived (lockfiles) · not reviewed", derived.join(", ")),
        ));
    }
    rows
}

/// Every validation run across steps, in order, with the step it belongs to.
pub(super) fn runs(step_list: &[Value]) -> Vec<(usize, Value)> {
    step_list
        .iter()
        .flat_map(|st| {
            arr(&st["checked"])
                .iter()
                .map(move |c| (n(&st["ordinal"]), c.clone()))
        })
        .collect()
}
fn tested_summary(step_list: &[Value]) -> (String, String) {
    let runs = runs(step_list);
    let tests: Vec<&Value> = runs
        .iter()
        .map(|(_, c)| c)
        .filter(|c| c["kind"] == "test")
        .collect();
    let builds = runs.iter().filter(|(_, c)| c["kind"] == "build").count();
    let failed = tests.iter().filter(|c| c["outcome"] == "failed").count();
    let line = if tests.is_empty() && builds == 0 {
        "No build or test run was recorded in this session".into()
    } else {
        let last = tests
            .last()
            .map(|c| match s(&c["outcome"]) {
                "ok" => "last run passed",
                "failed" => "last run FAILED",
                _ => "last outcome unknown",
            })
            .unwrap_or("no test runs");
        format!(
            "tests ran {} · {} · {}{}",
            plural(tests.len(), "time", "times"),
            if failed == 0 {
                "no failures".to_owned()
            } else {
                format!("{failed} failed, then fixed")
            },
            last,
            if builds > 0 {
                format!(" · {builds} build{}", if builds == 1 { "" } else { "s" })
            } else {
                String::new()
            }
        )
    };
    let untested: Vec<String> = change_groups(step_list)
        .iter()
        .filter(|st| !arr(&st["checked"]).iter().any(|c| c["kind"] == "test"))
        .map(|st| s(&st["title"]).to_lowercase())
        .collect();
    let not = if untested.is_empty() {
        "Every change group had a test run after it".into()
    } else {
        format!(
            "Not tested: {}",
            security::short(&untested.join(" · "), 110)
        )
    };
    (line, not)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn plan(
    brief: Arc<Value>,
    hunk_list: &[Value],
    step_list: &[Value],
    verdict_map: &BTreeMap<String, Value>,
    opened: &HashSet<String>,
    state: PlanState,
    agent: &str,
    evidence: &[(Value, Value, usize)],
    pinned: bool,
) -> Document {
    let mut doc = Document::new(View::Plan, "Plan · before you accept these changes");
    let files = hunks::files(hunk_list).len();
    let added: usize = hunk_list.iter().map(|h| arr(&h["added"]).len()).sum();
    let explained: BTreeSet<&str> = hunk_list
        .iter()
        .filter(|h| h["attribution"] != "none")
        .map(|h| s(&h["file"]))
        .collect();
    let evidence_line = match evidence {
        [] => "evidence: no captured session matches · Enter to choose one".to_owned(),
        [(sn, work, _)] => format!(
            "evidence: {} “{}” · {}{}",
            s(&sn["agent"]),
            security::short(s(&work["session"]["title"]), 44).replace('\n', " "),
            crate::insights::when(&work["session"]["last_event"]),
            if pinned { " · chosen by hand" } else { "" }
        ),
        many => format!(
            "evidence: {} sessions explain {} of {} files · Enter for detail",
            many.len(),
            explained.len(),
            files
        ),
    };
    if matches!(state, PlanState::NoChanges) {
        doc.text("Nothing uncommitted. There is nothing to accept yet.", TEXT);
        link(
            &mut doc,
            format!("{evidence_line} · Steps shows what it did."),
            Link::Evidence,
        );
        doc.artifact = Some(brief);
        return doc;
    }
    doc.text(
        format!(
            "{} · +{added} lines · uncommitted",
            plural(files, "file", "files")
        ),
        MUTED,
    );
    link(&mut doc, evidence_line, Link::Evidence);

    // What changed: plain-language groups from the steps that produced code.
    let groups = change_groups(step_list);
    doc.boxed("What changed", None, |doc| {
        if groups.is_empty() {
            doc.text(
                if step_list.is_empty() {
                    "No captured session explains these changes · pick a session on the left"
                } else {
                    "The selected session recorded no code edits"
                },
                MUTED,
            );
        }
        let several = evidence.len() > 1;
        for st in &groups {
            let ordinal = n(&st["ordinal"]);
            let seen = if opened.contains(&format!("change-{ordinal}")) {
                "✓"
            } else {
                " "
            };
            let count = arr(&st["files"]).len();
            link(
                doc,
                format!(
                    "{seen} {:<64} {:>2} {}{}",
                    security::short(s(&st["title"]), 64),
                    count,
                    if count == 1 { "file " } else { "files" },
                    if several {
                        format!("  ⟨{}⟩", security::short(s(&st["session_label"]), 28))
                    } else {
                        String::new()
                    }
                ),
                Link::Change(ordinal),
            );
        }
    });

    // What to worry about: formed questions (model, on request) and offline gaps.
    let questions = arr(&brief["questions"]);
    let gap_rows = gaps(hunk_list, step_list);
    doc.boxed("What to worry about", None, |doc| {
        for (i, q) in questions.iter().enumerate() {
            let (t, _) = tier(q);
            let (sd, _) = said(q);
            let verdict = verdict_map.get(s(&q["key"]));
            let state = verdict.map(|v| s(&v["state"])).unwrap_or("");
            let stale = verdict.is_some_and(|v| verdicts::stale(v, q, hunk_list));
            let mark = if state.is_empty() {
                if opened.contains(&format!("question-{i}")) { "✓" } else { " " }
            } else {
                verdicts::glyph(state)
            };
            let status = if state.is_empty() {
                sd.to_owned()
            } else {
                format!("{}{}", verdicts::label(state), if stale { " · stale" } else { "" })
            };
            link(
                doc,
                format!(
                    "{mark} {t}  {:<58} {status}",
                    security::short(s(&q["question"]), 58).replace('\n', " ")
                ),
                Link::Question(i),
            );
        }
        match &state {
            PlanState::Generating => doc.text("  …   forming the questions a domain expert would ask · x cancels", AMBER),
            PlanState::Failed(e) => {
                link(doc, format!("  !   could not form questions: {} · Enter retries", security::short(e, 60)), Link::GenerateQuestions)
            }
            PlanState::Stale => link(
                doc,
                "  !   the code changed since these were formed · Enter re-forms them",
                Link::GenerateQuestions,
            ),
            PlanState::NotGenerated => link(
                doc,
                format!("  ?   what a domain expert would ask about this diff · Enter asks {agent} · nothing has run"),
                Link::GenerateQuestions,
            ),
            PlanState::Ready if questions.is_empty() => {
                doc.text("  ·   no consequential questions were formed", MUTED)
            }
            _ => {}
        }
        for (key, text) in &gap_rows {
            let seen = if opened.contains(&format!("gap-{key}")) { "✓" } else { " " };
            link(doc, format!("{seen} ·    {text}"), Link::Gap(key.clone()));
        }
        if questions.is_empty() && gap_rows.is_empty() && matches!(state, PlanState::Ready) {
            doc.text("  nothing flagged", MUTED);
        }
    });

    // Was it tested: facts only.
    let (ran, not) = tested_summary(step_list);
    let seen = if opened.contains("tests") { "✓" } else { " " };
    doc.boxed("Was it tested", None, |doc| {
        link(doc, format!("{seen} {ran}"), Link::Tests);
        doc.text(
            format!("  {not}"),
            if not.starts_with("Not tested") {
                AMBER
            } else {
                MUTED
            },
        );
    });

    // Progress: have you actually looked, and have you decided.
    let read = groups
        .iter()
        .filter(|st| opened.contains(&format!("change-{}", n(&st["ordinal"]))))
        .count();
    let decided = questions
        .iter()
        .filter(|q| verdict_map.contains_key(s(&q["key"])))
        .count();
    let pushed = questions
        .iter()
        .filter(|q| {
            verdict_map
                .get(s(&q["key"]))
                .is_some_and(|v| v["state"] == "push_back")
        })
        .count();
    let rejected = arr(&brief["rejected"]).len();
    doc.boxed("Progress", None, |doc| {
        let mut parts = vec![format!("{read} of {} changes read", groups.len())];
        if !questions.is_empty() {
            parts.push(format!("{decided} of {} worries decided", questions.len()));
        }
        parts.push(if pushed == 0 {
            "nothing pushed back".into()
        } else {
            format!("{pushed} pushed back · /export writes the review")
        });
        doc.text(format!("  {}", parts.join(" · ")), TEXT);
        if rejected > 0 {
            doc.text(
                format!(
                    "  {} formed question{} rejected: citations did not check out",
                    rejected,
                    if rejected == 1 { " was" } else { "s were" }
                ),
                AMBER,
            );
        }
    });
    doc.source_selection = (!doc.sources.is_empty()).then_some(0);
    doc.artifact = Some(brief);
    doc
}

/// Render one hunk as a diff with current-tree line numbers, optional `◄` markers
/// on specific new lines, and a step tag. Collapsed when `open` is false.
pub(super) fn hunk_block(
    doc: &mut Document,
    hunk: &Value,
    markers: &BTreeMap<usize, String>,
    tag: &str,
    open: bool,
) {
    let start = doc.lines.len();
    let mut new = Some(n(&hunk["start"]));
    let mut old: Option<usize> = None;
    for line in s(&hunk["diff"]).lines() {
        if let Some(header) = line.strip_prefix("@@") {
            old = header
                .split_whitespace()
                .find_map(|f| f.strip_prefix('-'))
                .and_then(|f| f.split(',').next())
                .and_then(|f| f.parse().ok());
            continue;
        }
        let (gutter_old, gutter_new, sign, body, color) = if let Some(body) = line.strip_prefix('+')
        {
            let at = new;
            new = new.map(|v| v + 1);
            (None, at, '+', body, GREEN)
        } else if let Some(body) = line.strip_prefix('-') {
            let at = old;
            old = old.map(|v| v + 1);
            (at, None, '-', body, RED)
        } else {
            let body = line.strip_prefix(' ').unwrap_or(line);
            let (a, b) = (old, new);
            old = old.map(|v| v + 1);
            new = new.map(|v| v + 1);
            (a, b, ' ', body, TEXT)
        };
        let number = |v: Option<usize>| v.map(|v| v.to_string()).unwrap_or_default();
        let marker = gutter_new.and_then(|l| markers.get(&l));
        let text = match marker {
            Some(note) => format!("{body}    ◄ {note}"),
            None => body.to_owned(),
        };
        doc.code_line(
            format!(
                "{:>5} {:>5} {sign} │ ",
                number(gutter_old),
                number(gutter_new)
            ),
            &text,
            if marker.is_some() { AMBER } else { color },
        );
    }
    doc.gap();
    let title = format!(
        "{}  @@ {}–{}  {}{}",
        s(&hunk["file"]),
        n(&hunk["start"]),
        n(&hunk["end"]),
        tag,
        if hunk["attribution"] == "partial" {
            "  (partly captured)"
        } else {
            ""
        }
    );
    doc.fold_code(
        format!("hunk-{}-{start}", s(&hunk["id"])),
        &title,
        start,
        open,
    );
}

fn why_rows(doc: &mut Document, st: &Value, long: bool) {
    let recorded = arr(&st["why"]["recorded"]);
    if recorded.is_empty() {
        let nearby = arr(&st["why"]["nearby"])
            .iter()
            .find(|nb| nb["after_edits"] != true && nb["secondary"] != true);
        match nearby {
            Some(nb) => {
                doc.text("no reason given · nearby:", AMBER);
                doc.text(
                    format!(
                        "  “{}”",
                        security::short(s(&nb["text"]), if long { 600 } else { 160 })
                            .trim()
                            .replace('\n', " ")
                    ),
                    MUTED,
                );
            }
            None => doc.text("no reason given", AMBER),
        }
    }
    for r in recorded.iter().take(if long { 6 } else { 1 }) {
        doc.text(
            format!(
                "“{}”",
                security::short(s(&r["quote"]), if long { 1200 } else { 220 })
                    .trim()
                    .replace('\n', " ")
            ),
            TEXT,
        );
        if long {
            doc.text(
                format!(
                    "— {} · {} · {}",
                    s(&r["agent"]),
                    if r["kind"] == "decision_record" {
                        "decision record"
                    } else {
                        "exact quote"
                    },
                    crate::insights::when(&r["timestamp"])
                ),
                GREEN,
            );
        }
    }
}
fn check_rows(doc: &mut Document, st: &Value) {
    let checked = arr(&st["checked"]);
    if checked.is_empty() {
        doc.text(
            if arr(&st["edit_indices"]).is_empty() {
                "—"
            } else {
                "✗ no build or test run"
            },
            if arr(&st["edit_indices"]).is_empty() {
                MUTED
            } else {
                AMBER
            },
        );
        return;
    }
    let mut parts = vec![];
    let mut previous_failed = false;
    for c in checked {
        let (g, _) = match s(&c["outcome"]) {
            "ok" => ("✓", GREEN),
            "failed" => ("✗", RED),
            _ => ("·", MUTED),
        };
        let label = format!("{g} {}", security::short(s(&c["command"]), 28));
        if previous_failed && c["outcome"] == "ok" {
            parts.push(format!("→ fixed {label}"));
        } else {
            parts.push(label);
        }
        previous_failed = c["outcome"] == "failed";
    }
    doc.text(parts.join("   "), TEXT);
}

/// Enter on a "What changed" row.
pub(super) fn change(
    step_list: &[Value],
    ordinal: usize,
    hunk_list: &[Value],
    brief: Option<&Value>,
) -> Option<Document> {
    let st = step_list.iter().find(|st| n(&st["ordinal"]) == ordinal)?;
    let mut doc = Document::new(View::PlanChange, s(&st["title"]));
    doc.text(
        format!(
            "step {} of {} · {} · {}",
            ordinal + 1,
            step_list.len(),
            time(st, "started"),
            short_files(st, 80)
        ),
        MUTED,
    );
    doc.heading("What it does");
    let produced: Vec<&Value> = steps::hunks_for(st, hunk_list)
        .iter()
        .filter_map(|id| hunks::find(hunk_list, id))
        .collect();
    let added: usize = produced.iter().map(|h| arr(&h["added"]).len()).sum();
    doc.text(
        format!(
            "Edits {} · {} in the working tree · +{added} lines",
            short_files(st, 90),
            plural(produced.len(), "hunk", "hunks")
        ),
        TEXT,
    );
    doc.heading("Why, in the agent's words");
    why_rows(&mut doc, st, true);
    doc.heading("Checked");
    check_rows(&mut doc, st);
    for g in arr(&st["gaps"]) {
        doc.text(format!("⚠ {}", s(g)), AMBER);
    }
    if let Some(b) = brief {
        let related: Vec<(usize, &Value)> = arr(&b["questions"])
            .iter()
            .enumerate()
            .filter(|(_, q)| {
                arr(&q["hunks"])
                    .iter()
                    .any(|id| produced.iter().any(|h| h["id"] == *id))
            })
            .collect();
        if !related.is_empty() {
            doc.heading("Worries about this change");
            for (qi, q) in related {
                let (t, _) = tier(q);
                let (sd, _) = said(q);
                link(
                    &mut doc,
                    format!("  {t}  {}  {sd}", security::short(s(&q["question"]), 60)),
                    Link::Question(qi),
                );
            }
        }
    }
    doc.heading(format!(
        "The code · {}",
        plural(produced.len(), "hunk", "hunks")
    ));
    for (i, h) in produced.iter().enumerate() {
        hunk_block(&mut doc, h, &BTreeMap::new(), "", i < 3);
        link(
            &mut doc,
            format!("   full diff for {}", s(&h["file"])),
            Link::Hunk(s(&h["id"]).into()),
        );
    }
    if produced.is_empty() {
        doc.text("This step's captured edits are no longer in the working tree (superseded, reverted or reformatted).", MUTED);
    }
    doc.gap();
    link(&mut doc, "Open this step in Steps →", Link::Step(ordinal));
    link(&mut doc, "← Plan", Link::PlanHome);
    doc.artifact = Some(Arc::new(serde_json::json!({"context": "plan"})));
    Some(doc)
}

/// Enter on a gap row.
pub(super) fn gap(key: &str, hunk_list: &[Value], step_list: &[Value]) -> Document {
    let mut doc = Document::new(View::PlanGap, "Evidence gap");
    match key {
        "unexplained" => {
            doc.title = "Changed with no record of why".into();
            doc.text(
                "These hunks match no captured edit in the selected session. They may be yours, an earlier session's, a formatter's, or shell-written by the agent. wy does not guess.",
                TEXT,
            );
            let mut by_file: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
            for h in hunk_list
                .iter()
                .filter(|h| h["attribution"] == "none" && !hunks::derived(s(&h["file"])))
            {
                by_file.entry(s(&h["file"])).or_default().push(h);
            }
            for (file, list) in by_file {
                doc.heading(format!("{file} · {}", plural(list.len(), "hunk", "hunks")));
                for h in list {
                    hunk_block(&mut doc, h, &BTreeMap::new(), "", false);
                }
                link(
                    &mut doc,
                    format!("   full diff for {file}"),
                    Link::Hunk(by_hunk_id(hunk_list, file)),
                );
            }
            doc.heading("What you can do");
            doc.text("• If you wrote these, nothing to judge here.", MUTED);
            doc.text("• If the agent did, pick the session that did it on the left, or ask it to re-apply edits through its edit tool so they are captured.", MUTED);
        }
        "derived" => {
            doc.title = "Derived files".into();
            doc.text("Lockfiles and generated artifacts follow from other changes. wy shows them in the diff but never asks you to judge them.", TEXT);
            for h in hunk_list.iter().filter(|h| hunks::derived(s(&h["file"]))) {
                doc.text(
                    format!(
                        "  {} · {}",
                        s(&h["file"]),
                        plural(arr(&h["added"]).len(), "line", "lines")
                    ),
                    MUTED,
                );
            }
        }
        _ if key.starts_with("shell-") => {
            let ordinal: usize = key.trim_start_matches("shell-").parse().unwrap_or(0);
            if let Some(st) = step_list.iter().find(|st| n(&st["ordinal"]) == ordinal) {
                doc.title = format!("Step {} wrote files through the shell", ordinal + 1);
                doc.text("Edits made with shell redirection, sed, mv or similar leave no captured content. The files changed, but what the agent wrote is not on record.", TEXT);
                doc.heading("Commands");
                for g in arr(&st["gaps"])
                    .iter()
                    .filter(|g| s(g).starts_with("shell command"))
                {
                    doc.text(
                        format!(
                            "  {}",
                            s(g).trim_start_matches(
                                "shell command may have changed files without captured content: "
                            )
                        ),
                        MUTED,
                    );
                }
                doc.gap();
                link(
                    &mut doc,
                    format!("Open step {} →", ordinal + 1),
                    Link::Step(ordinal),
                );
            }
        }
        _ => doc.text("Unknown gap", MUTED),
    }
    doc.gap();
    link(&mut doc, "← Plan", Link::PlanHome);
    doc.artifact = Some(Arc::new(serde_json::json!({"context": "plan"})));
    doc
}
fn by_hunk_id(hunk_list: &[Value], file: &str) -> String {
    hunk_list
        .iter()
        .find(|h| h["file"] == file)
        .map(|h| s(&h["id"]).to_owned())
        .unwrap_or_default()
}

/// Enter on "Was it tested".
pub(super) fn tests(step_list: &[Value]) -> Document {
    let mut doc = Document::new(View::PlanTests, "Was it tested");
    let (ran, not) = tested_summary(step_list);
    doc.text(ran, TEXT);
    doc.text(not, AMBER);
    doc.heading("Every run, in order");
    let all = runs(step_list);
    if all.is_empty() {
        doc.text("No build or test command was recorded.", MUTED);
    }
    for (ordinal, c) in &all {
        let (g, color) = match s(&c["outcome"]) {
            "ok" => ("✓", GREEN),
            "failed" => ("✗", RED),
            _ => ("·", MUTED),
        };
        link(
            &mut doc,
            format!(
                "{g} step {:<2} {:<6} {}",
                ordinal + 1,
                s(&c["kind"]),
                security::short(s(&c["command"]), 80)
            ),
            Link::Step(*ordinal),
        );
        let _ = color;
    }
    doc.heading("Change groups and their tests");
    for st in change_groups(step_list) {
        let tested = arr(&st["checked"]).iter().any(|c| c["kind"] == "test");
        link(
            &mut doc,
            format!("{} {}", if tested { "✓" } else { "✗" }, s(&st["title"])),
            Link::Step(n(&st["ordinal"])),
        );
    }
    doc.text(
        "A test run after a step is evidence it was exercised, not proof it was covered.",
        MUTED,
    );
    doc.gap();
    link(&mut doc, "← Plan", Link::PlanHome);
    doc.artifact = Some(Arc::new(serde_json::json!({"context": "plan"})));
    doc
}

/// Enter on a worry (a formed question).
pub(super) fn worry(
    brief: Arc<Value>,
    index: usize,
    hunk_list: &[Value],
    step_list: &[Value],
    verdict_map: &BTreeMap<String, Value>,
) -> Option<Document> {
    let q = arr(&brief["questions"]).get(index)?.clone();
    let (t, tc) = tier(&q);
    let (sd, sc) = said(&q);
    let mut doc = Document::new(
        View::Worry,
        format!("{} · {}", index + 1, s(&q["question"])),
    );
    doc.lines.push(Line::from(vec![
        Span::styled(t.trim(), Style::default().fg(tc).bold()),
        Span::raw(" · agent's record: "),
        Span::styled(sd.trim(), Style::default().fg(sc)),
    ]));

    doc.heading("Why this matters here");
    doc.text(s(&q["domain_why"]), TEXT);
    if !s(&q["consequence_note"]).is_empty() {
        doc.text(format!("If wrong: {}", s(&q["consequence_note"])), TEXT);
    }
    doc.heading("What the agent chose");
    doc.text(s(&q["chosen"]), TEXT);
    doc.text("(observed in the diff)", MUTED);
    doc.heading("What the agent said about it");
    match s(&q["said"]["status"]) {
        "recorded" => {
            doc.text(format!("“{}”", s(&q["said"]["quote"]).trim()), TEXT);
            if let Some(st) = arr(&brief["packet"]["statements"])
                .iter()
                .find(|st| st["id"] == q["said"]["quote_id"])
            {
                doc.text(
                    format!(
                        "— {} · {}",
                        brief["packet"]["session"]["agent"]
                            .as_str()
                            .unwrap_or("agent"),
                        crate::insights::when(&st["timestamp"])
                    ),
                    MUTED,
                );
            }
            doc.text("(exact quote)", GREEN);
        }
        "inferred" => {
            doc.text(
                "No direct statement. A plausible reason can be read from the code:",
                TEXT,
            );
            let from: Vec<&str> = arr(&q["said"]["inferred_from"]).iter().map(s).collect();
            doc.text(format!("inferred from {}", from.join(", ")), AMBER);
        }
        _ => {
            doc.text(
                "Nothing. No recorded statement, rationale or alternative addresses this.",
                TEXT,
            );
            doc.text("That is not proof the agent didn't consider it — only that there is no evidence it did.", MUTED);
        }
    }

    let mut markers: BTreeMap<&str, BTreeMap<usize, String>> = BTreeMap::new();
    for c in arr(&q["check"]) {
        if n(&c["line"]) > 0 {
            markers
                .entry(s(&c["hunk"]))
                .or_default()
                .insert(n(&c["line"]), s(&c["note"]).to_owned());
        }
    }
    let cited: Vec<&Value> = arr(&q["hunks"])
        .iter()
        .filter_map(|id| hunks::find(hunk_list, s(id)))
        .collect();
    doc.heading(format!(
        "The code · {}",
        plural(cited.len(), "hunk", "hunks")
    ));
    for (i, h) in cited.iter().enumerate() {
        let ordinals = steps::for_hunk(step_list, h);
        hunk_block(
            &mut doc,
            h,
            markers.get(s(&h["id"])).unwrap_or(&BTreeMap::new()),
            &step_tag(&ordinals),
            i < 2,
        );
        if h["attribution"] == "none" && h["derived"] != true {
            doc.text(
                "   ⚠ no captured edit for this hunk · shown from the working tree",
                AMBER,
            );
        }
        link(
            &mut doc,
            format!("   full diff for {}", s(&h["file"])),
            Link::Hunk(s(&h["id"]).into()),
        );
        for o in ordinals {
            link(&mut doc, format!("   step {} →", o + 1), Link::Step(o));
        }
    }
    if cited.len() < arr(&q["hunks"]).len() {
        doc.text("Some cited hunks are no longer in the working tree.", AMBER);
    }
    let checks: Vec<&Value> = arr(&q["check"]).iter().collect();
    if !checks.is_empty() {
        doc.heading("How to check it yourself");
        for c in checks {
            let place = hunks::find(hunk_list, s(&c["hunk"]))
                .map(|h| {
                    format!(
                        "{}:{}",
                        s(&h["file"]),
                        if n(&c["line"]) > 0 {
                            n(&c["line"])
                        } else {
                            n(&h["start"])
                        }
                    )
                })
                .unwrap_or_else(|| s(&c["hunk"]).into());
            doc.text(format!("→ {place}   {}", s(&c["note"])), TEXT);
        }
    }
    if !s(&q["expert_view"]).trim().is_empty() {
        doc.heading("What an expert would want here · wy's assessment");
        doc.text(s(&q["expert_view"]), TEXT);
    }
    let alternatives = arr(&q["alternatives"]);
    if !alternatives.is_empty() {
        doc.heading("Alternatives");
        for a in alternatives {
            doc.text(
                format!(
                    "• {}   ({})",
                    s(&a["text"]),
                    if a["attributed"] == "self-reported" {
                        "self-reported"
                    } else {
                        "assessed"
                    }
                ),
                TEXT,
            );
        }
    }

    doc.heading("Your verdict");
    let verdict = verdict_map.get(s(&q["key"]));
    let state = verdict.map(|v| s(&v["state"])).unwrap_or("");
    let stale = verdict.is_some_and(|v| verdicts::stale(v, &q, hunk_list));
    for (st, label, key) in [
        ("accept", "accept", "y"),
        ("push_back", "push back", "n"),
        ("unsure", "unsure", "m"),
    ] {
        link(
            &mut doc,
            format!("   {} {label}   {key}", if state == st { "●" } else { "○" }),
            Link::Verdict(index, st.into()),
        );
    }
    if stale {
        doc.text(
            "   ⚠ the code this is about changed after your verdict",
            AMBER,
        );
    }
    let note = verdict.map(|v| s(&v["note"])).unwrap_or("");
    link(
        &mut doc,
        if note.is_empty() {
            "   note: (i to add)".to_owned()
        } else {
            format!("   note: {note}")
        },
        Link::Note(index),
    );
    doc.gap();
    link(&mut doc, "← Plan", Link::PlanHome);
    if index + 1 < arr(&brief["questions"]).len() {
        link(&mut doc, "Next worry →", Link::Question(index + 1));
    }
    doc.artifact = Some(brief);
    Some(doc)
}

/// The Steps screen: one box per step, in order.
pub(super) fn steps(
    step_list: &[Value],
    hunk_list: &[Value],
    evidence: &[(Value, Value, usize)],
    brief: Option<&Value>,
) -> Document {
    let mut doc = Document::new(View::Steps, "Steps · what the agent did, in order");
    if evidence.is_empty() {
        doc.text("No captured session matches these changes.", AMBER);
        link(&mut doc, "Choose a session by hand →", Link::Evidence);
        return doc;
    }
    let first = step_list
        .first()
        .map(|st| time(st, "started"))
        .unwrap_or_default();
    let last = step_list
        .last()
        .map(|st| time(st, "ended"))
        .unwrap_or_default();
    let files: BTreeSet<&str> = step_list
        .iter()
        .flat_map(|st| arr(&st["files"]).iter().map(s))
        .collect();
    let several = evidence.len() > 1;
    let source = if several {
        format!("{} sessions", evidence.len())
    } else {
        let (sn, work, _) = &evidence[0];
        format!(
            "{} · “{}”",
            s(&sn["agent"]),
            security::short(s(&work["session"]["title"]), 50).replace('\n', " ")
        )
    };
    link(
        &mut doc,
        format!(
            "{source} · {first} → {last} · {} · {}",
            plural(step_list.len(), "step", "steps"),
            plural(files.len(), "file", "files")
        ),
        Link::Evidence,
    );
    let discussion = step_list
        .iter()
        .filter(|st| arr(&st["edit_indices"]).is_empty() && arr(&st["checked"]).is_empty())
        .count();
    if discussion > 0 {
        doc.text(
            format!(
                "{} step{} with no edits or checks (discussion) hidden · open a step for its conversation",
                discussion,
                if discussion == 1 { "" } else { "s" }
            ),
            MUTED,
        );
    }
    for st in step_list
        .iter()
        .filter(|st| !arr(&st["edit_indices"]).is_empty() || !arr(&st["checked"]).is_empty())
    {
        let ordinal = n(&st["ordinal"]);
        let files = short_files(st, 40);
        // Fallback titles already name the files; do not say them twice.
        let fallback = s(&st["title"]).starts_with("Edit ");
        let title = format!(
            "{}  {}  {}{}{}",
            ordinal + 1,
            time(st, "started"),
            security::short(s(&st["title"]), if fallback { 70 } else { 56 }),
            if files.is_empty() || fallback {
                String::new()
            } else {
                format!("  ── {files}")
            },
            if several {
                format!("  ⟨{}⟩", security::short(s(&st["session_label"]), 28))
            } else {
                String::new()
            }
        );
        let worries: Vec<(usize, &Value)> = brief
            .map(|b| {
                let produced = steps::hunks_for(st, hunk_list);
                arr(&b["questions"])
                    .iter()
                    .enumerate()
                    .filter(|(_, q)| {
                        arr(&q["hunks"])
                            .iter()
                            .any(|id| produced.contains(&s(id).to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        doc.boxed(&title, Some(Link::Step(ordinal)), |doc| {
            let at = doc.lines.len();
            why_rows(doc, st, false);
            prefix(doc, at, "Why    ");
            let at = doc.lines.len();
            check_rows(doc, st);
            prefix(doc, at, "Check  ");
            for g in arr(&st["gaps"]).iter().take(2) {
                doc.text(format!("Gap    {}", security::short(s(g), 100)), AMBER);
            }
            for (qi, q) in worries {
                let (t, _) = tier(q);
                link(
                    doc,
                    format!("Plan   {t}  {}", security::short(s(&q["question"]), 70)),
                    Link::Question(qi),
                );
            }
        });
    }
    let unowned: Vec<&Value> = hunk_list
        .iter()
        .filter(|h| steps::for_hunk(step_list, h).is_empty())
        .collect();
    if !unowned.is_empty() {
        doc.boxed("Not explained by this session", None, |doc| {
            let mut by_file: BTreeMap<&str, usize> = BTreeMap::new();
            for h in &unowned {
                *by_file.entry(s(&h["file"])).or_default() += 1;
            }
            for (file, count) in by_file {
                let tag = if hunks::derived(file) {
                    "derived"
                } else {
                    "no captured edit matches"
                };
                doc.text(
                    format!("{file} · {} · {tag}", plural(count, "hunk", "hunks")),
                    AMBER,
                );
            }
        });
    }
    doc.gap();
    doc.text("Steps group one captured session by intent; when unsure wy does not split. Shell and manual edits may be missing.", MUTED);
    doc.source_selection = (!doc.sources.is_empty()).then_some(0);
    doc.artifact = Some(Arc::new(serde_json::json!({"context": "steps"})));
    doc
}
/// Prepend a fixed label to the first line written since `at`, indent the rest.
fn prefix(doc: &mut Document, at: usize, label: &str) {
    for (i, line) in doc.lines.iter_mut().enumerate().skip(at) {
        let lead = if i == at {
            label.to_owned()
        } else {
            " ".repeat(label.len())
        };
        line.spans
            .insert(0, Span::styled(lead, Style::default().fg(MUTED)));
    }
}

/// Enter on a step.
pub(super) fn step(
    step_list: &[Value],
    index: usize,
    hunk_list: &[Value],
    edits: &[Value],
    brief: Option<&Value>,
) -> Option<Document> {
    let st = step_list.get(index)?;
    let mut doc = Document::new(View::Step, format!("{} · {}", index + 1, s(&st["title"])));
    doc.text(
        format!(
            "{}–{} · request {} · {} · {}",
            time(st, "started"),
            time(st, "ended"),
            n(&st["turn"]) + 1,
            steps::status_label(st),
            s(&st["session_label"])
        ),
        MUTED,
    );
    if !s(&st["request"]).is_empty() {
        doc.text(
            format!(
                "You asked: {}",
                security::short(s(&st["request"]), 160).replace('\n', " ")
            ),
            MUTED,
        );
    }
    doc.heading("What it did");
    let files = arr(&st["files"])
        .iter()
        .map(s)
        .collect::<Vec<_>>()
        .join(", ");
    doc.text(
        if files.is_empty() {
            "No captured edits in this step (planning, reading or shell work).".to_owned()
        } else {
            format!("Edited {files}.")
        },
        TEXT,
    );
    for nb in arr(&st["why"]["nearby"])
        .iter()
        .filter(|nb| nb["after_edits"] == true)
        .take(2)
    {
        doc.text(
            format!(
                "Afterwards: “{}”",
                security::short(s(&nb["text"]), 300).trim()
            ),
            MUTED,
        );
    }
    doc.heading("Why it said it did it");
    why_rows(&mut doc, st, true);
    let rationale = arr(&st["why"]["rationale"]);
    if !rationale.is_empty() {
        let lines: Vec<Line<'static>> = rationale
            .iter()
            .flat_map(|r| {
                security::short(s(&r["text"]), 1500)
                    .lines()
                    .map(|l| Line::from(l.to_owned()))
                    .collect::<Vec<_>>()
            })
            .collect();
        doc.gap();
        doc.disclosure(
            format!("step-rationale-{index}"),
            "Rationale (tentative · readable thinking, not a verified reason)",
            lines,
        );
    }
    doc.heading("What it checked");
    check_rows(&mut doc, st);
    for g in arr(&st["gaps"]) {
        doc.text(format!("⚠ {}", s(g)), AMBER);
    }
    let produced: Vec<&Value> = steps::hunks_for(st, hunk_list)
        .iter()
        .filter_map(|id| hunks::find(hunk_list, id))
        .collect();
    doc.heading(format!(
        "The code · {} in the working tree",
        plural(produced.len(), "hunk", "hunks")
    ));
    for (i, h) in produced.iter().enumerate() {
        hunk_block(&mut doc, h, &BTreeMap::new(), "", i < 3);
        link(
            &mut doc,
            format!("   full diff for {}", s(&h["file"])),
            Link::Hunk(s(&h["id"]).into()),
        );
    }
    let superseded: Vec<&Value> = arr(&st["edit_indices"])
        .iter()
        .filter_map(|i| edits.get(n(i)))
        .filter(|e| !produced.iter().any(|h| h["file"] == e["file"]))
        .collect();
    if !superseded.is_empty() {
        doc.text(
            "Captured edits no longer in the working tree (superseded, reverted or reformatted):",
            MUTED,
        );
        for e in superseded.iter().take(10) {
            link(
                &mut doc,
                format!("   {} · captured {}", s(&e["file"]), s(&e["operation"])),
                Link::SessionEdit(crate::history::edit_ref(e)),
            );
        }
    }
    if let Some(b) = brief {
        let related: Vec<(usize, &Value)> = arr(&b["questions"])
            .iter()
            .enumerate()
            .filter(|(_, q)| {
                arr(&q["hunks"])
                    .iter()
                    .any(|id| produced.iter().any(|h| h["id"] == *id))
            })
            .collect();
        if !related.is_empty() {
            doc.heading("Worries this step raises");
            for (qi, q) in related {
                let (t, _) = tier(q);
                let (sd, _) = said(q);
                link(
                    &mut doc,
                    format!("   {t}  {}  {sd}", security::short(s(&q["question"]), 60)),
                    Link::Question(qi),
                );
            }
        }
    }
    doc.gap();
    link(
        &mut doc,
        format!("Original conversation · request {} →", n(&st["turn"]) + 1),
        Link::Transcript(n(&st["turn"])),
    );
    link(&mut doc, "← All steps", Link::StepsHome);
    if index + 1 < step_list.len() {
        link(&mut doc, "Next step →", Link::Step(index + 1));
    }
    doc.artifact = Some(Arc::new(serde_json::json!({"context": "steps"})));
    Some(doc)
}

/// Enter on the evidence line: which sessions explain the diff, and how to change that.
pub(super) fn evidence(
    evidence: &[(Value, Value, usize)],
    candidates: &[Value],
    hunk_list: &[Value],
    pinned: Option<&str>,
) -> Document {
    let mut doc = Document::new(
        View::PlanGap,
        "Evidence · which sessions explain these changes",
    );
    doc.text(
        "A session counts as evidence when the text of one of its captured edits matches a hunk in the working tree. Nothing is assigned by date or proximity.",
        TEXT,
    );
    doc.heading(if pinned.is_some() {
        "Chosen by hand"
    } else {
        "Chosen from the diff"
    });
    if evidence.is_empty() {
        doc.text("No session matches. The diff may be yours, written through the shell, or from a session that was not captured.", AMBER);
    }
    for (sn, work, _) in evidence {
        let key = crate::session_work::snapshot_key(sn);
        let explained = hunk_list
            .iter()
            .filter(|h| arr(&h["evidence"]).iter().any(|e| e["session_key"] == key))
            .count();
        doc.text(
            format!(
                "● {} “{}” · {} · explains {}",
                s(&sn["agent"]),
                security::short(s(&work["session"]["title"]), 60).replace('\n', " "),
                crate::insights::when(&work["session"]["last_event"]),
                plural(explained, "hunk", "hunks")
            ),
            TEXT,
        );
    }
    if pinned.is_some() {
        link(&mut doc, "Let the diff choose instead", Link::AutoEvidence);
    }
    let others: Vec<&Value> = candidates
        .iter()
        .filter(|sn| {
            !evidence
                .iter()
                .any(|(e, _, _)| e["id"] == sn["id"] && e["agent"] == sn["agent"])
        })
        .collect();
    if !others.is_empty() {
        doc.heading("Other captured sessions · no edit matches this diff");
        for sn in others {
            let key = crate::session_work::snapshot_key(sn);
            link(
                &mut doc,
                format!(
                    "○ {} “{}” · {} · use this one",
                    s(&sn["agent"]),
                    security::short(&crate::session_work::title(sn), 60).replace('\n', " "),
                    crate::insights::when(&crate::insights::session_dates(sn).1)
                ),
                Link::Session(key),
            );
        }
    }
    doc.gap();
    link(
        &mut doc,
        "All captured sessions (picker) →",
        Link::SessionPicker,
    );
    link(&mut doc, "← Plan", Link::PlanHome);
    doc.artifact = Some(Arc::new(serde_json::json!({"context": "plan"})));
    doc
}
