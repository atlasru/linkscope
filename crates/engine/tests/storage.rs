use linkscope_engine::{model::*, storage};
use std::{fs, path::Path};

fn project(title: &str) -> Project {
    Project {
        title: title.into(),
        entities: vec![Entity {
            id: "a".into(),
            label: "A".into(),
            ..Entity::default()
        }],
        ..Project::default()
    }
}
fn write(path: &Path, value: &Project) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

#[test]
fn atomic_save_roundtrips_and_preserves_previous_valid_version() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("case.linkscope");
    let first = project("First");
    let second = project("Second");
    storage::save(&path, &first).unwrap();
    storage::save(&path, &second).unwrap();
    assert_eq!(storage::load(&path).unwrap().project, second);
    assert!(!temp.path().join("case.linkscope.pending").exists());
    assert_eq!(
        storage::load(&temp.path().join("case.linkscope.bak"))
            .unwrap()
            .project,
        first
    );
}

#[test]
fn interrupted_first_save_recovers_pending_without_mutating_disk() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("case.linkscope");
    let expected = project("Pending");
    write(&temp.path().join("case.linkscope.pending"), &expected);
    let result = storage::load(&path).unwrap();
    assert_eq!(result.project, expected);
    assert!(result.recovered_from.unwrap().ends_with(".pending"));
    assert!(!path.exists());
}

#[test]
fn committed_project_wins_over_interrupted_pending_save() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("case.linkscope");
    let expected = project("Committed");
    write(&path, &expected);
    write(
        &temp.path().join("case.linkscope.pending"),
        &project("Uncommitted"),
    );
    let result = storage::load(&path).unwrap();
    assert_eq!(result.project, expected);
    assert!(result.recovered_from.is_none());
}

#[test]
fn corrupt_primary_recovers_backup_and_refuses_silent_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("case.linkscope");
    storage::save(&path, &project("First")).unwrap();
    storage::save(&path, &project("Second")).unwrap();
    fs::write(&path, b"{interrupted").unwrap();
    let recovered = storage::load(&path).unwrap();
    assert_eq!(recovered.project.title, "First");
    assert!(recovered.recovered_from.unwrap().ends_with(".bak"));
    assert!(storage::save(&path, &project("Replacement")).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"{interrupted");
}

#[test]
fn corrupt_copies_and_unknown_versions_return_clear_errors() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("case.linkscope");
    fs::write(&path, br#"{"schemaVersion":99,"id":"case"}"#).unwrap();
    let error = storage::load(&path).unwrap_err().to_string();
    assert!(error.contains("Unsupported project schema version 99"));
    fs::write(&path, b"null").unwrap();
    assert!(storage::load(&path).is_err());
}

#[test]
fn invalid_graph_does_not_replace_valid_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("case.linkscope");
    let original = project("Original");
    storage::save(&path, &original).unwrap();
    let mut invalid = project("Invalid");
    invalid.relationships.push(Relationship {
        id: "bad".into(),
        source: "a".into(),
        target: "missing".into(),
        ..Relationship::default()
    });
    assert!(storage::save(&path, &invalid).is_err());
    assert_eq!(storage::load(&path).unwrap().project, original);
}

#[test]
fn v1_nodes_edges_groups_migrate_and_preserve_properties() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("v1.json");
    fs::write(&path, br##"{"version":1,"id":"case","title":"Old","nodes":[{"id":"a","label":"A","properties":{"n":4}},{"id":"b"}],"edges":[{"id":"e","source":"a","target":"b","directed":false}],"groups":[{"id":"g","members":["a"],"color":"#ff0000"}]}"##).unwrap();
    let result = storage::load(&path).unwrap();
    assert_eq!(result.project.schema_version, 2);
    assert_eq!(result.project.entities[0].properties["n"], 4);
    assert!(!result.project.relationships[0].directed);
    assert_eq!(result.project.collections[0].members, vec!["a"]);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap()["version"],
        1
    );
}

#[test]
fn ambiguous_migration_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("v1.json");
    fs::write(
        &path,
        br#"{"version":1,"id":"case","nodes":[],"entities":[]}"#,
    )
    .unwrap();
    assert!(storage::load(&path).is_err());
}

#[test]
fn actual_legacy_export_migrates_without_losing_attribution_or_attributes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("legacy.json");
    fs::write(&path, br#"{"version":1,"nodes":[{"id":"old-a","kind":"domain","value":"example.org","attributes":{"rank":4},"sources":["https://source.example/report"],"confidence":"information","timestamp":123,"x":1.5,"y":-2.5},{"id":"old-b","kind":"ip","value":"192.0.2.1","attributes":{},"sources":[],"confidence":"data","timestamp":456,"x":0,"y":0}],"edges":[{"id":"old-e","source":"old-a","target":"old-b","relation":"resolves_to","provenance":"DNS","timestamp":789}]}"#).unwrap();
    let result = storage::load(&path).unwrap();
    let entity = &result.project.entities[0];
    assert_eq!(entity.label, "example.org");
    assert_eq!(entity.properties["rank"], 4);
    assert_eq!(entity.metadata["legacyConfidence"], "information");
    assert_eq!(entity.metadata["legacyTimestamp"], 123);
    assert_eq!(
        entity.sources[0].uri.as_deref(),
        Some("https://source.example/report")
    );
    assert_eq!(
        entity.sources.last().unwrap().record.as_ref().unwrap()["value"],
        "example.org"
    );
    assert_eq!((entity.x, entity.y), (1.5, -2.5));
    let edge = &result.project.relationships[0];
    assert_eq!(edge.label, "resolves_to");
    assert_eq!(edge.kind, "resolves_to");
    assert!(edge.directed);
    assert!(edge.sources.iter().any(|source| source.label == "DNS"));
    assert_eq!(edge.metadata["legacyTimestamp"], 789);
}
