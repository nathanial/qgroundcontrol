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
        ├── error.rs      # Structured error mapping to napi::Error
        └── lib.rs        # Async diagnostics + status exports consumed by TS
```

### Electron shell
- Compiles TypeScript into `dist/`.
- Loads the Rust native module (`rust-core/index.node`) on startup, wires IPC handlers for async diagnostics, and replays bootstrap data to the renderer.
- Renderer exposes buttons that exercise the napi surface and renders the vehicle snapshot supplied by the Rust core.

### Rust core
- Built as a `cdylib` using napi-rs. Functions annotated with `#[napi]` are callable from Node/Electron as synchronous or async commands.
- Exposes `health_check()`, `run_diagnostics()`, `bootstrap_vehicle_status()`, `simulate_failure()`, and `version()` with structured error propagation.
- `domain-model/` crate centralises MAVLink-agnostic data types (vehicle IDs, arming state, etc.) so both the napi layer and future Rust services share one definition set.
- Future crates will house MAVLink transport, mission planners, parameter stores, etc. Exported APIs should stay small and typed to keep the boundary stable.

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

1. Design the native API surface (e.g., async telemetry streams, command dispatch) and expose them via napi-rs.
2. Add CI jobs for `napi build` on each platform to guarantee the Node module stays portable.
3. Expand the renderer into mission management, telemetry panels, and maps using the new bindings.

This scaffold gives us a compact playground for migrating features from the legacy Qt codebase to a modern Electron + Rust stack.
