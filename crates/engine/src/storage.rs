//! Versioned JSON persistence. Save uses a same-directory staged file and atomic
//! rename; the previous valid project is kept as `.bak`. Loading never repairs or
//! overwrites a damaged file implicitly.
use crate::{model::*, validate_project};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
};

pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadOutcome {
    pub project: Project,
    pub recovered_from: Option<String>,
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

pub fn save(path: &Path, project: &Project) -> Result<()> {
    validate_project(project)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ensure!(parent.is_dir(), "Project directory does not exist");
    if path.exists() {
        // A recovered project can be saved to another name. Never silently erase
        // the damaged original which may contain recoverable user data.
        read_exact(path)
            .context("Existing project is corrupt; save to a new filename to preserve it")?;
    }
    let mut staged = tempfile::NamedTempFile::new_in(parent).context("Create staged project")?;
    {
        let mut writer = BufWriter::with_capacity(1024 * 1024, staged.as_file_mut());
        serde_json::to_writer(&mut writer, project).context("Serialize project")?;
        writer.flush()?;
    }
    ensure!(
        staged.as_file().metadata()?.len() <= MAX_FILE_BYTES,
        "Project exceeds 512 MiB file limit"
    );
    staged
        .as_file()
        .sync_all()
        .context("Flush staged project")?;
    if path.exists() {
        let mut backup = tempfile::NamedTempFile::new_in(parent)?;
        let mut source = File::open(path)?;
        std::io::copy(&mut source, backup.as_file_mut())?;
        backup.as_file().sync_all()?;
        backup
            .persist(sidecar(path, ".bak"))
            .map_err(|e| e.error)
            .context("Preserve previous project")?;
    }
    let pending = sidecar(path, ".pending");
    let file = staged
        .persist(&pending)
        .map_err(|e| e.error)
        .context("Stage pending project")?;
    drop(file);
    fs::rename(&pending, path)
        .context("Atomically replace project; pending copy retained for recovery")?;
    // On Unix this persists directory entry changes. Windows RenameFileEx and
    // flushed file contents are the available standard-library boundary.
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn load(path: &Path) -> Result<LoadOutcome> {
    let candidates = [
        path.to_path_buf(),
        sidecar(path, ".pending"),
        sidecar(path, ".bak"),
    ];
    let mut failures = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        match read_exact(candidate) {
            Ok(project) => {
                return Ok(LoadOutcome {
                    project,
                    recovered_from: if index == 0 {
                        None
                    } else {
                        Some(candidate.to_string_lossy().into_owned())
                    },
                })
            }
            Err(error) => failures.push(format!("{}: {error:#}", candidate.display())),
        }
    }
    bail!(
        "Cannot open project or recovery copies. {}",
        failures.join("; ")
    )
}

fn read_exact(path: &Path) -> Result<Project> {
    let file = File::open(path).with_context(|| format!("Open {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_FILE_BYTES,
        "Project exceeds 512 MiB file limit"
    );
    // Deserialize directly into typed records. A generic JSON value tree at this
    // boundary would duplicate every property/key in a large portable graph.
    // A bounded byte buffer avoids serde's per-byte Read adapter cost without
    // allocating a generic JSON value tree for current projects.
    let mut data = Vec::with_capacity(file.metadata()?.len() as usize);
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut data)?;
    ensure!(
        data.len() as u64 <= MAX_FILE_BYTES,
        "Project exceeds 512 MiB file limit"
    );
    let header: VersionHeader = serde_json::from_slice(&data).context("Invalid project JSON")?;
    ensure!(
        matches!(header.schema_version, 1 | 2),
        "Unsupported project schema version {}",
        header.schema_version
    );
    let project = if header.schema_version == 1 {
        migrate_v1(serde_json::from_slice(&data).context("Invalid v1 project")?)?
    } else {
        let wire: PortableProject = serde_json::from_slice(&data).context("Invalid v2 project")?;
        ensure!(
            wire.schema_version == SCHEMA_VERSION,
            "Project schema changed while opening"
        );
        Project {
            schema_version: SCHEMA_VERSION,
            id: wire.id,
            title: wire.title,
            entities: wire.entities,
            relationships: wire.relationships,
            collections: wire.collections,
            metadata: wire.metadata,
        }
    };
    validate_project(&project).context("Invalid project graph")?;
    Ok(project)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionHeader {
    #[serde(alias = "version")]
    schema_version: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableProject {
    schema_version: u32,
    id: String,
    #[serde(default = "untitled")]
    title: String,
    #[serde(default)]
    entities: Vec<Entity>,
    #[serde(default)]
    relationships: Vec<Relationship>,
    #[serde(default)]
    collections: Vec<Collection>,
    #[serde(default)]
    metadata: Properties,
}

fn untitled() -> String {
    "Untitled".into()
}

/// Actual legacy v1 exports used value/attributes and relation/provenance.
/// Preserve unknown legacy fields in a source record rather than discarding them.
#[derive(Deserialize)]
struct LegacyProject {
    id: Option<String>,
    title: Option<String>,
    #[serde(default, alias = "entities")]
    nodes: Vec<Value>,
    #[serde(default, alias = "relationships")]
    edges: Vec<Value>,
    #[serde(default, alias = "collections")]
    groups: Vec<Collection>,
    #[serde(default)]
    metadata: Properties,
}

#[derive(Deserialize)]
struct LegacyEntity {
    id: String,
    kind: String,
    value: String,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    #[serde(default)]
    attributes: Properties,
    #[serde(default)]
    sources: Vec<String>,
    confidence: Option<Value>,
    timestamp: Option<Value>,
}

#[derive(Deserialize)]
struct LegacyRelationship {
    id: String,
    source: String,
    target: String,
    relation: String,
    provenance: Option<String>,
    timestamp: Option<Value>,
}

fn legacy_source(record: Value) -> SourceRef {
    SourceRef {
        id: uuid::Uuid::new_v4().to_string(),
        label: "LinkScope v1".into(),
        record: Some(record),
        ..SourceRef::default()
    }
}

fn migrate_v1(wire: LegacyProject) -> Result<Project> {
    let mut project = Project {
        title: wire.title.unwrap_or_else(untitled),
        metadata: wire.metadata,
        collections: wire.groups,
        ..Project::default()
    };
    if let Some(id) = wire.id {
        project.id = id;
    }
    for record in wire.nodes {
        if record.get("value").is_some() {
            let node: LegacyEntity =
                serde_json::from_value(record.clone()).context("Invalid legacy entity")?;
            let mut metadata = Properties::new();
            if let Some(value) = node.confidence {
                metadata.insert("legacyConfidence".into(), value);
            }
            if let Some(value) = node.timestamp {
                metadata.insert("legacyTimestamp".into(), value);
            }
            let mut sources: Vec<_> = node
                .sources
                .into_iter()
                .map(|label| SourceRef {
                    id: uuid::Uuid::new_v4().to_string(),
                    uri: if label.starts_with("https://") || label.starts_with("http://") {
                        Some(label.clone())
                    } else {
                        None
                    },
                    label,
                    ..SourceRef::default()
                })
                .collect();
            sources.push(legacy_source(record));
            project.entities.push(Entity {
                id: node.id,
                kind: node.kind,
                label: node.value,
                x: node.x,
                y: node.y,
                properties: node.attributes,
                metadata,
                sources,
                ..Entity::default()
            });
        } else {
            project
                .entities
                .push(serde_json::from_value(record).context("Invalid v1 portable entity")?);
        }
    }
    for record in wire.edges {
        if record.get("relation").is_some() {
            let edge: LegacyRelationship =
                serde_json::from_value(record.clone()).context("Invalid legacy relationship")?;
            let mut metadata = Properties::new();
            if let Some(value) = edge.timestamp {
                metadata.insert("legacyTimestamp".into(), value);
            }
            let mut sources = vec![legacy_source(record)];
            if let Some(label) = edge.provenance {
                if !label.is_empty() {
                    sources.push(SourceRef {
                        id: uuid::Uuid::new_v4().to_string(),
                        label,
                        ..SourceRef::default()
                    });
                }
            }
            project.relationships.push(Relationship {
                id: edge.id,
                source: edge.source,
                target: edge.target,
                kind: edge.relation.clone(),
                label: edge.relation,
                metadata,
                sources,
                ..Relationship::default()
            });
        } else {
            project
                .relationships
                .push(serde_json::from_value(record).context("Invalid v1 portable relationship")?);
        }
    }
    Ok(project)
}
