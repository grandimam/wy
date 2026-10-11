//! Your verdicts. Stored apart from generated briefs so regeneration never wipes
//! your notes; flagged stale, never silently cleared, when the cited code changes.
use crate::{arr, hunks, n, s, security, steps, storage::Store};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub const STATES: [&str; 3] = ["accept", "push_back", "unsure"];

pub fn label(state: &str) -> &'static str {
    match state {
        "accept" => "accepted",
        "push_back" => "push back",
        "unsure" => "unsure",
        _ => "",
    }
}
pub fn glyph(state: &str) -> &'static str {
    match state {
        "accept" => "✓",
        "push_back" => "✗",
        "unsure" => "?",
        _ => " ",
    }
}

pub fn load(root: &Path) -> Result<BTreeMap<String, Value>> {
    Ok(Store::open(root)?
        .all("verdict")?
        .into_iter()
        .filter(|v| v["question_key"].is_string())
        .map(|v| (s(&v["question_key"]).to_owned(), v))
        .collect())
}

/// Record or update a verdict. `state` may be empty to keep the current state and
/// only change the note. The cited hunk hashes capture what the code looked like.
pub fn set(
    root: &Path,
    question: &Value,
    hunk_list: &[Value],
    state: Option<&str>,
    note: Option<&str>,
) -> Result<Value> {
    let key = crate::judgment::question_key(question);
    let store = Store::open(root)?;
    let mut verdict = store.get("verdict", &key).unwrap_or_else(|_| {
        json!({
            "question_key": key, "question": question["question"], "state": "unsure", "note": "",
            "hunk_hashes": {}, "at": crate::now()
        })
    });
    if let Some(state) = state {
        ensure!(
            STATES.contains(&state),
            "Verdict must be accept, push_back or unsure"
        );
        verdict["state"] = json!(state);
    }
    if let Some(note) = note {
        verdict["note"] = json!(security::short(&security::redact(note), 600));
    }
    verdict["at"] = json!(crate::now());
    verdict["hunk_hashes"] = json!(
        arr(&question["hunks"])
            .iter()
            .filter_map(|id| hunks::find(hunk_list, s(id))
                .map(|h| (s(id).to_owned(), json!(hunks::hash(h)))))
            .collect::<serde_json::Map<String, Value>>()
    );
    store.put("verdict", &key, &verdict)?;
    Ok(verdict)
}

/// A verdict is stale when any cited hunk's content changed after it was given.
pub fn stale(verdict: &Value, question: &Value, hunk_list: &[Value]) -> bool {
    arr(&question["hunks"]).iter().any(|id| {
        let now = hunks::find(hunk_list, s(id)).map(hunks::hash);
        let then = verdict["hunk_hashes"][s(id)].as_str().map(str::to_owned);
        then != now
    })
}

fn status_line(q: &Value) -> String {
    match s(&q["said"]["status"]) {
        "recorded" => "recorded".into(),
        "inferred" => "inferred".into(),
        _ => "agent said: nothing".into(),
    }
}

fn cite(q: &Value, hunk_list: &[Value]) -> String {
    arr(&q["hunks"])
        .iter()
        .filter_map(|id| hunks::find(hunk_list, s(id)))
        .map(|h| format!("{}:{}-{}", s(&h["file"]), n(&h["start"]), n(&h["end"])))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Markdown review written in the reviewer's voice. The push-back section is
/// phrased so it can be pasted back to the agent as the next instruction.
pub fn export(
    review: &Value,
    brief: &Value,
    step_list: &[Value],
    hunk_list: &[Value],
    verdicts: &BTreeMap<String, Value>,
    title: &str,
) -> String {
    let files = hunks::files(hunk_list).len();
    let mut out = String::new();
    out.push_str(&format!(
        "# Review: {} — uncommitted, {files} file{}, {}\n\n",
        security::short(title, 80).replace('\n', " "),
        if files == 1 { "" } else { "s" },
        crate::now().split('T').next().unwrap_or("")
    ));
    out.push_str("Reviewed with wy. Verdicts are the reviewer's. \"Recorded\" quotes are the agent's own words; \"assessment\" is wy's; \"unknown\" means no evidence was captured.\n\n");
    let questions = arr(&brief["questions"]);
    let mut groups: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for q in questions {
        let state = verdicts
            .get(s(&q["key"]))
            .map(|v| s(&v["state"]))
            .unwrap_or("");
        groups.entry(state).or_default().push(q);
    }
    for (state, heading) in [
        ("push_back", "Push back"),
        ("unsure", "Unsure"),
        ("accept", "Accepted"),
        ("", "Not reviewed"),
    ] {
        let Some(items) = groups.get(state) else {
            continue;
        };
        out.push_str(&format!("## {heading} ({})\n", items.len()));
        for q in items {
            let verdict = verdicts.get(s(&q["key"]));
            let stale_mark = verdict
                .filter(|v| stale(v, q, hunk_list))
                .map(|_| " · code changed since verdict")
                .unwrap_or("");
            if state == "push_back" || state == "unsure" {
                out.push_str(&format!("### {}\n", s(&q["question"])));
                out.push_str(&format!(
                    "{} · {}{stale_mark}\n",
                    s(&q["consequence"]).to_uppercase(),
                    status_line(q)
                ));
                out.push_str(&format!(
                    "What the agent did: {} ({}).\n",
                    s(&q["chosen"]),
                    cite(q, hunk_list)
                ));
                if !s(&q["consequence_note"]).is_empty() {
                    out.push_str(&format!("Why it matters: {}\n", s(&q["consequence_note"])));
                }
                if q["said"]["status"] == "recorded" {
                    out.push_str(&format!(
                        "Agent said: \"{}\"\n",
                        security::short(s(&q["said"]["quote"]), 400).trim()
                    ));
                }
                if let Some(v) = verdict
                    && !s(&v["note"]).is_empty()
                {
                    out.push_str(&format!("Reviewer: {}\n", s(&v["note"])));
                }
                if state == "push_back" && !s(&q["expert_view"]).is_empty() {
                    out.push_str(&format!("Ask: {}\n", s(&q["expert_view"])));
                }
                out.push('\n');
            } else {
                let said = if q["said"]["status"] == "recorded" {
                    format!(
                        " — recorded: \"{}\"",
                        security::short(s(&q["said"]["quote"]), 120)
                            .trim()
                            .replace('\n', " ")
                    )
                } else {
                    format!(" — {}", status_line(q))
                };
                let note = verdict
                    .map(|v| s(&v["note"]))
                    .filter(|t| !t.is_empty())
                    .map(|t| format!(" · {t}"))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "- {} — {}{said}{note}{stale_mark}\n",
                    s(&q["question"]),
                    s(&q["consequence"]).to_uppercase()
                ));
            }
        }
        out.push('\n');
    }
    let questioned: Vec<&str> = questions
        .iter()
        .flat_map(|q| arr(&q["hunks"]).iter().map(s))
        .collect();
    let unquestioned: Vec<String> = hunk_list
        .iter()
        .filter(|h| !questioned.contains(&s(&h["id"])))
        .map(|h| {
            let tag = if h["derived"] == true {
                "derived"
            } else if h["attribution"] == "none" {
                "no captured evidence"
            } else {
                "not questioned"
            };
            format!(
                "{}:{}-{} ({tag})",
                s(&h["file"]),
                n(&h["start"]),
                n(&h["end"])
            )
        })
        .collect();
    if !unquestioned.is_empty() {
        out.push_str("## Not in any question\n");
        for line in unquestioned.iter().take(40) {
            out.push_str(&format!("- {line}\n"));
        }
        if unquestioned.len() > 40 {
            out.push_str(&format!("- …and {} more\n", unquestioned.len() - 40));
        }
        out.push('\n');
    }
    if !arr(&brief["rejected"]).is_empty() {
        out.push_str(&format!(
            "{} generated question(s) were rejected because their citations did not check out.\n\n",
            arr(&brief["rejected"]).len()
        ));
    }
    if !step_list.is_empty() {
        out.push_str("## Agent's account (for reference)\n");
        for st in step_list {
            let time = st["started"]
                .as_str()
                .and_then(|t| t.get(11..16))
                .unwrap_or("--:--");
            let why = arr(&st["why"]["recorded"])
                .first()
                .map(|r| {
                    format!(
                        " — \"{}\"",
                        security::short(s(&r["quote"]), 100)
                            .trim()
                            .replace('\n', " ")
                    )
                })
                .unwrap_or_else(|| format!(" — {}", steps::status_label(st)));
            let checks: Vec<String> = arr(&st["checked"])
                .iter()
                .map(|c| {
                    format!(
                        "{} {}",
                        if c["outcome"] == "ok" {
                            "✓"
                        } else if c["outcome"] == "failed" {
                            "✗"
                        } else {
                            "·"
                        },
                        s(&c["kind"])
                    )
                })
                .collect();
            let gaps = if arr(&st["gaps"]).is_empty() {
                String::new()
            } else {
                format!(" ⚠ {}", arr(&st["gaps"]).len())
            };
            out.push_str(&format!(
                "{} {time} {}{why}{}{gaps}\n",
                n(&st["ordinal"]) + 1,
                s(&st["title"]),
                if checks.is_empty() {
                    String::new()
                } else {
                    format!(" {}", checks.join(" "))
                }
            ));
        }
        out.push('\n');
    }
    let _ = review;
    security::redact(&out)
}
