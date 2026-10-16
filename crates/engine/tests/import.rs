use linkscope_engine::{import, model::*, Graph};

#[test]
fn csv_preserves_quoted_newlines_commas_quotes_and_original_records() {
    let result = import::preview(
        "name,note\r\n\"Alice, A\",\"line1\nline2 \"\"quote\"\"\"\r\nBob,plain\r\n",
        "csv",
        "report.csv",
    )
    .unwrap();
    assert_eq!(result.patch.entities.len(), 2);
    let first = &result.patch.entities[0];
    assert_eq!(first.label, "Alice, A");
    assert_eq!(first.properties["note"], "line1\nline2 \"quote\"");
    assert_eq!(first.sources[0].label, "report.csv");
    assert!(first.sources[0].record.is_some());
    assert!(result.patch.relationships.is_empty());
}

#[test]
fn csv_rejects_ambiguous_headers_malformed_quotes_and_wrong_width() {
    for text in [
        "a,a\n1,2",
        ",b\n1,2",
        "a,b\n1",
        "a\n\"unclosed",
        "a\n\"closed\"junk",
        "a\nbad\"quote",
    ] {
        assert!(import::preview(text, "csv", "bad.csv").is_err(), "{text}");
    }
}

#[test]
fn generic_records_never_merge_on_coincidental_id() {
    let result = import::preview(
        r#"[{"id":"same","name":"A","count":2},{"id":"same","name":"B","count":3}]"#,
        "json",
        "records.json",
    )
    .unwrap();
    assert_eq!(result.patch.entities.len(), 2);
    assert_ne!(result.patch.entities[0].id, result.patch.entities[1].id);
    assert_eq!(result.patch.entities[0].properties["id"], "same");
    assert_eq!(result.patch.entities[0].properties["count"], 2);
    assert!(result.patch.relationships.is_empty());
}

#[test]
fn structured_graph_preserves_explicit_topology_and_is_editable_undoable() {
    let result = import::preview(r#"{"entities":[{"id":"a","label":"A","kind":"Domain"},{"id":"b","label":"B"}],"relationships":[{"id":"e","source":"a","target":"b","directed":false}],"collections":[{"id":"g","members":["a","b"]}]}"#, "json", "graph.json").unwrap();
    let mut graph = Graph::empty();
    graph.apply(result.patch).unwrap();
    assert_eq!(graph.entity("a").unwrap().kind, "Domain");
    assert_eq!(graph.relationship_count(), 1);
    graph.undo().unwrap();
    assert_eq!(graph.entity_count(), 0);
    graph.redo().unwrap();
    assert_eq!(graph.entity_count(), 2);
    let mut entity = graph.entity("a").unwrap().clone();
    entity.notes = "Edited".into();
    graph
        .apply(Patch {
            entities: vec![entity],
            ..Patch::default()
        })
        .unwrap();
    assert_eq!(graph.entity("a").unwrap().notes, "Edited");
}

#[test]
fn structured_import_rejects_dangling_edges_deletes_and_duplicates() {
    for text in [
        r#"{"entities":[{"id":"a"}],"relationships":[{"id":"e","source":"a","target":"missing"}]}"#,
        r#"{"entities":[],"removeEntities":["a"]}"#,
        r#"{"entities":[{"id":"a"},{"id":"a"}]}"#,
    ] {
        assert!(import::preview(text, "json", "bad.json").is_err());
    }
}

#[test]
fn text_extracts_separate_candidates_without_invented_edges_or_duplicate_url_parts() {
    let result = import::preview("See https://example.org/path, email alice@example.net; host sub.example.com and IP 192.0.2.1 or [2001:db8::1].", "txt", "notes.txt").unwrap();
    let labels: Vec<_> = result
        .patch
        .entities
        .iter()
        .map(|e| e.label.as_str())
        .collect();
    assert!(labels.contains(&"https://example.org/path"));
    assert!(labels.contains(&"alice@example.net"));
    assert!(labels.contains(&"sub.example.com"));
    assert!(labels.contains(&"192.0.2.1"));
    assert!(labels.contains(&"2001:db8::1"));
    assert!(!labels.contains(&"example.org"));
    assert!(result.patch.relationships.is_empty());
    assert!(!result.warnings.is_empty());
    assert!(result
        .patch
        .entities
        .iter()
        .all(|e| e.sources[0].record.as_ref().unwrap()["line"].is_string()));
}

#[test]
fn repeated_text_mentions_keep_distinct_identity_and_empty_text_is_empty() {
    let result = import::preview("example.com example.com", "txt", "notes.txt").unwrap();
    assert_eq!(result.patch.entities.len(), 2);
    assert_ne!(result.patch.entities[0].id, result.patch.entities[1].id);
    assert!(import::preview("ordinary prose", "txt", "notes.txt")
        .unwrap()
        .patch
        .is_empty());
}

#[test]
fn size_and_unknown_format_are_rejected() {
    assert!(import::preview("data", "pdf", "file.pdf").is_err());
    assert!(import::preview(&"a".repeat(32 * 1024 * 1024 + 1), "txt", "big.txt").is_err());
}

#[test]
fn long_single_line_keeps_bounded_context_and_unicode_boundaries() {
    let text = format!(
        "{} {} {}",
        "слово ".repeat(5000),
        "example.com ".repeat(1000),
        "конец ".repeat(5000)
    );
    let result = import::preview(&text, "txt", "long.txt").unwrap();
    assert_eq!(result.patch.entities.len(), 1000);
    assert!(result
        .patch
        .entities
        .iter()
        .all(|entity| entity.sources[0].record.as_ref().unwrap()["line"]
            .as_str()
            .unwrap()
            .len()
            <= 300));
}
