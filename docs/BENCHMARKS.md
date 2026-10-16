# Measured performance — 2026-10-16

The native engine, spatial indexes, and actual Canvas renderer were measured
separately. **The 100,000-entity / 500,000-relationship dataset loads and renders;
adaptive detail gives a fast sampled viewport in the measured Edge run, while
initial opening and the full-scene overview remain bottlenecks.** These
measurements do not establish smooth rendering of every element at once or
end-to-end desktop latency.

Runs used the Windows development computer, an AMD Ryzen 5 4500U. Earlier runs
overlapped build and test work; the final adaptive-renderer run followed the long
native compilation. These are development measurements, not controlled
comparative trials or performance guarantees.

## Native graph and persistence

The release Rust benchmark uses 100,000 rich entities and 500,000 mixed directed
and undirected relationships with deterministic ring chords. Full methodology,
memory measurements, environment, and commands are in
[`crates/engine/BENCHMARK.md`](../crates/engine/BENCHMARK.md).

| Operation | ms |
| --- | ---: |
| Apply and validate initial bulk patch | 2,652.747 |
| Edit one entity | 0.035 |
| Move 1,000 entities in one patch | 2.084 |
| Delete 100 entities and incident relationships | 2.273 |
| Undo deletion | 2.585 |
| Build portable snapshot | 1,030.654 |
| First atomic save | 1,241.638 |
| Replace save, validate old file, preserve backup | 3,604.350 |
| Load and validate portable file | 1,472.440 |
| Construct reopened working graph | 2,307.126 |

Portable JSON occupied 90,083,009 bytes (85.9 MiB). The benchmark process working
set after dropping the initial delta was 485.9 MiB; retaining an original graph,
portable snapshot, and reopened graph together reached 1,158.1 MiB. These resident
process measurements are not precise heap-allocation counts. They exclude the
WebView, IPC serialization, drawing, and layout.

## Spatial indexes

[`benchmark-spatial.json`](benchmark-spatial.json) records Node 24.18.1 on Windows
x64. Its reproducible lattice uses 100,000 points and 500,000 edge bounds: four
local relationships and one deterministic long relationship per entity. Queries
cover 1,000 × 700 world units; 10 warm-up samples precede 50 measured samples.

| Operation | Median ms | p95 ms |
| --- | ---: | ---: |
| Point viewport query (mean 1,129 results) | 0.101 | 1.708 |
| Complete edge-bounds query (mean 23,955 candidates) | 29.865 | 64.843 |
| Spatially distributed visual edge sample (up to 7,000) | 17.059 | 30.196 |

Building the point index took 159.740 ms; Morton bulk construction of the edge
hierarchy took 2,245.853 ms. Updating 1,000 point entries took 7.193 ms, excluding
incident-edge geometry updates. The process reported 487.5 MiB RSS and 348.6 MiB
heap used, including temporary benchmark allocations without forced collection.
These figures must not be added to native working-set figures as if both measured
the same process or allocation boundary.

The edge query is a broad phase: long relationships can produce many false
positives. Exact curved-edge picking has additional cost. Point picking and
rectangle selection query the complete point index; drawing budgets do not
delete or truncate the working graph.

## Actual Canvas2D and worker

[`benchmark-renderer.json`](benchmark-renderer.json) records the final measured
Canvas implementation in headless installed Microsoft Edge 155 on Windows,
1,280 × 800 pixels, device-pixel ratio 1. A blank same-origin fixture loads the
actual renderer and worker modules from the development server; it avoids Vite
hot reload and unrelated application startup. This is **not a Tauri WebView2
host measurement**.

The dataset is a 400-column, 24-unit lattice with 100,000 entities and 500,000
relationships, including 20% deterministic long relationships. All entities and
relationships are retained. Visual level of detail deliberately samples nodes
and edges; label collision suppression keeps dense labels readable. Adaptive
budgets aim for 25 ms CPU submission, with hysteresis, minima of 1,000 nodes and
250 edges, and at most three refinement frames after an external invalidation.
Rendering stops at rest. Selected objects and exhaustive picking bypass visual
sampling; unusually large selections can therefore remain expensive.

| Measurement | Result |
| --- | ---: |
| Construct frontend record maps | 619.4 ms |
| Build renderer cache and indexes synchronously | 11,603.8 ms |
| Cold overview: 100,000 visible / 20,000 drawn nodes, 1,500 edges | 85.2 ms |
| Settled overview: 100,000 visible / 1,000 drawn nodes, 250 edges | 66.7 ms |
| Normal zoom: 1,925 visible / 963 drawn nodes, approximately 246 edges and 201 labels, 20 settled samples | median 15.6 ms / p95 19.3 ms |
| Background layout: 60 iterations, complete graph | 13,167.3 ms |
| Worker output | 100,000 positions, all finite |

Frame timings measure demand-driven CPU drawing submission, including visibility
queries and labels; they do not guarantee GPU presentation time or sustained
frame rate. Samples wait until the finite refinement window settles. The full
overview exceeded the 25 ms target even at minimum drawing budgets because
visibility queries still enumerate all 100,000 visible points. The worker
measurement includes input/output preparation and worker execution. Its result
was validated but not applied during this benchmark. Integration tests separately
check cancellation and application callbacks.

Actual captures are [`benchmark-renderer-viewport.png`](benchmark-renderer-viewport.png)
and [`benchmark-renderer-overview.png`](benchmark-renderer-overview.png). They are
generated benchmark data, not sample content shipped in projects.

The earlier pre-batching result is preserved in
[`benchmark-renderer-before-batching.json`](benchmark-renderer-before-batching.json);
an experiment with one giant drawing path is preserved in
[`benchmark-renderer-full-batching.json`](benchmark-renderer-full-batching.json).
That experiment regressed dense-view drawing and motivated bounded geometry
batches. The fixed-budget result before adaptive detail is preserved in
[`benchmark-renderer-before-adaptive.json`](benchmark-renderer-before-adaptive.json).
Adaptive drawing trades visible detail for lower submission cost; the result
must be read together with its actual drawing counts. Machine load also varied
substantially between runs, so these reports do not support a controlled causal
claim about each optimization. The latest JSON is the current baseline.

## Reproduce

```powershell
cargo run -p linkscope-engine --release --example benchmark
$env:BENCH_OUTPUT = 'docs/benchmark-spatial.json'
npm run benchmark:canvas
```

Start `npm run dev` in one terminal. In another terminal on Windows with Microsoft
Edge installed:

```powershell
npm run benchmark:renderer
```

`BENCH_URL` selects a running server if it is not `http://127.0.0.1:1420`;
`BENCH_OUTPUT` selects the JSON output path. The native GNU toolchain commands
used on this computer are recorded in the engine report. Browser profile creation
and native atomic-file tests required execution outside this environment's Windows
sandbox; ordinary native operations were tested separately from sandbox failures.

The isolated Canvas interaction suite can be repeated with WebGL disabled:

```powershell
$env:LINKSCOPE_TEST_URL = 'http://127.0.0.1:1420'
npx playwright test --config scripts/canvas-playwright.config.ts
```

This configuration passes `--disable-webgl`; the tests explicitly verify that a
WebGL2 context is unavailable while exercising Canvas2D editing. The renderer
does not depend on WebGL2. This verifies the compatibility path in Edge; it does
not establish compatibility with every platform WebView.

The final isolated run passed all six interaction tests in 21.1 seconds with
WebGL disabled: multiselection/movement/creation, edge and loop picking, linking
and marquee selection, anchored zoom and panning, worker completion/cancellation,
parallel-edge picking after a delta, and bounded adaptive refinement that stops
at rest without dropping selected nodes. The seven pure spatial-index tests also
passed, including balanced-tree invariants, bulk-load edits, complete queries,
negative coordinates, and distributed visual sampling. Small-fixture interaction
tests do not establish full target-size end-to-end editing latency.

## Priorities exposed by measurements

1. Move bulk render-cache/index construction off the UI thread and stream initial
   project data; current target-size opening visibly blocks for seconds.
2. Sample visible point buckets directly for drawing while retaining complete
   indexes for picking, and reduce dense-edge broad-phase costs. Evaluate compact
   endpoint buffers and a GPU renderer while retaining the verified Canvas2D path.
3. Profile native WebView2 drawing and IPC with controlled machine load. The
   adaptive viewport result is promising, but full overview latency remains high.
4. Improve layout quality and time on disconnected/dense datasets. The current
   approximate grid-aggregate force method is cancellable and linear per
   iteration, but a 13-second run is not immediate.
5. Reduce rich-record duplication and snapshot retention, and evaluate incremental
   persistence journaling. Current memory and save/open costs remain material.
