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
    ├── build.rs          # Hooks napi-build into cargo
    └── src/
        └── lib.rs        # Exports health_check/version bindings today
```

### Electron shell
- Compiles TypeScript into `dist/`.
- Loads the Rust native module (`rust-core/index.node`) on startup and broadcasts its status messages to the renderer via IPC.
- Renderer shows the latest Rust messages, proving the bridge end to end.

### Rust core
- Built as a `cdylib` using napi-rs. Functions annotated with `#[napi]` are callable from Node/Electron.
- Currently exposes `health_check()` and `version()`—both return structured payloads used by the renderer.
- Future crates will house MAVLink transport, mission planners, parameter stores, etc. Exported APIs should stay small and typed to keep the boundary stable.

## Prerequisites
- Node.js ≥ 18 (for Electron 30 and napi builds).
- Rust toolchain (stable) with the `rustup` default target for your OS.

## Getting started

```bash
cd greenfield
npm install             # Installs Electron, TypeScript, @napi-rs/cli
npm run build           # Builds the napi module and TypeScript bundles
npm run dev             # Rebuilds and launches Electron
```

During development you can rebuild the native module alone:

```bash
npm run build:native    # cd rust-core && napi build --platform --release
```

The resulting `rust-core/index.node` is ignored by git but consumed by Electron at runtime.

## Next steps

1. Design the native API surface (e.g., async telemetry streams, command dispatch) and expose them via napi-rs.
2. Add CI jobs for `napi build` on each platform to guarantee the Node module stays portable.
3. Expand the renderer into mission management, telemetry panels, and maps using the new bindings.

This scaffold gives us a compact playground for migrating features from the legacy Qt codebase to a modern Electron + Rust stack.
