# Validation record — 2026-10-16

Validation workspace: `linkscope-recode` on the Windows development computer.

## Implemented

- Generic Rust property multigraph with stable IDs, mixed relationship direction,
  parallel/self edges, provenance, notes, metadata and collections.
- Validated atomic patches, cascade deletion, undo/redo, bulk changes and incremental
  frontend deltas. No label-based identity merging.
- Versioned portable JSON, actual legacy record migration, atomic replacement,
  backups/pending recovery, corrupt-file preservation, process ownership lock and
  dirty-only autosave. New/Open archives the previous working project.
- Canvas2D editor: indexed selection/picking/culling, pan/zoom, multi-drag,
  rectangle selection, parallel/self relationship display, directed arrows, LOD,
  cancellable layout worker, and demand-driven rendering.
- Compact dark workspace, searchable commands, shortcuts, editable inspector,
  entity search/type filter, grouping, merge, split and duplicate; persistent
  collapsible/resizable panels and camera.
- JSON/CSV/TXT previews, CSV label/type mapping, source records, explicit acceptance.
- Independent DNS, RDAP and certificate-observation jobs with bounded concurrency,
  cancellation, timeout, rate gate, bounded TTL cache, response caps and attribution.

## Confirmed checks

- Engine: **30/30 tests pass** using native Windows GNU Rust, release profile.
  Graph invariants/history, import parsing, legacy migration and interrupted/corrupt
  persistence recovery are covered. No skipped/ignored cases.
- Transformations: **16/16 deterministic tests pass**, native Windows GNU Rust.
  Controlled concurrency, queue/rate cancellation, deadline, bounded body/cache,
  safe rate-limited redirects, parsing, fresh identities and attribution tested.
- Public fixture live smoke: example.com DNS returned4records/4edges in1.268s;
  RDAP1record/1edge in2.283s; crt.sh81records/81edges in10.504s. No project data
  was used. Availability and counts are observations, not service guarantees.
- TypeScript strict build passes; cache/spatial/queue tests pass **17/17**.
  Browser Canvas/worker tests pass **6/6 with WebGL disabled**, including adaptive
  budgets and demand-driven settling. The final combined browser suite passes
  **10/10** (six canvas/worker cases and four editor DOM cases). Editor DOM wiring
  uses an explicitly test-only IPC adapter. Across the Rust and TypeScript suites,
  **79 automated tests passed**, none failed or ignored in their final runs.
- Native Windows debug executable builds with bundled frontend assets via
  `cargo build --locked --offline -p linkscope-desktop --features custom-protocol`.
  Keep its generated WebView2Loader.dll alongside the executable. Six desktop
  regression tests pass (session revisions, save failure, job retention,
  corrupt/pending recovery and autosave revision tracking).
- Actual standalone WebView2/Rust smoke passed: dialog-driven entity creation,
  relationship creation, inspector edit, cascade delete/undo, real CSV parser and
  actual Canvas2D surface. This is distinct from the test-only browser adapter.
  Final evidence: [NATIVE-SMOKE.json](NATIVE-SMOKE.json).
- Actual termination/reopen check passed: after verifying a dirty autosave,
  the isolated process was stopped without normal close, then the standalone app
  reopened the same directory. All four entities, one relationship, edited label
  and imported analyst property survived. Evidence:
  [NATIVE-RECOVERY.json](NATIVE-RECOVERY.json). These fixture directories live under
  target/native-tests, outside Playwright's cleanup directory.
- Target-size engine benchmark executed on Windows: **100,000 entities and
  500,000 relationships**. Full raw timings, memory and reproducible commands:
  [engine benchmark](../crates/engine/BENCHMARK.md). Renderer results are recorded
  separately in benchmark-renderer.json and benchmark-spatial.json.
  [Consolidated measurements](BENCHMARKS.md): target-size Rust single edit0.035ms,
  replacement save3.604s; graph working set485.9MiB. Adaptive Canvas normal view
  median15.6ms/p9519.3ms while drawing963nodes/~246edges. Initial frontend indexing
 11.60s and settled full overview66.7ms remain material limits. These are separate
  engine/Edge measurements, not a target-size native end-to-end guarantee.

## Current limitations and next priorities

1. Target-size editor opening still does substantial synchronous frontend index
   construction and a full initial JSON IPC transfer. Move loading/indexing to a
   worker and introduce paged/binary view projections before claiming immediate
   target-size opening. Subsequent edits already use patches.
2. Canvas LOD intentionally omits some nodes/edges/labels at overview scales.
   Picking/search retain full identity. Dense overlapping long-edge picking can
   remain expensive. No WebGL/WebGPU backend has been implemented or benchmarked.
3. Full project snapshots are memory-intensive and much slower than small graph
   edits. Introduce an append-only edit journal/checkpoint strategy before large
   investigations can have cheap frequent durable autosave. Current I/O is off
   the UI thread; snapshot capture still takes a graph lock.
4. Native file dialogs are implemented but their manual operation is unverified:
   Computer Use app-access approval timed out before a window snapshot could be
   obtained. No signed installers, optimized desktop release build, update system,
   cross-platform native dialogs, macOS/Linux
   desktop validation, screen-reader audit or prolonged crash/fault-injection soak.
   Windows is the implemented native-dialog platform.
5. No arbitrary-code plugin runtime, custom service-definition UI, credential vault,
   SQLite legacy database conversion, GraphML/STIX/MISP export, archive formats or
   rich report generation. These were not reproduced from the legacy app.
6. Public-data services are fixed integrations. No external AI or implicit network
   activity. RDAP/certificate observations do not assert common ownership.
7. JSON property editing is strict and functional but needs richer typed controls
   for casual users. Source metadata displays read-only in the inspector. Undo
   history is in memory and resets on reopening; title changes are outside graph
   history. Recovery archives are retained without an automatic cleanup policy.

## Preservation and publication

The replacement was initially developed locally beside preserved `legacy/` and
`preserved-local/` source trees, without commits or remote changes. After native
validation, the user explicitly requested GitHub publication and approved updating
the workflows for the new packages and tests. The remote `legacy` branch was
created at 53c672af7f7e879b493c1d830c41dfce3ace8aa7 before preparing the replacement
commit on `main`. Publication uses a normal descendant commit, without force pushes,
tags, releases, pull requests or repository-setting changes. Local backup folders,
build outputs and test workspaces are excluded from the published tree.

All 41 preserved local source files matched the original tree byte-for-byte when
checked. SHA-256 evidence: [preservation-check.json](preservation-check.json).
`git -C legacy status --porcelain` was empty and its HEAD remained
53c672af7f7e879b493c1d830c41dfce3ace8aa7.
