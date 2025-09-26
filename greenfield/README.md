# QGroundControl Greenfield Scaffold

Date: September 26, 2025

This folder hosts a fresh Electron + Rust workspace where we can incrementally re-implement QGroundControl without Qt. The UI remains TypeScript-driven, while telemetry, mission logic, and hardware integrations will migrate into a reusable Rust core. The bridge between both worlds is powered by [`napi-rs`](https://github.com/napi-rs/napi-rs), so Rust exports a Node module that the Electron main process can load directly.

## Layout

```
greenfield/
├── package.json          # Electron + TypeScript tooling + napi build scripts
├── tsconfig*.json        # Split TypeScript configs (Node vs. browser targets)
├── src/
│   ├── main.ts           # Electron main process (loads Rust napi module)
│   ├── preload.ts        # Context isolation bridge exposing core events
│   └── renderer/         # Browser-side TypeScript + static assets
└── rust-core/
    ├── Cargo.toml        # Rust crate compiled to index.node via napi-rs
    ├── domain-model/     # Shared Rust data structures re-exported over napi
    ├── build.rs          # Hooks napi-build into cargo
    └── src/
        ├── discovery.rs  # Serial/UDP device polling with event emission
        ├── events.rs     # ThreadsafeFunction event bus bridging Rust → JS
        ├── mavlink.rs    # Async MAVLink transport + parameter handling
        ├── error.rs      # Structured error mapping to napi::Error
        └── lib.rs        # Public napi surface + conversions to domain types
```

### Electron shell
- Compiles TypeScript into `dist/`.
- Loads the Rust native module (`rust-core/index.node`) on startup, wires IPC handlers for diagnostics, discovery, connectivity, and parameter requests, and replays bootstrap data to the renderer.
- Renderer now renders:
  - A live device table fed by the Rust discovery loop.
  - Connection status + heartbeat telemetry coming from the MAVLink service.
  - Parameter fetch progress/batches and mission log events emitted by the native core.

### Rust core
- Built as a `cdylib` using napi-rs. `#[napi]` functions expose synchronous commands (health/version) and async flows (device watch, connect/disconnect, parameter fetch).
- `discovery.rs` polls serial ports + UDP defaults, deduplicates devices, and emits `CoreEvent`s into a threadsafe function sink.
- `mavlink.rs` manages simulated, UDP, and serial links (via `mavlink` crate), forwards heartbeat/statustext messages, and orchestrates parameter downloads with timeout/stream progress.
- `domain-model/` centralises MAVLink-agnostic data types so both the napi layer and future services share one schema.

## Phase 1 capabilities

- **Device discovery**: `startDeviceWatch` spawns a background task that surfaces serial devices plus default UDP endpoints; snapshots and deltas are streamed to Electron.
- **Core event bus**: `registerEventSink` wires a napi `ThreadsafeFunction`, allowing Rust to push `device_*`, `connection_status`, `heartbeat`, `mission_log`, `parameter_*`, and diagnostic events directly to the renderer.
- **MAVLink connectivity**: `connectMavlink` supports `Simulated`, UDP (`udpin:*`), and serial endpoints. Heartbeats update shared `VehicleStatus`, while `STATUSTEXT` feeds the mission log pipeline.
- **Parameter downloads**: `fetchParameters` issues `PARAM_REQUEST_LIST`, tracks progress, caches the resulting list, and emits progress/batch events for the renderer.
- **Electron UI**: new controls allow refreshing links, connecting/disconnecting, and triggering parameter syncs while visualising telemetry/log output.

**Try it locally:**

1. `npm run build` (or `npm run dev` for hot reload) to build the napi module + bundles.
2. Launch the Electron shell; you should see the device table populate (default UDP + any detected serial ports).
3. Click **Connect** on the simulated link (or provide a real endpoint), observe heartbeats + connection status updates, then **Fetch Parameters** to exercise the async pipeline.
4. Logs stream in via the shared event bus; diagnostics still available under **Run Diagnostics** / **Simulate Failure**.
## Prerequisites
- Node.js ≥ 18 (for Electron 30 and napi builds).
- Rust toolchain (stable) with the `rustup` default target for your OS.

## Getting started

```bash
cd greenfield
npm install             # Installs Electron, TypeScript, @napi-rs/cli, Vitest
npm run build           # Builds the napi module + TypeScript bundles (generates rust-core/index.d.ts)
npm run dev             # Rebuilds once then launches Electron for manual testing
```

For iterative Rust edits, keep a watcher running alongside Electron:

```bash
npm run dev:native      # Watches rust-core with napi --watch (install `cargo-watch` first)
```

Key scripts:

- `npm run build:native` – One-off release build of the napi module + `.d.ts` bindings.
- `npm run test` – Executes `cargo test` and the Vitest suite against the compiled napi module.
- `npm run lint` – Type-checks the TypeScript workspace without emitting JS.

## Next steps

1. Extend the MAVLink service with live telemetry streams (attitude, battery, GPS) and expose them as structured events.
2. Layer a TypeScript state store (Zustand/Redux) on the renderer side to normalise event consumption and hydrate dashboards.
3. Implement bidirectional command channels (RC override, mission upload) with optimistic UI + error handling.
4. Add CI coverage for `npm run build`, `npm run test`, and smoke-connect scripts to keep the napi bridge healthy across platforms.

With Phase 1 complete, the greenfield shell now proves end-to-end connectivity and logging without touching the legacy Qt UI, giving us a launch pad for the telemetry/Mission planning work queued in Phases 2 and beyond.

## Phase 3 – Mission Planning & Upload (September 26, 2025)

- Integrated a MapLibre-powered mission canvas that renders live waypoint overlays, updates in real time, and supports click-to-add editing with automatic bounding fits.
- Added a mission inspector alongside the map with altitude editing, quick waypoint removal, and a square-pattern generator for rapid survey prototyping.
- Exposed Rust-side mission upload/download workflows over the napi bridge, maintaining optimistic revision checks and surfacing sync progress/events to the renderer.
- Extended test coverage with a simulated-link round-trip mission suite to ensure the MAVLink mission protocol stays healthy during future refactors.
