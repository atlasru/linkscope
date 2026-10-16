use linkscope_engine::{model::*, Graph};

fn entity(id: &str) -> Entity {
    Entity {
        id: id.into(),
        kind: "Record".into(),
        label: id.into(),
        ..Entity::default()
    }
}
fn edge(id: &str, source: &str, target: &str) -> Relationship {
    Relationship {
        id: id.into(),
        source: source.into(),
        target: target.into(),
        ..Relationship::default()
    }
}
fn graph() -> Graph {
    Graph::new(Project {
        entities: vec![entity("a"), entity("b"), entity("c")],
        ..Project::default()
    })
    .unwrap()
}

#[test]
fn directed_undirected_parallel_and_self_edges_are_independent() {
    let mut graph = graph();
    let mut undirected = edge("e2", "a", "b");
    undirected.directed = false;
    graph
        .apply(Patch {
            relationships: vec![
                edge("e1", "a", "b"),
                undirected,
                edge("e3", "b", "a"),
                edge("self", "a", "a"),
            ],
            ..Patch::default()
        })
        .unwrap();
    assert_eq!(graph.relationship_count(), 4);
    assert_eq!(graph.incident_ids("a").count(), 4);
    assert_eq!(graph.incident_ids("b").count(), 3);
    assert!(!graph.relationship("e2").unwrap().directed);
}

#[test]
fn transaction_rejects_dangling_edges_without_partial_updates_or_history_changes() {
    let mut graph = graph();
    let before = graph.snapshot();
    let failure = graph.apply(Patch {
        entities: vec![entity("valid")],
        relationships: vec![edge("bad", "valid", "missing")],
        ..Patch::default()
    });
    assert!(failure.is_err());
    assert_eq!(graph.snapshot(), before);
    assert_eq!(graph.revision(), 0);
    assert!(!graph.can_undo());
}

#[test]
fn deletes_cascade_and_undo_restores_edges_and_collection_memberships() {
    let mut graph = graph();
    graph
        .apply(Patch {
            relationships: vec![edge("e", "a", "b"), edge("self", "a", "a")],
            collections: vec![Collection {
                id: "g".into(),
                label: "Group".into(),
                members: vec!["a".into(), "c".into()],
                color: "#123456".into(),
            }],
            ..Patch::default()
        })
        .unwrap();
    let before = graph.snapshot();
    let delta = graph
        .apply(Patch {
            remove_entities: vec!["a".into()],
            ..Patch::default()
        })
        .unwrap();
    assert_eq!(delta.patch.remove_relationships, vec!["e", "self"]);
    assert_eq!(delta.patch.collections[0].members, vec!["c"]);
    assert_eq!(graph.relationship_count(), 0);
    graph.undo().unwrap();
    assert_eq!(graph.snapshot(), before);
    assert_eq!(graph.incident_ids("a").count(), 2);
    graph.redo().unwrap();
    assert!(graph.entity("a").is_none());
    assert_eq!(graph.incident_ids("b").count(), 0);
}

#[test]
fn edge_retargeted_away_from_deleted_entity_survives_and_undo_restores_it() {
    let mut graph = graph();
    graph
        .apply(Patch {
            relationships: vec![edge("e", "a", "b")],
            ..Patch::default()
        })
        .unwrap();
    graph
        .apply(Patch {
            remove_entities: vec!["a".into()],
            relationships: vec![edge("e", "c", "b")],
            ..Patch::default()
        })
        .unwrap();
    assert_eq!(graph.incident_ids("c").count(), 1);
    graph.undo().unwrap();
    assert_eq!(graph.relationship("e").unwrap().source, "a");
    assert_eq!(graph.incident_ids("c").count(), 0);
}

#[test]
fn undo_redo_handles_mixed_creation_update_and_removal() {
    let mut graph = graph();
    let before = graph.snapshot();
    let mut updated = entity("b");
    updated.notes = "Changed".into();
    graph
        .apply(Patch {
            entities: vec![entity("d"), updated],
            remove_entities: vec!["a".into()],
            relationships: vec![edge("e", "b", "d")],
            ..Patch::default()
        })
        .unwrap();
    let after = graph.snapshot();
    graph.undo().unwrap();
    assert_eq!(graph.snapshot(), before);
    graph.redo().unwrap();
    assert_eq!(graph.snapshot(), after);
    assert_eq!(graph.revision(), 3);
}

#[test]
fn new_edit_after_undo_discards_redo() {
    let mut graph = graph();
    graph
        .apply(Patch {
            entities: vec![entity("d")],
            ..Patch::default()
        })
        .unwrap();
    graph.undo().unwrap();
    assert!(graph.can_redo());
    graph
        .apply(Patch {
            entities: vec![entity("e")],
            ..Patch::default()
        })
        .unwrap();
    assert!(!graph.can_redo());
    assert!(graph.redo().is_err());
}

#[test]
fn invalid_edit_preserves_redo_and_noop_preserves_revision() {
    let mut graph = graph();
    graph
        .apply(Patch {
            entities: vec![entity("d")],
            ..Patch::default()
        })
        .unwrap();
    graph.undo().unwrap();
    assert!(graph
        .apply(Patch {
            relationships: vec![edge("bad", "a", "missing")],
            ..Patch::default()
        })
        .is_err());
    assert!(graph.can_redo());
    graph.apply(Patch::default()).unwrap();
    assert_eq!(graph.revision(), 2);
}

#[test]
fn rejects_duplicates_conflicts_missing_removals_and_nonfinite_coordinates() {
    let mut graph = graph();
    for patch in [
        Patch {
            entities: vec![entity("x"), entity("x")],
            ..Patch::default()
        },
        Patch {
            remove_entities: vec!["a".into(), "a".into()],
            ..Patch::default()
        },
        Patch {
            entities: vec![entity("a")],
            remove_entities: vec!["a".into()],
            ..Patch::default()
        },
        Patch {
            remove_entities: vec!["missing".into()],
            ..Patch::default()
        },
        Patch {
            entities: vec![Entity {
                x: f64::NAN,
                ..entity("x")
            }],
            ..Patch::default()
        },
        Patch {
            entities: vec![Entity {
                y: f64::INFINITY,
                ..entity("x")
            }],
            ..Patch::default()
        },
        Patch {
            collections: vec![Collection {
                id: "g".into(),
                members: vec!["a".into(), "a".into()],
                ..Collection::default()
            }],
            ..Patch::default()
        },
        Patch {
            collections: vec![Collection {
                id: "g".into(),
                members: vec!["missing".into()],
                ..Collection::default()
            }],
            ..Patch::default()
        },
    ] {
        assert!(graph.apply(patch).is_err());
    }
    assert_eq!(graph.entity_count(), 3);
    assert_eq!(graph.revision(), 0);
}

#[test]
fn deleting_multiple_adjacent_entities_deduplicates_cascades() {
    let mut graph = graph();
    graph
        .apply(Patch {
            relationships: vec![edge("e", "a", "b")],
            ..Patch::default()
        })
        .unwrap();
    let delta = graph
        .apply(Patch {
            remove_entities: vec!["a".into(), "b".into()],
            ..Patch::default()
        })
        .unwrap();
    assert_eq!(delta.patch.remove_relationships, vec!["e"]);
    graph.undo().unwrap();
    assert_eq!(graph.relationship_count(), 1);
}

#[test]
fn history_is_limited_to_64_operations() {
    let mut graph = Graph::empty();
    for index in 0..100 {
        graph
            .apply(Patch {
                entities: vec![entity(&index.to_string())],
                ..Patch::default()
            })
            .unwrap();
    }
    let mut count = 0;
    while graph.can_undo() {
        graph.undo().unwrap();
        count += 1;
    }
    assert_eq!(count, 64);
    assert_eq!(graph.entity_count(), 36);
}

#[test]
fn projects_and_delta_roundtrip_typed_properties_and_optional_provenance() {
    let mut graph = graph();
    let mut record = entity("a");
    record.properties.insert(
        "nested".into(),
        serde_json::json!({"count":3,"flags":[true,null]}),
    );
    record.sources.push(SourceRef {
        id: "source".into(),
        label: "Original".into(),
        uri: Some("file:///data.json".into()),
        retrieved_at: Some("2026-10-16T00:00:00Z".into()),
        record: Some(serde_json::json!({"name":"source"})),
    });
    let delta = graph
        .apply(Patch {
            entities: vec![record],
            ..Patch::default()
        })
        .unwrap();
    let json = serde_json::to_value(delta).unwrap();
    assert_eq!(json["canUndo"], true);
    assert!(json["patch"].is_object());
    assert_eq!(
        json["patch"]["entities"][0]["sources"][0]["retrievedAt"],
        "2026-10-16T00:00:00Z"
    );
    let snapshot = graph.snapshot();
    let restored: Project =
        serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
    assert_eq!(restored, snapshot);
}

#[test]
fn title_change_increments_revision_without_graph_history() {
    let mut graph = graph();
    graph.set_title("Case".into()).unwrap();
    assert_eq!(graph.snapshot().title, "Case");
    assert_eq!(graph.revision(), 1);
    assert!(!graph.can_undo());
    assert!(graph.set_title(" ".into()).is_err());
}
