//! Disk-backed request pages. Only summaries and the selected request are read.
use crate::{arr, n, s, session_work, storage::Store};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::path::Path;

pub const PAGE_SIZE: usize = 20;

/// Materialize an immutable work snapshot once. Content is never shortened here.
/// The importer still owns source validation and capture budgets.
pub fn index(root: &Path, work: &Value) -> Result<()> {
    let key = s(&work["session"]["storage_key"]);
    ensure!(!key.is_empty(), "No selected session");
    let store = Store::open(root)?;
    store.index_session_work(key, work)
}

pub fn page(root: &Path, key: &str, selected: usize) -> Result<Value> {
    let store = Store::open(root)?;
    let mut work = store.session_work_header(key)?;
    ensure!(s(&work["root"]) == root.to_string_lossy(), "Request index belongs to another repository");
    let count = n(&work["request_count"]);
    let selected = selected.min(count.saturating_sub(1));
    let first = selected / PAGE_SIZE * PAGE_SIZE;
    work["request_page"] = json!(store.session_request_labels(key, first, PAGE_SIZE)?);
    work["request_page_start"] = json!(first);
    work["selected_request"] = json!(selected);
    work["indexed_reader"] = json!(true);
    if count > 0 {
        let detail = store.session_request(key, selected)?;
        work["turns"] = json!([detail["turn"].clone()]);
        work["edits"] = detail["edits"].clone();
    } else {
        work["turns"] = json!([]);
        work["edits"] = json!([]);
    }
    Ok(work)
}

/// Older snapshots migrate on demand, without loading other sessions.
pub fn ensure_index(root: &Path, key: &str, review: &Value) -> Result<()> {
    let store = Store::open(root)?;
    if store.has_session_work(key)? { return Ok(()); }
    let session = store.get("session", key)?;
    crate::validate("Session", &session)?;
    ensure!(crate::history::belongs(s(&session["cwd"]), root) && session_work::snapshot_key(&session) == key,
        "Session snapshot is not verifiably scoped to this repository");
    index(root, &session_work::build(review, &session))
}

pub fn count(work: &Value) -> usize {
    work["request_count"].as_u64().map(|v| v as usize).unwrap_or_else(|| arr(&work["turns"]).len())
}
