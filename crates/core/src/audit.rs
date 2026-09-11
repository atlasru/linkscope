use anyhow::Result;
use serde::Serialize;
use std::{fs::OpenOptions, io::Write, path::PathBuf};

pub struct Audit { directory: Option<PathBuf> }
#[derive(Serialize)]
struct Entry<'a> { timestamp: i64, event: &'a str, count: usize }
impl Audit {
    pub fn new(directory: Option<PathBuf>) -> Self { Self{directory} }
    // Deliberately accepts no arbitrary URL, error body, entity or secret.
    pub fn record(&self,event:&str,count:usize)->Result<()> {
        let Some(dir)=&self.directory else{return Ok(())};std::fs::create_dir_all(dir)?;
        let path=dir.join("events.jsonl");
        if path.metadata().map(|m|m.len()>2*1024*1024).unwrap_or(false){
            let old=dir.join("events.1.jsonl");if old.exists(){std::fs::remove_file(&old)?;}std::fs::rename(&path,old)?;
        }
        let mut file=OpenOptions::new().create(true).append(true).open(path)?;
        serde_json::to_writer(&mut file,&Entry{timestamp:crate::model::now(),event,count})?;file.write_all(b"\n")?;Ok(())
    }
}

