# LinkScope 2.0

A local graph editor and investigation workbench. The replacement source lives
in `crates/engine`, `crates/transforms`, `src-tauri`, and `ui`. No account, AI service,
analytics or telemetry is used. External research is an explicit contextual action
with a named destination and a result preview.

This is an implemented foundation, not a declaration of production readiness.
See [validation](docs/VALIDATION.md) for measured results and outstanding work.

## Preserve and inspect

- The [legacy branch](https://github.com/atlasru/linkscope/tree/legacy) preserves
  the older application at `53c672af7f7e879b493c1d830c41dfce3ace8aa7`, including
  its full commit history.
- During development, independent source copies were also retained locally in
  `legacy/` and `preserved-local/`. Those local backups and generated dependencies
  are excluded from this branch.
- [Source audit](docs/LEGACY-AUDIT.md) and [architecture](docs/ARCHITECTURE.md).

The replacement advances the original repository history. The older application
remains available on `legacy`; no history rewrite is required.

## Develop on Windows

Install Node 22.12+ (Node24 tested), Rust stable, Edge WebView2 Runtime, and native
Rust build prerequisites. Standard Tauri development uses MSVC C++ Build Tools and
a Windows SDK. This machine also has `C:\msys64\mingw64`; a **full GNU host** Rust
toolchain can be used when the MSVC linker is missing:

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu
npm.cmd ci
.\scripts\windows.ps1 Check
.\scripts\windows.ps1 Dev
.\scripts\windows.ps1 Build
```

The script selects the available GNU host if installed; otherwise it uses the
normal configured Rust toolchain. Outside that environment:

```powershell
npm.cmd ci
npm.cmd run desktop
npm.cmd run desktop:build
```

`npm run dev` is a frontend development server. File access and graph mutation
require the desktop IPC; a plain browser reports that requirement explicitly.
It does not simulate saved projects. `desktop:build` builds an executable without
installers, signing, releases, tags or publication.

The validated standalone development build is:

```powershell
cargo build --locked -p linkscope-desktop --features custom-protocol
cargo test --locked -p linkscope-desktop --features custom-protocol
```

Keep `WebView2Loader.dll` beside the resulting
`target/debug/linkscope-desktop.exe`. The native executable and its Rust IPC have
been exercised; optimized installer/release packaging has not been validated.

## Use

Double-click empty canvas or press N to add an entity. Drag to move; drag empty
space to pan; wheel to zoom. Shift-click or Shift-drag selects several objects.
L connects two clicked entities; V returns to selection. Select a relationship
on the canvas to edit its type, properties or direction. Edits are applied from
the inspector; Delete and Ctrl+Z remove and restore records and incident links.

Ctrl+K opens searchable commands with shortcuts. Ctrl+G groups a selection.
Merge preserves original records in metadata; Split explicitly transfers selected
properties to a new identity. Duplicate creates a separate copy. Collections can
be selected and ungrouped in the explorer. Panels resize at their dividers and
collapse with the header buttons or Ctrl+B / Ctrl+Shift+B.

Import UTF-8 JSON, CSV or TXT through Import, or use `Import pasted text` in
commands. CSV record labels and types can be mapped in the preview. Text extraction
produces candidate observations without invented relationships. Explicit graph
JSON uses `entities`, `relationships`, and optional `collections`; see
[format](docs/FORMAT.md). External IDs are remapped when importing into an existing
project, so equal labels/IDs never silently merge unrelated records.

Select a `domain` entity for DNS, RDAP or certificate observations; available
commands disclose their destination before running. The full project is never
sent. Jobs can be cancelled; results require an explicit Add to graph action.
Each accepted result/import is one undoable edit. Public service errors are real
errors and are not replaced with example results.

Ctrl+S saves a portable `.linkscope` JSON project. Ctrl+O reopens one. Autosave
stores the active project in Tauri's platform app-data directory for
`local.linkscope.v2` every three seconds when changed and on normal close. A
previous valid generation is retained in `.bak`; interrupted `.pending` saves can
be loaded. Corrupt originals are retained, and recovered projects should be saved
under a new filename. New/Open archives the previous nonempty working graph in
the app-data `recoveries/` folder. Undo history stays in memory. Panel dimensions,
visibility and camera persist separately. One process owns an app-data directory.

## Validate

```powershell
.\scripts\windows.ps1 Test
cargo test --locked -p linkscope-engine -p linkscope-transforms
npm.cmd test
npm.cmd run test:ui
npm.cmd run build
.\scripts\windows.ps1 Benchmark
node node_modules/tsx/dist/cli.mjs scripts/benchmark-renderer.ts
```

For the native WebView/Rust interaction smoke, first build the debug executable
with `--features custom-protocol`, then run `.\scripts\native-smoke.ps1`.
It uses a fresh test directory and actual IPC, never a browser adapter. Windows
file dialogs still require a manual check; this runner does not assert they work.
For a deliberate crash/reopen check, run the debug executable with `--data-dir`
pointing to an isolated native smoke fixture, `--smoke-test` pointing to a result
JSON path, and `--smoke-restore`; it verifies the expected persisted fixture.

Rust tests cover graph invariants/history, migration, corruption/recovery, imports,
and transformation concurrency, rate limiting, cancellation and timeout. Browser
tests exercise real Canvas2D/worker interactions and DOM wiring with an explicitly
test-only IPC adapter. Native smoke validation must separately exercise the actual
WebView and Rust boundary. Performance numbers separate graph operations, CPU
visibility queries and actual Canvas2D frames; none implies every target-size
element is rendered smoothly at once.

The current native file-dialog implementation is Windows-specific. macOS/Linux
desktop packaging and dialogs remain a porting task. CI checks the new engine,
frontend and desktop packages. Build jobs upload artifacts without creating
releases or changing repository settings.
