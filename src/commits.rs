//! Local associations between Git commits and saved conversation snapshots.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path};

use crate::{arr, history, repository, s, storage::Store};

pub fn recent(path: &Path) -> Result<Vec<Value>> {
    let root = repository::root(path)?;
    if repository::head(&root).is_none() {
        return Ok(vec![]);
    }
    let log = repository::git(
        &root,
        &["log", "-50", "--format=%H%x00%h%x00%s", "HEAD", "--"],
        true,
    )?;
    Ok(log.lines().filter_map(|line| {
        let mut fields = line.splitn(3, '\0');
        Some(json!({"commit":fields.next()?,"short":fields.next()?,"subject":crate::security::redact(fields.next()?)}))
    }).collect())
}

fn review(store: &Store, root: &Path, id: &str) -> Result<Value> {
    let review = store.get("review", id)?;
    crate::validate("Review", &review)?;
    ensure!(
        s(&review["root"]) == root.to_string_lossy(),
        "Saved review belongs to another repository"
    );
    Ok(review)
}

fn sessions(store: &Store, root: &Path, review: &Value) -> Result<Vec<(String, Value)>> {
    let mut refs = arr(&review["sessions"]).to_vec();
    // Preserve support for reviews saved before multi-session references existed.
    if refs.is_empty() && review["session_id"].is_string() {
        refs.push(json!({"id":review["session_id"],"storage_key":review["session_id"]}));
    }
    let mut sessions = vec![];
    for reference in refs {
        let key = s(&reference["storage_key"]);
        let session = store.get("session", key).with_context(|| {
            format!(
                "Saved conversation snapshot is missing for review {}",
                s(&review["id"])
            )
        })?;
        crate::validate("Session", &session)?;
        ensure!(
            history::belongs(s(&session["cwd"]), root)
                && session["id"] == reference["id"]
                && (reference["agent"].is_null() || session["agent"] == reference["agent"]),
            "Saved conversation does not match this repository or review"
        );
        sessions.push((key.to_owned(), session));
    }
    Ok(sessions)
}

pub fn link(path: &Path, revision: &str, review_id: Option<&str>) -> Result<Value> {
    let root = repository::root(path)?;
    let commit = repository::resolve_commit(&root, revision)?;
    let store = Store::open(&root)?;
    let review = review(&store, &root, review_id.unwrap_or("latest"))?;
    let sessions = sessions(&store, &root, &review)?;
    ensure!(
        !sessions.is_empty(),
        "Review has no saved conversations; refresh with r so agent history is captured first"
    );
    // Pin the concrete review ID, never the mutable 'latest' alias or its HEAD.
    let id = s(&review["id"]);
    store.link_review(&commit, id, "explicit")?;
    Ok(
        json!({"commit":commit,"review_id":id,"session_count":sessions.len(),"association":"explicit"}),
    )
}

fn match_reviews(root: &Path, store: &Store, commit: &str) -> Result<()> {
    let parents = repository::git(root, &["rev-list", "--parents", "-n", "1", commit], true)?;
    let parents: Vec<_> = parents.split_whitespace().skip(1).collect();
    // A merge has multiple possible review bases; require an explicit association.
    if parents.len() > 1 {
        return Ok(());
    }
    let reviews: Vec<_> = store
        .reviews_at_base(parents.first().copied())?
        .into_iter()
        .filter(|review| {
            s(&review["root"]) == root.to_string_lossy()
                && !arr(&review["changes"]).is_empty()
                && (!arr(&review["sessions"]).is_empty() || review["session_id"].is_string())
        })
        .collect();
    if reviews.is_empty() {
        return Ok(());
    }
    let hashes = json!(repository::committed_sources(root, commit)?.hashes);
    for review in reviews {
        crate::validate("Review", &review)?;
        // Compare the complete captured source snapshot, including unchanged files.
        // Matching just HEAD would attach the conversation to the preceding commit.
        if review["file_hashes"] == hashes {
            sessions(store, root, &review)?;
            store.link_review(commit, s(&review["id"]), "snapshot-match")?;
        }
    }
    Ok(())
}

pub fn lookup(path: &Path, revision: &str, source: &str) -> Result<Value> {
    history::valid_source(source)?;
    let root = repository::root(path)?;
    let commit = repository::resolve_commit(&root, revision)?;
    let store = Store::open(&root)?;
    match_reviews(&root, &store, &commit)?;
    let links = store.linked_reviews(&commit)?;
    ensure!(
        !links.is_empty(),
        "No saved conversations linked to commit {commit}. Use /link {commit} [review-id] to attach a saved review."
    );
    let mut saved: Vec<Value> = vec![];
    let mut positions = HashMap::new();
    for link in &links {
        let id = s(&link["review_id"]);
        let review = review(&store, &root, id)?;
        for (key, mut session) in sessions(&store, &root, &review)? {
            if source != "both" && session["agent"] != source {
                continue;
            }
            if let Some(&index) = positions.get(&key) {
                let existing: &mut Value = &mut saved[index];
                existing["review_ids"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(id));
            } else {
                positions.insert(key.clone(), saved.len());
                let context = session.clone();
                for event in session["events"].as_array_mut().unwrap() {
                    let mut evidence = history::event_evidence(&context, event);
                    history::origins::enrich(&root, &mut evidence)?;
                    event["provenance"] = evidence["provenance"].clone();
                    for field in ["origin_status", "origin_reason", "originals"] {
                        if let Some(value) = evidence.get(field) {
                            event[field] = value.clone();
                        }
                    }
                }
                session["storage_key"] = json!(key);
                session["review_ids"] = json!([id]);
                saved.push(session);
            }
        }
    }
    Ok(json!({"commit":commit,"links":links,"sessions":saved}))
}

