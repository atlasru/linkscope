# LinkScope 2.0 architecture

## Assessment and decisions (2026-10-16)

The original source is preserved in `legacy/` at upstream commit
`53c672af7f7e879b493c1d830c41dfce3ace8aa7`. A separate, non-Git local tree
is preserved in `preserved-local/`, excluding only generated dependencies and
build artifacts. Neither tree is the replacement's source directory.
These are local development copies, excluded from Git. For repository users,
the [legacy branch](https://github.com/atlasru/linkscope/tree/legacy) preserves
the original application and its history at the same commit.

Discard the OSINT-shaped identity model, deterministic identity merging, monolithic
frontend, storage-coupled interactive edits, and renderer-owned layout. Retain the
use of Rust, Tauri 2, explicit outbound research actions, and offline operation.
The detailed source audit is in LEGACY-AUDIT.md.

## Replacement

- `crates/engine`: generic in-memory property multigraph, indexed adjacency,
  validated atomic patches, inverse-patch undo/redo, versioned portable projects,
  recoverable atomic persistence and conservative import preview. No networking,
  renderer or investigation-specific entity types in the graph engine.
- `crates/transforms`: independent asynchronous public-data services. Structured
  output is previewed before a single undoable graph patch is accepted.
- `src-tauri`: narrow asynchronous IPC adapter; serializes mutations and returns
  deltas. OS file dialogs and project paths stay in Rust. No general shell or
  arbitrary frontend filesystem access.
- `ui/src`: strict TypeScript editor, view cache updated by deltas, independent
  spatial index, demand-driven Canvas2D renderer, cancellable layout worker.

### Contracts

JSON uses camelCase. IDs are opaque stable strings, never inferred from labels.
`Entity`: id, kind, label, x, y, properties (JSON object), metadata (JSON object),
notes (string), sources (SourceRef[]).
`Relationship`: id, source, target, kind, label, directed, properties, metadata,
notes, sources. Parallel, reciprocal and self relationships are supported.
`SourceRef`: id, label, uri (optional), retrievedAt (optional), record (optional JSON).
`Collection`: id, label, members (entity IDs), color (string).
`Project`: schemaVersion=2, id, title, entities[], relationships[], collections[],
metadata (JSON object). Working state also has a monotonically increasing revision.
`Patch`: entities[], relationships[], collections[], removeEntities[],
removeRelationships[], removeCollections[]. All fields default empty. Patches
are validated before mutation; entity deletion captures incident edges and
collection membership for undo. `Delta` includes normalized patch fields plus
revision, canUndo, canRedo. Undo history is bounded and not part of portable files.

### Tradeoffs and research

Tauri 2 is retained over Electron (bundled Chromium cost) and egui (rich editor
forms/accessibility and renderer portability effort). Rust owns authoritative
state; the WebView contains a render cache, not a second persistence engine.
[Tauri IPC](https://v2.tauri.app/concept/inter-process-communication/) describes
the asynchronous command boundary.

Small framework-free TypeScript modules avoid per-node reactive component trees.
Canvas2D is the initial compatibility renderer: demand-driven drawing, spatial
viewport queries, edge and label budgets, indexed picking, and explicit LOD.
WebGL2 is evaluated but deliberately deferred until an editable baseline is
measured. [Sigma's rendering architecture](https://www.sigmajs.org/concepts/rendering/)
informs batch geometry and LOD design; using Sigma does not itself prove the target
scale. Canvas budgets mean a zoomed-out view may omit edges/labels. Benchmarks
must distinguish graph storage, visibility queries and actual drawing.

Hash maps plus adjacency sets are used instead of a duplicate petgraph topology:
the model needs per-edge direction, stable external IDs and rich editable records.
[StableGraph](https://docs.rs/petgraph/latest/petgraph/stable_graph/struct.StableGraph.html)
offers stable slots and O(V+E) space but would still need external-ID indexes and
mixed-direction semantics. Reassess compact slot storage after measurements.

Obsidian's [sample plugin](https://github.com/obsidianmd/obsidian-sample-plugin)
illustrates contextual commands and lifecycle cleanup. LinkScope takes progressive
disclosure and persistent collapsible workspaces, not its appearance or plugin API.

## Implementation order

1. Engine invariants, atomic history and persistence; strict contracts.
2. Editable canvas, inspector, direct selection/drag/link, keyboard commands.
3. Previewed CSV/JSON/TXT imports with source records and no inferred identity.
4. Explicit DNS/RDAP/certificate jobs with cancellation, limits and attribution.
5. Native Windows build, interaction tests, recovery tests and reproducible target-size measurements.

Production readiness is determined by recorded validation, not this design document.
