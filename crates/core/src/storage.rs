use crate::model::*;
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::{File, OpenOptions}, io::{BufRead, BufReader, Write}, path::Path};

#[derive(Serialize, Deserialize)]
struct Record { id: String, checksum: String, batch: Batch }
pub struct Store { pub db: Connection, log: Option<File>, poisoned: bool, _lock: Option<File> }
impl Store {
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let (db, log, lock) = if let Some(path) = path {
            std::fs::create_dir_all(path)?;
            #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?; }
            let lock=OpenOptions::new().create(true).read(true).write(true).open(path.join("session.lock"))?;
            fs2::FileExt::try_lock_exclusive(&lock).context("investigation is already open in another process")?;
            (Connection::open(path.join("graph.sqlite"))?, Some(OpenOptions::new().create(true).read(true).append(true).open(path.join("transactions.jsonl"))?), Some(lock))
        } else { (Connection::open_in_memory()?, None, None) };
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA cache_size=-8192;
          CREATE TABLE IF NOT EXISTS nodes(id TEXT PRIMARY KEY, data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS edges(id TEXT PRIMARY KEY, source TEXT NOT NULL REFERENCES nodes(id), target TEXT NOT NULL REFERENCES nodes(id), data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS applied(id TEXT PRIMARY KEY);
          CREATE TABLE IF NOT EXISTS cache(key TEXT PRIMARY KEY, body BLOB NOT NULL, expires INTEGER NOT NULL, created INTEGER NOT NULL);
          CREATE INDEX IF NOT EXISTS cache_expiry ON cache(expires);")?;
        let mut s = Self { db, log, poisoned: false, _lock:lock }; s.recover()?; Ok(s)
    }
    fn apply_record(&mut self, r: &Record) -> Result<()> {
        let tx = self.db.transaction()?;
        if tx.query_row("SELECT 1 FROM applied WHERE id=?", [&r.id], |_| Ok(())).optional()?.is_some() { return Ok(()); }
        for n in &r.batch.nodes { tx.execute("INSERT INTO nodes VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data", params![n.id, serde_json::to_string(n)?])?; }
        for e in &r.batch.edges { tx.execute("INSERT OR IGNORE INTO edges VALUES (?1,?2,?3,?4)", params![e.id,e.source,e.target,serde_json::to_string(e)?])?; }
        tx.execute("INSERT INTO applied VALUES (?)", [&r.id])?; tx.commit()?; Ok(())
    }
    fn recover(&mut self) -> Result<()> {
        let Some(log) = &self.log else { return Ok(()); };
        let mut reader = BufReader::new(log.try_clone()?);
        let mut offset = 0u64;
        loop {
            let mut line = Vec::new();
            let len = std::io::Read::by_ref(&mut reader).take(4 * 1024 * 1024).read_until(b'\n', &mut line)?;
            if len == 0 { break; }
            if line.last() != Some(&b'\n') {
                if len >= 4 * 1024 * 1024 { bail!("oversized transaction log record"); }
                self.log.as_ref().unwrap().set_len(offset)?; break;
            }
            let record: Record = serde_json::from_slice(&line).context("corrupt transaction log; recovery stopped")?;
            if format!("{:x}", Sha256::digest(serde_json::to_vec(&record.batch)?)) != record.checksum { bail!("transaction checksum mismatch"); }
            record.batch.validate()?; self.apply_record(&record)?; offset += len as u64;
        }
        Ok(())
    }
    pub fn commit(&mut self, batch: &Batch) -> Result<()> {
        if self.poisoned { bail!("storage requires restart/recovery after a failed commit"); }
        batch.validate()?;
        let r = Record { id: uuid::Uuid::new_v4().to_string(), checksum: format!("{:x}", Sha256::digest(serde_json::to_vec(batch)?)), batch: batch.clone() };
        // Poison before any durable side effect. Do not accept more writes after
        // an ambiguous fsync/SQLite failure; restart replays the idempotent intent.
        self.poisoned = true;
        if let Some(log) = &mut self.log { let mut bytes = serde_json::to_vec(&r)?; bytes.push(b'\n'); log.write_all(&bytes)?; log.sync_data()?; }
        self.apply_record(&r)?; self.poisoned = false; Ok(())
    }
    pub fn node(&self, id: &str) -> Result<Option<Node>> {
        self.db.query_row("SELECT data FROM nodes WHERE id=?", [id], |r| r.get::<_,String>(0)).optional()?.map(|s| serde_json::from_str(&s).map_err(Into::into)).transpose()
    }
    pub fn page(&self, table: &str, after: i64, limit: usize) -> Result<Vec<(i64, String)>> {
        if table != "nodes" && table != "edges" { bail!("invalid table"); }
        let mut stmt = self.db.prepare(&format!("SELECT rowid,data FROM {table} WHERE rowid>? ORDER BY rowid LIMIT ?"))?;
        let rows = stmt.query_map(params![after,limit.min(512)], |r| Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?; Ok(rows)
    }
    pub fn cache_get(&self, key: &str) -> Result<Option<Vec<u8>>> { Ok(self.db.query_row("SELECT body FROM cache WHERE key=? AND expires>?", params![key,now()], |r| r.get(0)).optional()?) }
    pub fn cache_put(&mut self, key: &str, body: &[u8], ttl: i64) -> Result<()> {
        if body.len() > 2 * 1024 * 1024 { bail!("cache item too large"); }
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM cache WHERE expires<=?", [now()])?;
        let mut size: i64 = tx.query_row("SELECT COALESCE(SUM(length(body)),0) FROM cache", [], |r| r.get(0))?;
        while size + body.len() as i64 > 32 * 1024 * 1024 {
            tx.execute("DELETE FROM cache WHERE key IN (SELECT key FROM cache ORDER BY created LIMIT 8)", [])?;
            size = tx.query_row("SELECT COALESCE(SUM(length(body)),0) FROM cache", [], |r| r.get(0))?;
        }
        tx.execute("INSERT OR REPLACE INTO cache VALUES (?1,?2,?3,?4)", params![key,body,now()+ttl.clamp(1,86400),now()])?; tx.commit()?; Ok(())
    }
}
use std::io::Read;
