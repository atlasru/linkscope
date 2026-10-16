pub mod import;
pub mod model;
pub mod storage;

use anyhow::{bail, ensure, Context, Result};
use model::*;
use std::collections::{HashMap, HashSet, VecDeque};

const HISTORY_LIMIT: usize = 64;
const HISTORY_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_ENTITIES: usize = 1_000_000;
pub const MAX_RELATIONSHIPS: usize = 5_000_000;

/// Authoritative graph. Rendering and external services depend only on its data contract.
/// Transactions validate their final topology before mutating any indexed state.
pub struct Graph {
    project: Project,
    entities: HashMap<String, Entity>,
    relationships: HashMap<String, Relationship>,
    collections: HashMap<String, Collection>,
    adjacency: HashMap<String, HashSet<String>>,
    revision: u64,
    undo: VecDeque<(Patch, usize)>,
    redo: VecDeque<(Patch, usize)>,
}

impl Graph {
    pub fn empty() -> Self {
        Self::new(Project::default()).expect("empty graph is valid")
    }

    pub fn new(mut project: Project) -> Result<Self> {
        ensure!(
            project.schema_version == SCHEMA_VERSION,
            "Unsupported schema version {}",
            project.schema_version
        );
        validate_id(&project.id)?;
        let patch = Patch {
            entities: std::mem::take(&mut project.entities),
            relationships: std::mem::take(&mut project.relationships),
            collections: std::mem::take(&mut project.collections),
            ..Patch::default()
        };
        let mut graph = Self {
            project,
            entities: HashMap::new(),
            relationships: HashMap::new(),
            collections: HashMap::new(),
            adjacency: HashMap::new(),
            revision: 0,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
        };
        let patch = graph.prepare(patch)?;
        // Opening a project needs no mutation delta or history: move its records
        // directly instead of transiently duplicating the entire loaded graph.
        for entity in patch.entities {
            graph.entities.insert(entity.id.clone(), entity);
        }
        for edge in patch.relationships {
            graph
                .adjacency
                .entry(edge.source.clone())
                .or_default()
                .insert(edge.id.clone());
            graph
                .adjacency
                .entry(edge.target.clone())
                .or_default()
                .insert(edge.id.clone());
            graph.relationships.insert(edge.id.clone(), edge);
        }
        for collection in patch.collections {
            graph.collections.insert(collection.id.clone(), collection);
        }
        Ok(graph)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Project-title changes are not graph edits and do not enter graph history.
    pub fn set_title(&mut self, title: String) -> Result<()> {
        ensure!(
            !title.trim().is_empty() && title.len() <= 4096,
            "Title must contain 1 to 4096 bytes"
        );
        if self.project.title != title {
            self.project.title = title;
            self.revision += 1;
        }
        Ok(())
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn entity(&self, id: &str) -> Option<&Entity> {
        self.entities.get(id)
    }
    pub fn relationship(&self, id: &str) -> Option<&Relationship> {
        self.relationships.get(id)
    }
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }
    pub fn relationship_count(&self) -> usize {
        self.relationships.len()
    }
    pub fn incident_ids(&self, id: &str) -> impl Iterator<Item = &str> {
        self.adjacency
            .get(id)
            .into_iter()
            .flatten()
            .map(String::as_str)
    }

    /// Portable snapshots have deterministic record ordering; interactive edits never call this.
    pub fn snapshot(&self) -> Project {
        let mut project = self.project.clone();
        project.entities = self.entities.values().cloned().collect();
        project.relationships = self.relationships.values().cloned().collect();
        project.collections = self.collections.values().cloned().collect();
        project.entities.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        project
            .relationships
            .sort_unstable_by(|a, b| a.id.cmp(&b.id));
        project.collections.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        project
    }

    pub fn apply(&mut self, patch: Patch) -> Result<Delta> {
        let patch = self.prepare(patch)?;
        if patch.is_empty() {
            return Ok(self.delta(patch));
        }
        let inverse = self.inverse(&patch);
        self.commit(&patch);
        self.revision += 1;
        Self::push_history(&mut self.undo, inverse);
        self.redo.clear();
        Ok(self.delta(patch))
    }

    pub fn undo(&mut self) -> Result<Delta> {
        let (patch, _) = self.undo.back().context("Nothing to undo")?;
        let patch = self.prepare(patch.clone())?;
        let inverse = self.inverse(&patch);
        self.undo.pop_back();
        self.commit(&patch);
        self.revision += 1;
        Self::push_history(&mut self.redo, inverse);
        Ok(self.delta(patch))
    }

    pub fn redo(&mut self) -> Result<Delta> {
        let (patch, _) = self.redo.back().context("Nothing to redo")?;
        let patch = self.prepare(patch.clone())?;
        let inverse = self.inverse(&patch);
        self.redo.pop_back();
        self.commit(&patch);
        self.revision += 1;
        Self::push_history(&mut self.undo, inverse);
        Ok(self.delta(patch))
    }

    fn delta(&self, patch: Patch) -> Delta {
        Delta {
            patch,
            revision: self.revision,
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
        }
    }

    fn push_history(stack: &mut VecDeque<(Patch, usize)>, patch: Patch) {
        let bytes = patch_size(&patch);
        stack.push_back((patch, bytes));
        let mut total = stack.iter().map(|(_, bytes)| bytes).sum::<usize>();
        // Always preserve the newest operation, even if one large deletion exceeds the budget.
        while stack.len() > 1 && (stack.len() > HISTORY_LIMIT || total > HISTORY_BYTES) {
            if let Some((_, bytes)) = stack.pop_front() {
                total -= bytes;
            }
        }
    }

    fn prepare(&self, mut patch: Patch) -> Result<Patch> {
        let entity_ids = unique(patch.entities.iter().map(|e| e.id.as_str()), "entity")?;
        let relationship_ids = unique(
            patch.relationships.iter().map(|e| e.id.as_str()),
            "relationship",
        )?;
        let collection_ids = unique(
            patch.collections.iter().map(|e| e.id.as_str()),
            "collection",
        )?;
        let removed_entities = unique(
            patch.remove_entities.iter().map(String::as_str),
            "removed entity",
        )?;
        let mut removed_relationships = unique(
            patch.remove_relationships.iter().map(String::as_str),
            "removed relationship",
        )?;
        let removed_collections = unique(
            patch.remove_collections.iter().map(String::as_str),
            "removed collection",
        )?;
        for id in &removed_entities {
            ensure!(self.entities.contains_key(*id), "Unknown entity {id}");
            ensure!(
                !entity_ids.contains(id),
                "Entity {id} is both updated and removed"
            );
        }
        for id in &removed_relationships {
            ensure!(
                self.relationships.contains_key(*id),
                "Unknown relationship {id}"
            );
            ensure!(
                !relationship_ids.contains(id),
                "Relationship {id} is both updated and removed"
            );
        }
        for id in &removed_collections {
            ensure!(
                self.collections.contains_key(*id),
                "Unknown collection {id}"
            );
            ensure!(
                !collection_ids.contains(id),
                "Collection {id} is both updated and removed"
            );
        }
        let exists = |id: &str| {
            entity_ids.contains(id)
                || (self.entities.contains_key(id) && !removed_entities.contains(id))
        };
        for entity in &patch.entities {
            ensure!(
                entity.x.is_finite() && entity.y.is_finite(),
                "Entity {} has non-finite coordinates",
                entity.id
            );
        }
        for relationship in &patch.relationships {
            ensure!(
                exists(&relationship.source) && exists(&relationship.target),
                "Relationship {} has missing endpoints",
                relationship.id
            );
        }
        // Existing incident edges disappear on node deletion unless explicitly retargeted.
        let mut cascaded = Vec::new();
        for id in &removed_entities {
            for edge in self.adjacency.get(*id).into_iter().flatten() {
                if !relationship_ids.contains(edge.as_str())
                    && !removed_relationships.contains(edge.as_str())
                {
                    cascaded.push(edge.clone());
                }
            }
        }
        cascaded.sort_unstable();
        cascaded.dedup();
        for id in &cascaded {
            removed_relationships.insert(id);
        }
        ensure!(
            self.entities.len() - removed_entities.len()
                + entity_ids
                    .iter()
                    .filter(|id| !self.entities.contains_key(**id))
                    .count()
                <= MAX_ENTITIES,
            "Entity limit exceeded ({MAX_ENTITIES})"
        );
        ensure!(
            self.relationships.len() - removed_relationships.len()
                + relationship_ids
                    .iter()
                    .filter(|id| !self.relationships.contains_key(**id))
                    .count()
                <= MAX_RELATIONSHIPS,
            "Relationship limit exceeded ({MAX_RELATIONSHIPS})"
        );
        for collection in &patch.collections {
            unique(
                collection.members.iter().map(String::as_str),
                "collection member",
            )?;
            ensure!(
                collection.members.iter().all(|id| exists(id)),
                "Collection {} has missing members",
                collection.id
            );
        }
        let mut changed_collections = Vec::new();
        if !removed_entities.is_empty() {
            for collection in self.collections.values() {
                if !collection_ids.contains(collection.id.as_str())
                    && !removed_collections.contains(collection.id.as_str())
                    && collection
                        .members
                        .iter()
                        .any(|id| removed_entities.contains(id.as_str()))
                {
                    let mut collection = collection.clone();
                    collection
                        .members
                        .retain(|id| !removed_entities.contains(id.as_str()));
                    changed_collections.push(collection);
                }
            }
        }
        drop(removed_relationships);
        drop(removed_entities);
        drop(removed_collections);
        drop(entity_ids);
        drop(relationship_ids);
        drop(collection_ids);
        patch.remove_relationships.extend(cascaded);
        patch.remove_relationships.sort_unstable();
        changed_collections.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        patch.collections.extend(changed_collections);
        Ok(patch)
    }

    fn inverse(&self, patch: &Patch) -> Patch {
        let mut inverse = Patch::default();
        for entity in &patch.entities {
            if let Some(previous) = self.entities.get(&entity.id) {
                inverse.entities.push(previous.clone());
            } else {
                inverse.remove_entities.push(entity.id.clone());
            }
        }
        for id in &patch.remove_entities {
            inverse.entities.push(self.entities[id].clone());
        }
        for relationship in &patch.relationships {
            if let Some(previous) = self.relationships.get(&relationship.id) {
                inverse.relationships.push(previous.clone());
            } else {
                inverse.remove_relationships.push(relationship.id.clone());
            }
        }
        for id in &patch.remove_relationships {
            inverse.relationships.push(self.relationships[id].clone());
        }
        for collection in &patch.collections {
            if let Some(previous) = self.collections.get(&collection.id) {
                inverse.collections.push(previous.clone());
            } else {
                inverse.remove_collections.push(collection.id.clone());
            }
        }
        for id in &patch.remove_collections {
            inverse.collections.push(self.collections[id].clone());
        }
        inverse
    }

    fn remove_relationship(&mut self, id: &str) {
        if let Some(edge) = self.relationships.remove(id) {
            if let Some(edges) = self.adjacency.get_mut(&edge.source) {
                edges.remove(id);
            }
            if let Some(edges) = self.adjacency.get_mut(&edge.target) {
                edges.remove(id);
            }
        }
    }

    fn commit(&mut self, patch: &Patch) {
        for id in &patch.remove_relationships {
            self.remove_relationship(id);
        }
        for id in &patch.remove_entities {
            self.entities.remove(id);
            self.adjacency.remove(id);
        }
        for id in &patch.remove_collections {
            self.collections.remove(id);
        }
        for entity in &patch.entities {
            self.entities.insert(entity.id.clone(), entity.clone());
        }
        for edge in &patch.relationships {
            self.remove_relationship(&edge.id);
            self.adjacency
                .entry(edge.source.clone())
                .or_default()
                .insert(edge.id.clone());
            self.adjacency
                .entry(edge.target.clone())
                .or_default()
                .insert(edge.id.clone());
            self.relationships.insert(edge.id.clone(), edge.clone());
        }
        for collection in &patch.collections {
            self.collections
                .insert(collection.id.clone(), collection.clone());
        }
    }
}

fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.trim().is_empty() && id.len() <= 1024 && !id.chars().any(char::is_control),
        "Invalid identifier"
    );
    Ok(())
}

/// Validate a portable project without allocating a second working graph.
pub fn validate_project(project: &Project) -> Result<()> {
    ensure!(
        project.schema_version == SCHEMA_VERSION,
        "Unsupported schema version {}",
        project.schema_version
    );
    validate_id(&project.id)?;
    ensure!(
        project.entities.len() <= MAX_ENTITIES && project.relationships.len() <= MAX_RELATIONSHIPS,
        "Project exceeds graph limits"
    );
    let entities = unique(project.entities.iter().map(|e| e.id.as_str()), "entity")?;
    unique(
        project.relationships.iter().map(|e| e.id.as_str()),
        "relationship",
    )?;
    unique(
        project.collections.iter().map(|e| e.id.as_str()),
        "collection",
    )?;
    for entity in &project.entities {
        ensure!(
            entity.x.is_finite() && entity.y.is_finite(),
            "Non-finite entity coordinates"
        );
    }
    for edge in &project.relationships {
        ensure!(
            entities.contains(edge.source.as_str()) && entities.contains(edge.target.as_str()),
            "Relationship {} has missing endpoints",
            edge.id
        );
    }
    for collection in &project.collections {
        unique(
            collection.members.iter().map(String::as_str),
            "collection member",
        )?;
        ensure!(
            collection
                .members
                .iter()
                .all(|id| entities.contains(id.as_str())),
            "Collection {} has missing members",
            collection.id
        );
    }
    Ok(())
}

fn unique<'a>(ids: impl Iterator<Item = &'a str>, kind: &str) -> Result<HashSet<&'a str>> {
    let mut seen = HashSet::new();
    for id in ids {
        validate_id(id)?;
        if !seen.insert(id) {
            bail!("Duplicate {kind} ID {id}");
        }
    }
    Ok(seen)
}

fn patch_size(patch: &Patch) -> usize {
    // The bounded writer counts bytes without materializing a second serialized patch.
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    let _ = serde_json::to_writer(&mut counter, patch);
    counter.0
}
