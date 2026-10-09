use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::{fs::{self, OpenOptions},io::Write,path::PathBuf,time::Duration};

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
        connection.execute_batch("CREATE TABLE IF NOT EXISTS artifacts (kind TEXT, id TEXT, data TEXT, PRIMARY KEY(kind,id)); PRAGMA user_version=1;")?;
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
