# LinkScope

**Fast, local-first OSINT link analysis for the desktop.**

LinkScope is an open-source desktop application for investigating relationships between domains, IP addresses, usernames, URLs, email addresses and other entities.

It combines an interactive graph workspace with OSINT transforms, local search and privacy-aware networking — without requiring a cloud account or sending your investigation graph to a central service.

> **Status:** LinkScope is currently an early engineering release. Core functionality works and desktop builds are produced for Windows, Linux and macOS, but the project is still under active development.

## Features

### Interactive link analysis

Build investigations as graphs of entities and relationships.

- Interactive WebGL2 graph visualization
- Entity inspector and context actions
- Search across graph data
- Time-based filtering
- Automatic community detection
- Visual clustering
- Stable entity identifiers and deduplication
- Relationship provenance and observation timestamps

LinkScope is designed to keep large investigations responsive by storing graph topology in memory while moving colder data to local storage when necessary.

### OSINT transforms

Run transforms directly from entities in the graph and automatically add discovered relationships.

LinkScope supports built-in sources as well as declarative HTTP source plugins. Multiple transforms can run concurrently and can be cancelled without discarding results that have already been committed.

See [docs/sources.md](docs/sources.md) for the current source matrix.

### Local-first by design

Investigation data stays on your machine.

LinkScope uses:

- SQLite WAL for persistent graph storage
- Tantivy for local full-text search
- Local transaction journaling for recovery
- Local encrypted storage for secrets
- No mandatory account
- No central LinkScope backend

An `--ephemeral` mode is also available for investigations that should not use LinkScope's normal persistent graph, index, configuration, plugin, log or key files.

### Privacy-aware networking

Network routing can be configured independently for different sources.

Supported modes include:

- Offline
- Direct HTTPS
- HTTP/HTTPS proxy
- SOCKS5 with remote DNS (`socks5h`)
- DNS-over-HTTPS for explicitly enabled direct connections

LinkScope starts in **Offline** mode. Network access must be explicitly configured.

Example SOCKS proxy:

```text
socks5h://127.0.0.1:9050
```

LinkScope does not currently bundle Tor or another anonymity network. A configured proxy must already be available on the system.

### Export

Investigations can be exported to common formats:

- JSON
- GraphML
- STIX 2.1
- MISP Event JSON

Exports are streamed from local storage instead of requiring another complete copy of the investigation graph in memory.

## Desktop builds

GitHub Actions currently produces packages for:

| Platform | Package |
| --- | --- |
| Windows x64 | NSIS `.exe` |
| Linux x64 | `.deb`, `.AppImage` |
| macOS Apple Silicon | `.dmg` |

Current build artifacts are available from [GitHub Actions](https://github.com/atlasru/linkscope/actions).

Builds are currently unsigned. macOS notarization is not configured yet.

## Getting started

1. Launch LinkScope.
2. Add an entity such as a domain, IP address, username, URL or email address.
3. Open **Settings** if the investigation requires network access and select a route.
4. Select an entity.
5. Run an available transform from the inspector or context menu.
6. Explore newly discovered entities and relationships on the graph.

The built-in demo creates a synthetic graph and does not perform network requests or modify the active investigation.

## Build from source

### Requirements

- Rust stable
- Tauri 2 platform dependencies
- Tauri CLI

Then:

```sh
cargo test -p linkscope-core
cargo install tauri-cli --version '^2' --locked
cargo tauri dev
```

Create a release build with:

```sh
cargo tauri build
```

The frontend can alternatively be started through npm:

```sh
npm install
npm run dev
```

Linux builds require the standard Tauri WebKitGTK dependencies. Windows uses the system WebView2 runtime.

## Ephemeral mode

Run LinkScope without opening its normal persistent investigation files:

```sh
./target/release/linkscope --ephemeral
```

SQLite and Tantivy operate in memory in this mode. Explicit exports still write files when requested by the user.

Ephemeral mode only controls LinkScope-managed persistence. It cannot guarantee that the operating system, WebView runtime, swap, crash dumps or other system components leave no traces.

## Plugins

Additional HTTP sources can be described using JSON manifests without recompiling the core application, provided the required response parser already exists.

Example plugin manifests are available in [`plugins/`](plugins/).

Currently supported parser types include:

`doh`, `crt`, `certspotter`, `internetdb`, `urlscan`, `cdx`, `attributes`, `pointer`

Plugin manifests are configuration, not executable native code. They can still send an entity value to their declared remote service, so only install manifests you trust.

## Architecture

LinkScope is built around a Rust core and a lightweight Tauri desktop interface.

```text
Tauri UI
   │
   ▼
Rust / Tokio
   │
   ├── Property graph
   ├── Transform engine
   ├── Network routing
   ├── SQLite persistence
   └── Tantivy search

WebGL2 ── graph rendering / layout
Web Worker ── community detection
```

Main technologies:

- **Rust** — application core
- **Tokio** — asynchronous execution
- **petgraph** — graph structures
- **SQLite** — durable local storage
- **Tantivy** — local search index
- **Tauri 2** — desktop shell
- **WebGL2** — graph rendering and GPU-assisted layout

More detail: [docs/architecture.md](docs/architecture.md).

## Project status

LinkScope is usable as an engineering prototype, but it should not yet be treated as a hardened forensic platform.

Current development areas include performance validation on large graphs, network/privacy testing, packaging improvements, authentication-backed data sources and broader UI testing.

Verification and acceptance documentation:

- [Architecture](docs/architecture.md)
- [Source matrix](docs/sources.md)
- [Acceptance tests](docs/acceptance.md)
- [Verification status](docs/verification.md)

## License

LinkScope is released under the **MIT License**.
