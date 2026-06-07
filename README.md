# FlowHub

FlowHub is a unified desktop platform for local file transfer, folder sync, network downloads, and controlled sharing.

The long-term goal is to provide one cross-platform app for:

- Discovering devices on the local network.
- Sending files peer-to-peer.
- Managing HTTP, HTTPS, FTP, Magnet, and future BitTorrent downloads.
- Syncing folders across trusted devices.
- Sharing files with explicit permissions.

FlowHub v0.1 focuses on the Send + Download MVP while keeping the architecture ready for Sync and Share.

## Current Status

This repository currently contains the v0.1 project foundation:

- Rust workspace with `core`, `discovery`, `send`, `download`, and `storage` crates.
- Tauri v2 desktop backend.
- React + Vite frontend with Dashboard, Send, Download, and Settings views.
- UDP LAN discovery service that starts with the desktop backend.
- TCP file transfer primitives with pause/resume control and SHA-256 verification.
- aria2 JSON-RPC client for adding and managing download tasks.
- SQLite metadata storage for files and tasks.
- Unit tests in the Rust crates where practical.

v0.1 is not fully product-complete yet. The remaining v0.1 work is listed in [v0.1 Completion Check](#v01-completion-check).

## Architecture

```text
UI <-> Core <-> [discovery, send, download, storage]
```

Workspace layout:

```text
.
├── app/
│   ├── src/                 # React UI
│   └── src-tauri/           # Tauri desktop backend
├── crates/
│   ├── core/                # FlowHub orchestration
│   ├── discovery/           # LAN device discovery
│   ├── send/                # P2P file transfer
│   ├── download/            # aria2 RPC client
│   └── storage/             # SQLite metadata
├── Cargo.toml
├── package.json
└── README.md
```

## v0.1 Scope

Target MVP features:

- Launch desktop app and show the main window.
- Sidebar navigation: Dashboard, Send, Download, Settings.
- Send module:
  - Discover LAN devices.
  - Drag files into the UI.
  - Send files to a selected peer.
  - Show progress, speed, and ETA.
- Download module:
  - Use aria2 RPC.
  - Support HTTP, HTTPS, FTP, and Magnet links.
  - Add, pause, resume, remove, and inspect tasks.
- Storage module:
  - Store local file metadata.
  - Store task metadata in SQLite.

## v0.1 Completion Check

| Area | Status | Notes |
| --- | --- | --- |
| Desktop shell | Done | Tauri + React structure is present. |
| Sidebar navigation | Done | Dashboard / Send / Download / Settings are implemented. |
| Rust workspace | Done | `core`, `discovery`, `send`, `download`, `storage` crates exist. |
| Discovery crate | Mostly done | UDP broadcast/receive, peer cache, `list_peers()`, and tests are present. |
| Discovery backend integration | Done | Tauri starts the discovery service on setup. |
| Send crate | Partially done | TCP transfer, pause/resume control, resume metadata, SHA-256 verification, and tests exist. |
| Send UI integration | Not complete | Drag/drop is UI-only; it does not yet call Tauri send commands or select a target peer. |
| Transfer progress UI | Not complete | Placeholder exists; live progress, speed, and ETA are not wired yet. |
| Download crate | Mostly done | aria2 JSON-RPC APIs exist: `add_url`, `pause`, `resume`, `remove`, `status`. |
| Download UI integration | Done | Add, pause, resume, and remove are wired through Tauri commands. |
| Download status updates | Mostly done | UI polls backend status and displays progress, speed, and ETA from aria2 when available. |
| Storage crate | Done | SQLite migration, file metadata, task metadata, and tests are present. |
| Rust build/test verification | Done | `cargo fmt --all -- --check`, `cargo check --workspace`, and `cargo test --workspace` pass. |
| Frontend type verification | Done | `tsc -p app/tsconfig.json --noEmit` passes. |

## Roadmap

### v0.1 — MVP: Send + Download

Implement the desktop shell, basic LAN discovery, send primitives, aria2 download primitives, and SQLite storage.

### v0.2 — Device Discovery

Make discovery production-ready:

- Stable device identity.
- Peer TTL and offline state.
- Better local IP detection.
- UI refresh and diagnostics.

### v0.3 — Send Module

Complete peer-to-peer transfer:

- Peer selection.
- Tauri send/receive commands.
- Transfer sessions.
- Resume negotiation.
- Progress events.

### v0.4 — UI Integration

Wire Send + Discovery into the desktop UI:

- Live peer list.
- Drag file to selected device.
- Progress, speed, and ETA.
- Error and retry states.

### v0.5 — Download Module

Harden aria2 integration:

- Add, pause, resume, remove, and status commands.
- HTTP, HTTPS, FTP, SFTP, Magnet, RSS support where aria2 supports it.
- Task persistence and recovery.

### v0.6 — Download UI

Complete download management UI:

- Task list.
- Progress bars.
- Speed and ETA.
- Pause, resume, and delete buttons.
- Live aria2 polling.

### v0.7 — Sync Module

Add folder sync:

- File watching.
- Incremental sync.
- Conflict strategies.
- Version history.

### v0.8 — Share Module

Add permission-based sharing:

- Share links.
- Read/write/download permissions.
- Share history.
- Revoke support.

### v0.9 — Enhanced P2P & BT

Improve large-scale transfer:

- BitTorrent support.
- DHT and PEX.
- Bandwidth limits.
- Multi-task scheduling.

### v1.0 — Stable Release

Ship the complete product:

- Send, Sync, Download, and Share modules.
- Stable macOS, Windows, and Linux desktop builds.
- Unified UI.
- Logs, errors, and user feedback.
- Documentation and test coverage target above 80%.

## Development

Install prerequisites:

- Rust toolchain with `cargo`.
- Node.js with `npm`.
- aria2 for network downloads.
- Platform-specific Tauri dependencies.

Install dependencies:

```sh
npm install
```

Run the desktop app:

```sh
npm run tauri:dev
```

Run the frontend only:

```sh
npm run dev
```

Run Rust tests:

```sh
cargo test --workspace
```

Run aria2 with RPC enabled:

```sh
aria2c --enable-rpc --rpc-listen-all=false --rpc-listen-port=6800
```

## Useful Codex Prompts

### Complete v0.1 Send UI Integration

```text
Complete FlowHub v0.1 Send UI integration.
- Add Tauri commands for selecting a peer and sending dropped files.
- Emit transfer progress events from Rust to React.
- Show progress, speed, ETA, pause, resume, and completion status.
- Keep existing crate boundaries: UI <-> Core <-> send/discovery/storage.
- Add focused tests where possible.
```

### Complete v0.1 Download UI Integration

```text
Complete FlowHub v0.1 Download UI integration.
- Wire pause, resume, remove, and status to Tauri commands.
- Poll aria2 status and update task progress, speed, ETA, and state.
- Persist task metadata in storage.
- Keep the existing React layout and Rust crate boundaries.
- Add tests for download task state handling.
```

### Implement v0.2 Discovery Hardening

```text
Improve FlowHub discovery crate for v0.2.
- Persist stable device_id.
- Include hostname, IP, version, and last_seen.
- Mark peers offline after TTL instead of immediately removing them.
- Add tests for TTL, duplicate peers, self-filtering, and peer updates.
- Expose the data through Tauri and refresh the UI.
```

## Notes

- The current database path is `flowhub.db` from the app working directory.
- The default aria2 RPC endpoint is `http://127.0.0.1:6800/jsonrpc`.
- The default discovery UDP port is `47321`.
- The repository directory is not initialized as a git repository yet.
