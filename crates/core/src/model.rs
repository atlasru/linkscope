use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence { #[default] Data, Information, Intelligence, CounterIntelligence }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: String,
    pub value: String,
    #[serde(default)] pub attributes: BTreeMap<String, serde_json::Value>,
    #[serde(default)] pub sources: Vec<String>,
    #[serde(default)] pub confidence: Confidence,
    pub timestamp: i64,
    pub x: f32,
    pub y: f32,
}
impl Node {
    pub fn new(kind: &str, value: &str, source: &str) -> Self {
        let value = match kind { "domain" | "email" => value.trim().trim_end_matches('.').to_lowercase(), _ => value.trim().to_owned() };
        let id = Uuid::new_v5(&Uuid::NAMESPACE_URL, format!("{kind}:{value}").as_bytes()).to_string();
        let bytes = Uuid::parse_str(&id).unwrap().as_u128();
        Self { id, kind: kind.into(), value, attributes: BTreeMap::new(), sources: vec![source.into()], confidence: Confidence::Data, timestamp: now(), x: ((bytes & 65535) as f32 / 65535.0 - 0.5) * 1000.0, y: (((bytes >> 16) & 65535) as f32 / 65535.0 - 0.5) * 700.0 }
    }
    pub fn validate(&self) -> Result<()> {
        if self.id.len() > 64 || Uuid::parse_str(&self.id).is_err() || self.value.is_empty() || self.value.len() > 4096 || self.kind.len() > 64 || !self.x.is_finite() || !self.y.is_finite() { bail!("invalid node"); }
        if serde_json::to_vec(self)?.len() > 65536 { bail!("node exceeds 64 KiB"); }
        Ok(())
    }
    pub fn merge(&mut self, other: Self) {
        self.attributes.extend(other.attributes);
        self.sources.extend(other.sources);
        self.sources.sort(); self.sources.dedup();
        // Corroboration is recorded explicitly by a transform or an analyst;
        // multiple sources alone do not prove that the same claim is confirmed.
        if self.confidence == Confidence::Data && other.confidence != Confidence::Data { self.confidence = other.confidence; }
        self.timestamp = self.timestamp.min(other.timestamp);
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge { pub id: String, pub source: String, pub target: String, pub relation: String, pub provenance: String, pub timestamp: i64 }
impl Edge {
    pub fn new(source: &str, target: &str, relation: &str, provenance: &str) -> Self {
        Self { id: Uuid::new_v5(&Uuid::NAMESPACE_URL, format!("{source}:{target}:{relation}:{provenance}").as_bytes()).to_string(), source: source.into(), target: target.into(), relation: relation.into(), provenance: provenance.into(), timestamp: now() }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Batch { pub nodes: Vec<Node>, pub edges: Vec<Edge> }
impl Batch {
    pub fn validate(&self) -> Result<()> {
        if self.nodes.len() > 512 || self.edges.len() > 2048 || serde_json::to_vec(self)?.len() > 2 * 1024 * 1024 { bail!("batch too large"); }
        for n in &self.nodes { n.validate()?; }
        for e in &self.edges { if e.relation.len() > 128 || e.provenance.len() > 128 || Uuid::parse_str(&e.id).is_err() { bail!("invalid edge"); } }
        Ok(())
    }
}

