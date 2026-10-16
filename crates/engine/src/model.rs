use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 2;
pub type Properties = BTreeMap<String, Value>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SourceRef {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieved_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Entity {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub properties: Properties,
    pub metadata: Properties,
    pub notes: String,
    pub sources: Vec<SourceRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Relationship {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub label: String,
    pub directed: bool,
    pub properties: Properties,
    pub metadata: Properties,
    pub notes: String,
    pub sources: Vec<SourceRef>,
}

impl Default for Relationship {
    fn default() -> Self {
        Self {
            id: String::new(),
            source: String::new(),
            target: String::new(),
            kind: String::new(),
            label: String::new(),
            directed: true,
            properties: Properties::new(),
            metadata: Properties::new(),
            notes: String::new(),
            sources: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Collection {
    pub id: String,
    pub label: String,
    pub members: Vec<String>,
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Project {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
    pub collections: Vec<Collection>,
    pub metadata: Properties,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            id: uuid::Uuid::new_v4().to_string(),
            title: "Untitled".into(),
            entities: Vec::new(),
            relationships: Vec::new(),
            collections: Vec::new(),
            metadata: Properties::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Patch {
    pub entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
    pub collections: Vec<Collection>,
    pub remove_entities: Vec<String>,
    pub remove_relationships: Vec<String>,
    pub remove_collections: Vec<String>,
}

impl Patch {
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
            && self.relationships.is_empty()
            && self.collections.is_empty()
            && self.remove_entities.is_empty()
            && self.remove_relationships.is_empty()
            && self.remove_collections.is_empty()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delta {
    pub patch: Patch,
    pub revision: u64,
    pub can_undo: bool,
    pub can_redo: bool,
}
