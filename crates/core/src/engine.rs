use crate::{graph::Graph, model::*, search::Search, storage::Store};
use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};

pub struct Engine { pub graph: Graph, pub store: Store, search: Search, pub cache_hits: u64, pub cache_misses: u64, search_dirty: bool, search_rebuild: bool }
#[derive(Serialize)]
pub struct Stats { pub nodes: usize, pub edges: usize, pub graph_estimated_bytes: usize, pub backend_rss_bytes: Option<u64>, pub cache_hit_rate: f64 }
impl Engine {
    pub fn open(path: Option<PathBuf>, budget: usize) -> Result<Self> {
        let store = Store::open(path.as_deref())?; let mut graph = Graph::new(budget, path.is_none()); let mut search = Search::new()?;
        let mut cursor = 0;
        loop {
            let rows = store.page("nodes", cursor, 256)?; if rows.is_empty() { break; }
            let mut batch = Batch::default();
            for (row, text) in rows { cursor = row; let n: Node = serde_json::from_str(&text)?; search.upsert(&n)?; batch.nodes.push(n); }
            graph.admit(&batch)?; graph.apply(batch);
        }
        cursor = 0;
        loop {
            let rows = store.page("edges", cursor, 512)?; if rows.is_empty() { break; }
            let mut batch = Batch::default(); for (row,text) in rows { cursor=row; batch.edges.push(serde_json::from_str(&text)?); }
            graph.admit(&batch)?; graph.apply(batch);
        }
        search.commit()?;
        Ok(Self { graph, store, search, cache_hits: 0, cache_misses: 0, search_dirty: false, search_rebuild:false })
    }
    pub fn apply(&mut self, mut batch: Batch) -> Result<Batch> {
        batch.validate()?;
        // Coalesce within the transaction, then merge against durable truth.
        let mut merged = std::collections::BTreeMap::<String,Node>::new();
        for n in batch.nodes.drain(..) {
            if let Some(old) = merged.get_mut(&n.id) { old.merge(n); }
            else { let mut old = self.store.node(&n.id)?.unwrap_or_else(|| n.clone()); old.merge(n); merged.insert(old.id.clone(), old); }
        }
        batch.nodes = merged.into_values().collect(); self.graph.admit(&batch)?;
        self.store.commit(&batch)?; self.graph.apply(batch.clone());
        for node in &batch.nodes { if self.search.upsert(node).is_err(){self.search_rebuild=true;break;} }
        self.search_dirty = true;
        Ok(batch)
    }
    pub fn search(&mut self, q: &str) -> Result<Vec<String>> {
        if self.search_rebuild {
            // Recover a failed indexing operation from committed SQLite truth.
            let mut cursor = 0;
            loop { let rows=self.store.page("nodes",cursor,256)?; if rows.is_empty(){break} for(row,s)in rows{cursor=row;self.search.upsert(&serde_json::from_str::<Node>(&s)?)?;} }
            self.search_rebuild=false;
        }
        if self.search_dirty { self.search.commit()?; self.search_dirty=false; }
        self.search.find(q, 500)
    }
    pub fn node(&mut self, id: &str) -> Result<Option<Node>> { if let Some(n) = self.graph.get_hot(id) { return Ok(Some(n)); } let n=self.store.node(id)?;if let Some(n)=&n {let batch=Batch{nodes:vec![n.clone()],edges:vec![]};if self.graph.admit(&batch).is_ok(){self.graph.apply(batch);}}Ok(n) }
    pub fn stats(&self) -> Stats {
        let total=self.cache_hits+self.cache_misses;
        Stats { nodes:self.graph.topology.node_count(), edges:self.graph.topology.edge_count(), graph_estimated_bytes:self.graph.estimated_bytes(), backend_rss_bytes:backend_rss(), cache_hit_rate:if total==0{0.0}else{self.cache_hits as f64/total as f64} }
    }
}
pub fn backend_rss() -> Option<u64> {
    #[cfg(target_os="linux")] { let s=std::fs::read_to_string("/proc/self/status").ok()?; let line=s.lines().find(|l|l.starts_with("VmRSS:"))?; return line.split_whitespace().nth(1)?.parse::<u64>().ok().map(|n|n*1024); }
    #[cfg(not(target_os="linux"))] { None }
}
type Work = Box<dyn FnOnce(&mut Engine) + Send>;
#[derive(Clone)]
pub struct EngineHandle { tx: mpsc::Sender<Work> }
impl EngineHandle {
    pub async fn start(path: Option<PathBuf>, budget: usize) -> Result<Self> {
        let (tx, mut rx)=mpsc::channel::<Work>(32); let (ready_tx,ready_rx)=oneshot::channel();
        std::thread::Builder::new().name("graph-store".into()).spawn(move || {
            match Engine::open(path,budget) {
                Ok(mut engine)=>{let _=ready_tx.send(Ok(()));while let Some(f)=rx.blocking_recv(){f(&mut engine);}},
                Err(e)=>{let _=ready_tx.send(Err(e));}
            }
        })?;
        ready_rx.await??; Ok(Self{tx})
    }
    pub async fn call<T:Send+'static>(&self, f:impl FnOnce(&mut Engine)->Result<T>+Send+'static)->Result<T>{
        let(tx,rx)=oneshot::channel();self.tx.send(Box::new(move|e|{let _=tx.send(f(e));})).await.map_err(|_|anyhow::anyhow!("engine stopped"))?;rx.await?
    }
}
