//! Conservative, offline import preview. Generic records always receive fresh IDs;
//! only explicit structured graph JSON can declare relationships or shared identity.
use crate::{model::*, validate_project};
use anyhow::{bail, ensure, Context, Result};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    net::IpAddr,
};

const MAX_IMPORT_BYTES: usize = 32 * 1024 * 1024;
const MAX_RECORDS: usize = 100_000;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub patch: Patch,
    pub warnings: Vec<String>,
}

pub fn preview(text: &str, format: &str, name: &str) -> Result<ImportPreview> {
    ensure!(
        text.len() <= MAX_IMPORT_BYTES,
        "Import exceeds 32 MiB limit"
    );
    let mut result = match format.trim_start_matches('.').to_ascii_lowercase().as_str() {
        "json" => json(text, name)?,
        "csv" => csv(text, name)?,
        "txt" | "text" => txt(text, name)?,
        other => bail!("Unsupported import format {other}"),
    };
    ensure!(
        result.patch.entities.len() <= MAX_RECORDS
            && result.patch.relationships.len() <= MAX_RECORDS * 5,
        "Import exceeds record limit"
    );
    for (index, entity) in result.patch.entities.iter_mut().enumerate() {
        if entity.label.is_empty() {
            entity.label = format!("Record {}", index + 1);
        }
        if entity.kind.is_empty() {
            entity.kind = "Record".into();
        }
    }
    validate_project(&Project {
        entities: result.patch.entities.clone(),
        relationships: result.patch.relationships.clone(),
        collections: result.patch.collections.clone(),
        ..Project::default()
    })?;
    Ok(result)
}

fn source(name: &str, record: Value) -> SourceRef {
    SourceRef {
        id: uuid::Uuid::new_v4().to_string(),
        label: name.to_owned(),
        record: Some(record),
        ..SourceRef::default()
    }
}

fn record_entity(record: Value, name: &str, index: usize) -> Entity {
    let properties = match &record {
        Value::Object(object) => object
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        other => [("value".to_owned(), other.clone())].into_iter().collect(),
    };
    let label = ["label", "name", "title"]
        .iter()
        .find_map(|key| record.get(*key).and_then(Value::as_str))
        .map(str::to_owned)
        .unwrap_or_else(|| match &record {
            Value::String(value) => value.clone(),
            _ => format!("Record {}", index + 1),
        });
    Entity {
        id: uuid::Uuid::new_v4().to_string(),
        label,
        kind: "Record".into(),
        x: (index % 24) as f64 * 140.0,
        y: (index / 24) as f64 * 100.0,
        properties,
        sources: vec![source(name, record)],
        ..Entity::default()
    }
}

fn json(text: &str, name: &str) -> Result<ImportPreview> {
    let value: Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).context("Invalid JSON")?;
    if let Some(records) = value.as_array() {
        ensure!(records.len() <= MAX_RECORDS, "Import exceeds record limit");
        return Ok(ImportPreview {
            patch: Patch {
                entities: records
                    .iter()
                    .enumerate()
                    .map(|(index, record)| record_entity(record.clone(), name, index))
                    .collect(),
                ..Patch::default()
            },
            warnings: Vec::new(),
        });
    }
    let object = value
        .as_object()
        .context("JSON must be a record array or structured graph object")?;
    ensure!(
        object.contains_key("entities"),
        "Structured graph JSON requires an entities array"
    );
    let mut patch: Patch =
        serde_json::from_value(value.clone()).context("Invalid structured graph records")?;
    ensure!(
        patch.remove_entities.is_empty()
            && patch.remove_relationships.is_empty()
            && patch.remove_collections.is_empty(),
        "Imports cannot delete existing project records"
    );
    let entities = object
        .get("entities")
        .and_then(Value::as_array)
        .context("entities must be an array")?;
    for (index, entity) in patch.entities.iter_mut().enumerate() {
        entity.sources.push(source(name, entities[index].clone()));
        if entities[index].get("x").is_none() {
            entity.x = (index % 24) as f64 * 140.0;
        }
        if entities[index].get("y").is_none() {
            entity.y = (index / 24) as f64 * 100.0;
        }
    }
    if let Some(edges) = object.get("relationships").and_then(Value::as_array) {
        for (edge, original) in patch.relationships.iter_mut().zip(edges) {
            edge.sources.push(source(name, original.clone()));
        }
    }
    Ok(ImportPreview {
        patch,
        warnings: Vec::new(),
    })
}

fn csv(text: &str, name: &str) -> Result<ImportPreview> {
    let rows = parse_csv(text.trim_start_matches('\u{feff}'))?;
    let headers = rows.first().context("CSV is empty")?;
    ensure!(!headers.is_empty(), "CSV has no columns");
    let mut seen = HashSet::new();
    ensure!(
        headers
            .iter()
            .all(|header| !header.trim().is_empty() && seen.insert(header)),
        "CSV headers must be nonempty and unique"
    );
    let mut entities = Vec::new();
    for (index, row) in rows.iter().skip(1).enumerate() {
        if row.len() == 1 && row[0].is_empty() {
            continue;
        }
        ensure!(
            row.len() == headers.len(),
            "CSV row {} has {} fields; expected {}",
            index + 2,
            row.len(),
            headers.len()
        );
        let record = Value::Object(
            headers
                .iter()
                .cloned()
                .zip(row.iter().cloned().map(Value::String))
                .collect(),
        );
        entities.push(record_entity(record, name, index));
    }
    Ok(ImportPreview { patch: Patch { entities, ..Patch::default() }, warnings: vec!["CSV rows become separate records. Relationships require explicit structured graph JSON.".into()] })
}

/// RFC 4180-style quoted fields, including embedded newlines and escaped quotes.
fn parse_csv(text: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut closed_quote = false;
    let mut started = false;
    while let Some(character) = chars.next() {
        started = true;
        if quoted {
            if character == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed_quote = true;
                }
            } else {
                field.push(character);
            }
            continue;
        }
        match character {
            '"' if field.is_empty() && !closed_quote => quoted = true,
            ',' => {
                row.push(std::mem::take(&mut field));
                closed_quote = false;
            }
            '\n' | '\r' => {
                if character == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                ensure!(rows.len() <= MAX_RECORDS + 1, "CSV exceeds record limit");
                closed_quote = false;
                started = false;
            }
            '"' => bail!("Unexpected quote in unquoted CSV field"),
            _ if closed_quote => bail!("Unexpected character after quoted CSV field"),
            _ => field.push(character),
        }
    }
    ensure!(!quoted, "Unclosed quoted CSV field");
    if started || !row.is_empty() || !field.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

fn txt(text: &str, name: &str) -> Result<ImportPreview> {
    let url = Regex::new(r#"https?://[^\s<>\"']+"#)?;
    let email = Regex::new(
        r"(?i)\b[a-z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)+\b",
    )?;
    let domain = Regex::new(r"(?i)\b(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}\b")?;
    let mut spans = BTreeMap::<usize, usize>::new();
    let mut found = Vec::<(usize, String, String)>::new();
    for (kind, regex) in [("URL", &url), ("Email", &email), ("Domain", &domain)] {
        for item in regex.find_iter(text) {
            if overlaps(&spans, item.start(), item.end()) {
                continue;
            }
            let value = item
                .as_str()
                .trim_end_matches(['.', ',', ';', ':', ')', ']', '}'])
                .to_owned();
            ensure!(
                found.len() < MAX_RECORDS,
                "Text extraction exceeds record limit"
            );
            spans.insert(item.start(), item.end());
            found.push((item.start(), kind.into(), value));
        }
    }
    // IP addresses require std's full parser; a numeric-looking token alone is not enough.
    let mut offset = 0;
    for token in text.split_inclusive(char::is_whitespace) {
        let value = token
            .trim()
            .trim_matches(['(', ')', '[', ']', '{', '}', ',', ';', '"', '\'', '.']);
        if value.parse::<IpAddr>().is_ok() && !overlaps(&spans, offset, offset + token.len()) {
            ensure!(
                found.len() < MAX_RECORDS,
                "Text extraction exceeds record limit"
            );
            found.push((offset, "IP Address".into(), value.into()));
        }
        offset += token.len();
    }
    found.sort_by_key(|(offset, _, _)| *offset);
    ensure!(
        found.len() <= MAX_RECORDS,
        "Text extraction exceeds record limit"
    );
    let line_starts: Vec<_> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(index, _)| index + 1))
        .collect();
    let entities = found.into_iter().enumerate().map(|(index, (offset, kind, value))| {
        let line_index = line_starts.partition_point(|start| *start <= offset).saturating_sub(1);
        let line_start = line_starts[line_index];
        let line_end = line_starts.get(line_index + 1).map_or(text.len(), |start| start - 1);
        // Bound retained context per occurrence: repeating an entire long line
        // for every candidate would amplify a modest input into unbounded data.
        let mut context_start = line_start.max(offset.saturating_sub(128));
        let mut context_end = line_end.min(offset.saturating_add(value.len()).saturating_add(128));
        while !text.is_char_boundary(context_start) { context_start += 1; }
        while !text.is_char_boundary(context_end) { context_end -= 1; }
        let mut entity = record_entity(serde_json::json!({ "value": value, "offset": offset, "line": &text[context_start..context_end], "lineNumber": line_index + 1 }), name, index);
        entity.kind = kind;
        entity.label = value;
        entity
    }).collect();
    Ok(ImportPreview { patch: Patch { entities, ..Patch::default() }, warnings: vec!["Review extracted candidates. Each occurrence is separate; no identities or relationships are inferred.".into()] })
}

fn overlaps(spans: &BTreeMap<usize, usize>, start: usize, end: usize) -> bool {
    spans
        .range(..end)
        .next_back()
        .is_some_and(|(_, previous_end)| *previous_end > start)
}
