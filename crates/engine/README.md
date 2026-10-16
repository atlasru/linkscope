# LinkScope graph engine

The engine has no network, UI, renderer, or investigation-specific dependency.
`Graph` owns rich records in hash maps and indexes incident edges by stable ID.
Directed, undirected, parallel, reciprocal, and self relationships coexist.

`Graph::apply(Patch)` validates the transaction's final topology before mutation.
Entity upserts replace complete records. Entity deletion also removes incident
relationships and prunes collection membership. Its returned `Delta.patch`
includes those cascades so the frontend updates its cache without a snapshot.
Duplicate IDs within an operation, update/remove conflicts, missing references,
invalid IDs, and non-finite coordinates reject the entire patch.

Undo/redo stores inverse patches rather than whole projects. History retains at
most 64 operations, evicting older operations above a soft 64 MiB serialized-data
budget. The newest operation is always retained to keep a bulk deletion undoable;
one large operation can therefore exceed that byte budget. History is not saved.
Changing the project title increments the revision but does not enter graph history.

## Portable projects

The UTF-8 JSON v2 contract is defined in `src/model.rs`. Fields use camelCase.
Properties and metadata preserve arbitrary JSON values. A valid project includes
`schemaVersion: 2` and a nonempty project `id`. Portable snapshots sort records by ID.

Actual v1 exports from the legacy application are supported:

```json
{
  "version": 1,
  "nodes": [{"id":"a","kind":"domain","value":"example.org","attributes":{},"sources":[],"confidence":"data","timestamp":123,"x":0,"y":0}],
  "edges": []
}
```

Migration maps `value` to `label`, `attributes` to properties, confidence and
timestamps to `legacyConfidence`/`legacyTimestamp` metadata, string sources to
source references, and edge `relation`/`provenance` to relation kind/label and
source references. Complete original legacy records remain attached as source
records. Legacy exports without a project ID get one on opening. The migration
also accepts v1 portable `entities`/`relationships`/`collections` record names or
`nodes`/`edges`/`groups` containing the current record shape. Ambiguous duplicate
aliases and invalid/dangling topology are errors. Loading never modifies v1 files.

Save validates the project and any existing destination, writes and flushes a
same-directory temporary file, preserves the previous valid destination in
`<path>.bak`, stages `<path>.pending`, and atomically renames it over the target.
Load tries the committed file, then a pending copy, then the backup. A valid
committed file takes precedence over an interrupted pending save. Recovery is
reported in `LoadOutcome.recoveredFrom`; no implicit repair overwrites the
damaged original. Save to a new filename after recovery. Directory-entry fsync
is used on Unix; standard Windows file flush and atomic rename do not constitute
a guarantee against every hardware/power failure. Files are capped at 512 MiB.

## Imports

Every import is a preview requiring the caller to accept a single graph patch.
JSON structured graph objects require explicit IDs/endpoints and preserve the
declared topology. JSON record arrays and CSV rows always get fresh entity IDs,
even when records have coincident `id` fields. CSV supports quoted commas,
escaped quotes, CRLF, and quoted newlines; duplicate headers and malformed rows
are errors. CSV does not infer relationships. TXT extracts separate URL, email,
domain, and validated IPv4/IPv6 occurrences. It neither merges repeated mentions
nor invents links. Sources retain original structured records or bounded text
context with byte offset and line number. Extraction is deliberately conservative,
requires review, and has no AI/service dependency. Import limits are 32 MiB,
100,000 entities, and 500,000 explicit relationships per preview. The in-memory
graph limits are 1 million entities and 5 million relationships.

## Validation and benchmarks

```powershell
cargo test -p linkscope-engine
cargo run -p linkscope-engine --release --example benchmark
```

The deterministic benchmark defaults to 100,000 entities and 500,000 edges.
Pass an entity count as the final argument to change its size. It measures bulk
apply, small edits, movement, cascading deletion/undo, snapshots, saving, loading,
and reopened-graph construction. On Windows it records this process's actual
working set and peak working set; on Linux it reports `/proc` RSS/HWM. It does
not measure rendering, IPC serialization, frame rate, or interaction latency.

This Windows environment initially lacked the MSVC linker. Engine tests can run
with the installed GNU Rust host and existing MinGW toolchain:

```powershell
$env:PATH = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-gnu\bin;C:\msys64\mingw64\bin;" + $env:PATH
& "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-gnu\bin\cargo.exe" test -p linkscope-engine --offline
```

The execution sandbox denied Windows temporary-file rename operations. Recovery
tests were rerun with ordinary local filesystem access and passed; failures were
not suppressed or marked ignored.
