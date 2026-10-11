//! The hunk spine. Every judgment question and every account step hangs off the
//! same list of HEAD-to-working-tree hunks. Attribution to a captured session edit
//! is a textual match or nothing; the diff is never *assigned* to a session because
//! the tree happens to be dirty while a session exists.
use crate::{arr, n, s, security};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Parse one changed file's unified diff into hunks with current-tree line ranges.
fn parse(change: &Value) -> Vec<Value> {
    let file = s(&change["file"]);
    let diff = s(&change["diff"]);
    let mut result = vec![];
    let mut current: Option<(usize, usize, Vec<String>, Vec<String>)> = None;
    let flush = |current: Option<(usize, usize, Vec<String>, Vec<String>)>,
                 out: &mut Vec<Value>| {
        let Some((start, new, lines, added)) = current else {
            return;
        };
        if lines.is_empty() {
            return;
        }
        let end = new.max(start).saturating_sub(1).max(start);
        let text = lines.join("\n");
        // Ids are file + ordinal, not line numbers: a verdict on the second hunk of a
        // file should survive an insertion above it and be flagged stale, not lost.
        out.push(json!({
            "id": format!("h-{}-{}", security::digest(file)[..8].to_owned(), out.len() + 1),
            "file": file, "start": start, "end": end,
            "diff": text, "added": added,
            "attribution": "none", "evidence": [], "symbol": ""
        }));
    };
    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("@@") {
            flush(current.take(), &mut result);
            let new_start = header
                .split_whitespace()
                .find_map(|f| f.strip_prefix('+'))
                .and_then(|f| f.split(',').next())
                .and_then(|f| f.parse::<usize>().ok())
                .unwrap_or(1)
                .max(1);
            current = Some((new_start, new_start, vec![line.to_owned()], vec![]));
            continue;
        }
        if (line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("\\ No newline"))
            && current.is_none()
        {
            continue;
        }
        let Some((_, new, lines, added)) = current.as_mut() else {
            // Diffs without headers (e.g. whole-file additions in fixtures) form one hunk.
            if line.starts_with('+') || line.starts_with('-') || line.starts_with(' ') {
                let mut added = vec![];
                if let Some(body) = line.strip_prefix('+') {
                    added.push(body.to_owned());
                }
                current = Some((
                    1,
                    1 + usize::from(!line.starts_with('-')),
                    vec![line.to_owned()],
                    added,
                ));
            }
            continue;
        };
        lines.push(line.to_owned());
        if let Some(body) = line.strip_prefix('+') {
            added.push(body.to_owned());
            *new += 1;
        } else if line.starts_with(' ') || line.is_empty() {
            *new += 1;
        }
    }
    flush(current, &mut result);
    for hunk in &mut result {
        let (start, end) = (n(&hunk["start"]), n(&hunk["end"]));
        // Smallest recorded symbol enclosing the range; "<module>" otherwise.
        let symbol = arr(&change["symbols"])
            .iter()
            .filter(|sym| n(&sym["start_line"]) <= start && end <= n(&sym["end_line"]))
            .min_by_key(|sym| n(&sym["end_line"]) - n(&sym["start_line"]))
            .map(|sym| s(&sym["symbol"]).to_owned())
            .unwrap_or_else(|| "<module>".into());
        hunk["symbol"] = json!(symbol);
    }
    result
}

/// Working-tree hunks for the saved review's changes. Derived artifacts carry no story.
pub fn working_tree(review: &Value) -> Vec<Value> {
    let mut result = vec![];
    let mut seen = HashSet::new();
    for change in arr(&review["changes"]) {
        for mut hunk in parse(change) {
            debug_assert!(seen.insert(s(&hunk["id"]).to_owned()));
            hunk["derived"] = json!(derived(s(&change["file"])));
            result.push(hunk);
        }
    }
    result
}

/// Lockfiles and generated artifacts: shown, never questioned.
pub fn derived(file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    [
        "Cargo.lock",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "poetry.lock",
        "Gemfile.lock",
        "go.sum",
        "composer.lock",
    ]
    .contains(&name)
        || name.ends_with(".min.js")
        || name.ends_with(".snap")
}

fn normalize(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Lines an edit introduced: complete writes contribute every line, patches only
/// their additions. Replacement-relative patches never claim file coordinates.
fn introduced(edit: &Value) -> Vec<String> {
    let text = s(&edit["text"]);
    if edit["format"] == "code" {
        text.lines().map(normalize).collect()
    } else {
        text.lines()
            .filter(|l| !l.starts_with("+++"))
            .filter_map(|l| l.strip_prefix('+'))
            .map(normalize)
            .collect()
    }
}

/// Attach captured edits whose text accounts for a hunk's added lines.
/// `matched`: every non-blank added line appears in captured edits for the file.
/// `partial`: some do. `none`: the file has no captured edit, or nothing matches.
pub fn attribute(hunks: &mut [Value], edits: &[Value]) {
    let mut by_file: BTreeMap<&str, Vec<(usize, HashSet<String>)>> = BTreeMap::new();
    for (index, edit) in edits.iter().enumerate() {
        by_file
            .entry(s(&edit["file"]))
            .or_default()
            .push((index, introduced(edit).into_iter().collect()));
    }
    for hunk in hunks.iter_mut() {
        let added: Vec<String> = arr(&hunk["added"])
            .iter()
            .map(|l| normalize(s(l)))
            .filter(|l| !l.is_empty())
            .collect();
        let Some(candidates) = by_file.get(s(&hunk["file"])) else {
            hunk["attribution"] = json!("none");
            hunk["evidence"] = json!([]);
            continue;
        };
        let mut evidence = vec![];
        let mut covered = 0;
        for line in &added {
            let mut found = false;
            for (index, lines) in candidates {
                if lines.contains(line) {
                    found = true;
                    if !evidence.contains(index) {
                        evidence.push(*index);
                    }
                }
            }
            covered += usize::from(found);
        }
        hunk["attribution"] = json!(if added.is_empty() {
            // Pure deletions: evidence is any edit to the file, but we cannot prove the lines.
            if candidates.is_empty() {
                "none"
            } else {
                "partial"
            }
        } else if covered == added.len() {
            "matched"
        } else if covered > 0 {
            "partial"
        } else {
            "none"
        });
        if added.is_empty() {
            evidence = candidates.iter().map(|(i, _)| *i).collect();
        }
        hunk["evidence"] = json!(
            evidence
                .iter()
                .map(|i| crate::history::edit_ref(&edits[*i]))
                .collect::<Vec<_>>()
        );
        hunk["edit_indices"] = json!(evidence);
    }
}

/// Identity of the diff: edit the tree and questions must be regenerated.
pub fn key(hunks: &[Value]) -> String {
    security::digest(
        &hunks
            .iter()
            .map(|h| format!("{}\n{}", s(&h["id"]), s(&h["diff"])))
            .collect::<Vec<_>>()
            .join("\n\u{1}\n"),
    )
}

pub fn hash(hunk: &Value) -> String {
    security::digest(s(&hunk["diff"]))
}

pub fn files(hunks: &[Value]) -> BTreeSet<String> {
    hunks.iter().map(|h| s(&h["file"]).to_owned()).collect()
}

pub fn find<'a>(hunks: &'a [Value], id: &str) -> Option<&'a Value> {
    hunks.iter().find(|h| h["id"] == id)
}

/// Build the full spine: parsed hunks attributed against the selected session's edits.
pub fn spine(review: &Value, edits: &[Value]) -> Vec<Value> {
    let mut hunks = working_tree(review);
    attribute(&mut hunks, edits);
    hunks
}
