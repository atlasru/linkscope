use crate::model::*;
use anyhow::{bail, Result};
use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use std::collections::{HashMap, HashSet};

pub struct HotNode { pub id: String, pub payload: Option<Node>, pub touched: i64, payload_bytes: usize }
pub struct Graph {
    pub topology: StableDiGraph<HotNode, String>,
    pub ids: HashMap<String, NodeIndex>,
    edge_ids: HashSet<String>,
    pub budget: usize,
    ephemeral: bool,
    estimated: usize,
}
impl Graph {
    pub fn new(budget: usize, ephemeral: bool) -> Self { Self { topology: StableDiGraph::new(), ids: HashMap::new(), edge_ids: HashSet::new(), budget, ephemeral, estimated: 0 } }
    pub fn estimated_bytes(&self) -> usize {
        self.estimated
    }
    pub fn admit(&mut self, batch: &Batch) -> Result<()> {
        batch.validate()?;
        for e in &batch.edges {
            for id in [&e.source, &e.target] { if !self.ids.contains_key(id) && !batch.nodes.iter().any(|n| &n.id == id) { bail!("dangling edge"); } }
        }
        let needed = serde_json::to_vec(batch)?.len() * 4 + batch.nodes.len() * 320 + batch.edges.len() * 200;
        if self.estimated_bytes() + needed > self.budget && !self.ephemeral {
            let mut cold: Vec<_> = self.topology.node_indices().map(|i| (self.topology[i].touched, i)).collect();
            cold.sort_unstable_by_key(|v| v.0);
            let mut current = self.estimated_bytes();
            for (_, i) in cold {
                if current + needed <= self.budget { break; }
                if self.topology[i].payload.take().is_some() { current = current.saturating_sub(self.topology[i].payload_bytes); self.topology[i].payload_bytes=0; self.estimated=current; }
            }
        }
        if self.estimated_bytes() + needed > self.budget { bail!("graph budget reached; reduce scope or raise budget"); }
        Ok(())
    }
    pub fn apply(&mut self, batch: Batch) {
        for n in batch.nodes {
            let id = n.id.clone();
            let payload_bytes=serde_json::to_vec(&n).map_or(0,|v|v.len()*3);
            if let Some(&i) = self.ids.get(&id) { self.estimated=self.estimated.saturating_sub(self.topology[i].payload_bytes)+payload_bytes; self.topology[i].payload_bytes=payload_bytes;self.topology[i].payload = Some(n); self.topology[i].touched = now(); }
            else { self.estimated+=320+payload_bytes;let i = self.topology.add_node(HotNode { id: id.clone(), payload: Some(n), touched: now(),payload_bytes }); self.ids.insert(id, i); }
        }
        for e in batch.edges { if self.edge_ids.insert(e.id.clone()) { self.estimated+=200;self.topology.add_edge(self.ids[&e.source], self.ids[&e.target], e.id); } }
    }
    pub fn get_hot(&mut self, id: &str) -> Option<Node> { let i = *self.ids.get(id)?; self.topology[i].touched = now(); self.topology[i].payload.clone() }
}
