# Native engine measurements — 2026-10-16

Reproducible dataset: 100,000 rich entities and 500,000 mixed directed/undirected
relationships, deterministic ring chords, stable `n*`/`e*` IDs, ordinal property
on every entity. No generated dataset is shipped as application content.

Environment: native Windows x86_64, AMD Ryzen 5 4500U, Rust 1.99.1 GNU host,
existing MSYS2 MinGW compiler, release profile with thin LTO and one codegen unit.
These are single-run wall times on the development computer while other build
work was occurring. They are neither statistical latency guarantees nor a claim
that this dataset can be rendered in full at interactive frame rates.

Final run after bounded-buffer project parsing and 1 MiB buffered writes:

| Operation | Milliseconds |
| --- | ---: |
| Generate deterministic dataset | 542.773 |
| Validate and apply initial bulk patch | 2,652.747 |
| Edit one entity | 0.035 |
| Move 1,000 entities in one patch | 2.084 |
| Delete 100 entities and incident edges | 2.273 |
| Undo deletion, restoring topology | 2.585 |
| Build sorted portable snapshot | 1,030.654 |
| First atomic save | 1,241.638 |
| Replace save, validate old file, preserve backup | 3,604.350 |
| Load and validate portable file | 1,472.440 |
| Construct reopened working graph | 2,307.126 |

Portable JSON size: **90,083,009 bytes** (approximately 85.9 MiB).

Windows `Get-Process` measurements for the benchmark process:

| Measurement point | Working set | Peak working set so far |
| --- | ---: | ---: |
| One working graph, after initial patch delta is dropped | 485.9 MiB | 824.4 MiB |
| Original graph + portable snapshot + reopened graph | 1,158.1 MiB | 1,387.4 MiB |

Working set is resident process memory, not a precise Rust heap allocation count.
The peak includes temporary mutation/serialization buffers. This makes the cost
of retaining a snapshot alongside two working graphs visible; it should not be
interpreted as the memory required by one steady-state graph.

An earlier run with per-byte reader deserialization and 8 KiB writer buffering
measured initial save 4,029.374 ms, replacement/backup 23,730.397 ms and load
5,233.151 ms. Those results motivated the persistence changes. System load also
varied, so the difference cannot be attributed exclusively to those changes.

## Reproduce

With a normal native Rust development environment:

```powershell
cargo test -p linkscope-engine
cargo run -p linkscope-engine --release --example benchmark
```

This computer initially had no MSVC C++ linker. The measured GNU command was:

```powershell
$env:PATH = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-gnu\bin;C:\msys64\mingw64\bin;" + $env:PATH
$env:CARGO_TARGET_DIR = 'target/engine-benchmark'
& "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-gnu\bin\cargo.exe" test -p linkscope-engine --release --offline
& "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-gnu\bin\cargo.exe" run -p linkscope-engine --release --example benchmark --offline
```

The final suite passed **30 tests**: 12 graph/history/invariant tests, 9 import
tests, and 9 persistence/recovery/migration tests. None were ignored. Tests and
benchmark required filesystem access outside the execution sandbox because its
Windows temporary-file rename restrictions returned access denied; ordinary
native Windows runs passed atomic replacement and recovery tests.

## Practical limits

Interactive small patches avoid graph snapshots and complete in milliseconds in
this benchmark. Save/open remain proportional to the complete graph. Autosave
must run outside the editor's mutation lock, coalesce requests, and avoid keeping
unneeded snapshots alive. Incremental durable journaling is a future priority.

These timings exclude IPC serialization, WebView cache memory, layout, visibility
queries, Canvas2D drawing, frame rate, and full desktop end-to-end latency. Measure
those independently before claiming target-scale desktop responsiveness. This
benchmark also does not stress dense collection memberships or oversized undo
operations; the newest undo operation can exceed the soft 64 MiB history budget.
