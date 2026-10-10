use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::{fs::{self, OpenOptions},io::Write,path::PathBuf,time::Duration};

mod session_index;

pub struct Store { connection: Connection, directory: PathBuf }
impl Store {
    pub fn open(root: &std::path::Path) -> Result<Self> {
        let directory=root.join(".wy");
        ensure!(!directory.is_symlink(),"Refusing symlinked .wy storage");
        if !directory.exists() {
            let mut builder=fs::DirBuilder::new();
            #[cfg(unix)] {use std::os::unix::fs::DirBuilderExt;builder.mode(0o700);}
            builder.create(&directory)?;
        }
        let database=directory.join("wy.sqlite3");
        ensure!(!database.is_symlink(),"Refusing symlinked database");
        let mut opts=OpenOptions::new();opts.create(true).write(true);
        #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);}
        opts.open(&database)?;
        let connection=Connection::open(&database)?;
        connection.busy_timeout(Duration::from_secs(10))?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS artifacts (kind TEXT, id TEXT, data TEXT, PRIMARY KEY(kind,id));
            CREATE TABLE IF NOT EXISTS commit_reviews (commit_hash TEXT NOT NULL, review_id TEXT NOT NULL, created_at TEXT NOT NULL, association TEXT NOT NULL, PRIMARY KEY(commit_hash,review_id));
            CREATE TABLE IF NOT EXISTS session_work_headers (session_key TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS session_requests (session_key TEXT NOT NULL, ordinal INTEGER NOT NULL, request_id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(session_key,ordinal));
            PRAGMA user_version=3;")?;
        Ok(Self{connection,directory})
    }
    pub fn get(&self, kind: &str, id: &str) -> Result<Value> {
        let raw:Option<String>=self.connection.query_row("SELECT data FROM artifacts WHERE kind=? AND id=?",params![kind,id],|r|r.get(0)).optional()?;
        serde_json::from_str(&raw.with_context(||format!("No {kind} named {id}"))?).context("Invalid stored artifact")
    }
    pub fn recent(&self, kind: &str, limit: usize) -> Result<Vec<Value>> {
        let mut query = self.connection.prepare("SELECT data FROM artifacts WHERE kind=? AND id<>'latest' ORDER BY rowid DESC LIMIT ?")?;
        let rows = query.query_map(params![kind, limit.min(100) as i64], |r| r.get::<_, String>(0))?;
        let mut result = vec![];
        for row in rows { result.push(serde_json::from_str(&row?)?); }
        Ok(result)
    }
    pub fn put(&self, kind: &str, id: &str, data: &Value) -> Result<()> {
        self.connection.execute("INSERT OR REPLACE INTO artifacts VALUES (?,?,?)",params![kind,id,serde_json::to_string(data)?])?;Ok(())
    }
    pub fn session_snapshots(&self, agent: &str, session: &str) -> Result<Vec<(String,Value)>> {
        let mut query=self.connection.prepare("SELECT id,data FROM artifacts WHERE kind='session' AND json_extract(data,'$.agent')=? AND json_extract(data,'$.id')=? ORDER BY rowid DESC LIMIT 21")?;
        let rows=query.query_map(params![agent,session],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?;
        let mut result=vec![];let mut bytes=0;
        for row in rows{let (key,raw)=row?;bytes+=raw.len();ensure!(result.len()<20&&bytes<=40_000_000,"Original turn lookup exceeds the saved-history capture limit");result.push((key,serde_json::from_str(&raw)?));}
        Ok(result)
    }
    pub fn link_review(&self, commit: &str, review_id: &str, association: &str) -> Result<()> {
        self.connection.execute("INSERT INTO commit_reviews VALUES (?,?,?,?) ON CONFLICT(commit_hash,review_id) DO UPDATE SET association='explicit' WHERE excluded.association='explicit'",params![commit,review_id,crate::now(),association])?;
        Ok(())
    }
    pub fn linked_reviews(&self, commit: &str) -> Result<Vec<Value>> {
        let mut query=self.connection.prepare("SELECT review_id,association FROM commit_reviews WHERE commit_hash=? ORDER BY created_at,review_id")?;
        Ok(query.query_map([commit],|r|Ok(serde_json::json!({"review_id":r.get::<_,String>(0)?,"association":r.get::<_,String>(1)?})))?.collect::<rusqlite::Result<Vec<Value>>>()?)
    }
    pub fn reviews_at_base(&self, base: Option<&str>) -> Result<Vec<Value>> {
        let mut query=self.connection.prepare("SELECT data FROM artifacts WHERE kind='review' AND id<>'latest' AND json_extract(data,'$.head') IS ? AND json_extract(data,'$.comparison_base') IS ? AND json_extract(data,'$.baseline_id') IS NULL ORDER BY rowid")?;
        let rows=query.query_map(params![base,base],|r|r.get::<_,String>(0))?;
        let mut reviews=vec![];
        for row in rows{reviews.push(serde_json::from_str(&row?)?);}
        Ok(reviews)
    }
    pub fn save_review(&mut self, review: &Value) -> Result<()> {
        crate::validate("Review",review)?;
        let raw=serde_json::to_string(review)?;
        let tx=self.connection.transaction()?;
        for id in [crate::s(&review["id"]),"latest"] {
            tx.execute("INSERT OR REPLACE INTO artifacts VALUES ('review',?,?)",params![id,raw])?;
        }
        tx.commit()?;
        let mut temp=tempfile::NamedTempFile::new_in(&self.directory)?;
        temp.write_all(serde_json::to_string_pretty(review)?.as_bytes())?;
        temp.as_file().sync_all()?;
        temp.persist(self.directory.join("review.json"))?;
        Ok(())
    }
}
