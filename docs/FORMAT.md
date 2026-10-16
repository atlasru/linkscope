# Portable project and import format

Projects are UTF-8 JSON, schemaVersion2, extension `.linkscope`. Arrays hold entities,
relationships and collections; arbitrary JSON values are allowed in `properties`
and `metadata`. IDs are opaque stable strings. A directed or undirected relationship
has its own ID: parallel, reciprocal and self relationships are all valid.

```json
{
  "schemaVersion": 2,
  "id": "project-1",
  "title": "Example",
  "metadata": {},
  "entities": [
    {"id":"a","kind":"person","label":"Alice","x":0,"y":0,"properties":{"active":true},"metadata":{},"notes":"","sources":[]},
    {"id":"b","kind":"organization","label":"Example","x":180,"y":0,"properties":{},"metadata":{},"notes":"","sources":[]}
  ],
  "relationships": [
    {"id":"r1","source":"a","target":"b","kind":"member","label":"Member of","directed":true,"properties":{},"metadata":{},"notes":"","sources":[]}
  ],
  "collections": [{"id":"g1","label":"People","members":["a"],"color":"#8096bd"}]
}
```

Structured imports accept the same entity/relationship/collection arrays without
the project header. Generic JSON arrays and CSV rows become independent records
with the original data in source attribution. CSV uses headers and quoted fields
including embedded newlines. TXT extracts conservative URL/email/IP/domain
candidates, retaining context and warning about ambiguity. No relation or real-world
identity is inferred from proximity or repeated labels. Imported graph IDs are
remapped consistently at acceptance to protect existing records.

Source records contain `id`, `label`, optional `uri`, `retrievedAt` and raw JSON
`record`. Provenance is editable as part of the portable format; the UI displays
it read-only. Original full records survive merge in `metadata.mergedRecords`.

The migration loader accepts schemaVersion1 (or version1) with nodes/edges/groups
and maps the documented legacy value/attributes/relation/provenance fields. It
preserves confidence/timestamp as metadata and raw records as source records.
Unknown/future schemas are rejected. Legacy SQLite databases are not silently
converted: export JSON from the legacy app before importing/migrating.

Graph operations validate their final topology before applying a patch, including
incident relationship removal and collection membership. Normalized deltas include
the effects needed by the renderer and inverse history. Undo history holds at most
64 operations with a soft64MiB bound; the newest operation is kept even if larger.
Undo history does not cross closing/reopening a project.
