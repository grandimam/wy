use super::Store;
use crate::{arr, n, s};
use anyhow::Result;
use rusqlite::params;
use serde_json::{Value, json};

impl Store {
    /// Read headers only; never deserialize every saved transcript to populate navigation.
    pub fn saved_session_catalog(&self) -> Result<(Vec<Value>, usize)> {
        let mut query = self.connection.prepare("SELECT id, json_extract(data,'$.id'), json_extract(data,'$.agent'), json_extract(data,'$.cwd'), json_extract(data,'$.path'), json_extract(data,'$.events[#-1].timestamp'), json_extract(data,'$.started_at') FROM artifacts WHERE kind='session' ORDER BY rowid DESC")?;
        let rows = query.query_map([], |r| Ok(json!({
            "storage_key":r.get::<_,String>(0)?, "id":r.get::<_,Option<String>>(1)?,
            "agent":r.get::<_,Option<String>>(2)?, "cwd":r.get::<_,Option<String>>(3)?,
            "path":r.get::<_,Option<String>>(4)?, "last_event":r.get::<_,Option<String>>(5)?,
            "source_timestamp":r.get::<_,Option<String>>(6)?
        })))?;
        let mut seen = std::collections::HashSet::new();
        let mut result = vec![];
        let mut skipped = 0;
        for row in rows {
            let row = row?;
            // Older stores may contain sessions without an agent (or other required
            // header fields). Do not let one such artifact invalidate the entire
            // Review, or hide an older valid snapshot with the same identity.
            if !crate::history::AGENTS.contains(&s(&row["agent"]))
                || ["id", "cwd", "path"].iter().any(|field| s(&row[*field]).is_empty())
            {
                skipped += 1;
                continue;
            }
            if seen.insert((s(&row["agent"]).to_owned(), s(&row["id"]).to_owned())) { result.push(row); }
        }
        Ok((result, skipped))
    }
    pub fn has_session_work(&self, key: &str) -> Result<bool> {
        Ok(self.connection.query_row("SELECT EXISTS(SELECT 1 FROM session_work_headers WHERE session_key=?)", [key], |r| r.get(0))?)
    }
    pub fn index_session_work(&self, key: &str, work: &Value) -> Result<()> {
        if self.has_session_work(key)? { return Ok(()); }
        let tx = self.connection.unchecked_transaction()?;
        let header = json!({"scope_kind":"session", "root":work["root"], "id":work["id"],
            "session":work["session"], "warnings":work["warnings"], "request_count":arr(&work["turns"]).len()});
        for (ordinal, turn) in arr(&work["turns"]).iter().enumerate() {
            let edits: Vec<_> = arr(&turn["edit_indices"]).iter()
                .filter_map(|index| arr(&work["edits"]).get(n(index))).cloned().collect();
            let mut local = turn.clone();
            local["edit_indices"] = json!((0..edits.len()).collect::<Vec<_>>());
            let data = json!({"turn":local,"edits":edits});
            tx.execute("INSERT INTO session_requests(session_key,ordinal,request_id,data) VALUES (?,?,?,?)",
                params![key, ordinal as i64, s(&turn["request"]["id"]), serde_json::to_string(&data)?])?;
        }
        tx.execute("INSERT INTO session_work_headers(session_key,data) VALUES (?,?)", params![key, serde_json::to_string(&header)?])?;
        tx.commit()?;
        Ok(())
    }
    pub fn session_work_header(&self, key: &str) -> Result<Value> {
        let raw: String = self.connection.query_row("SELECT data FROM session_work_headers WHERE session_key=?", [key], |r| r.get(0))?;
        Ok(serde_json::from_str(&raw)?)
    }
    pub fn session_request(&self, key: &str, ordinal: usize) -> Result<Value> {
        let raw: String = self.connection.query_row("SELECT data FROM session_requests WHERE session_key=? AND ordinal=?", params![key, ordinal as i64], |r| r.get(0))?;
        Ok(serde_json::from_str(&raw)?)
    }
    pub fn session_request_labels(&self, key: &str, first: usize, count: usize) -> Result<Vec<Value>> {
        let mut query = self.connection.prepare("SELECT ordinal,request_id FROM session_requests WHERE session_key=? AND ordinal>=? ORDER BY ordinal LIMIT ?")?;
        Ok(query.query_map(params![key, first as i64, count as i64], |r| {
            Ok(json!({"ordinal":r.get::<_,i64>(0)?, "request":{"id":r.get::<_,String>(1)?}}))
        })?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}
