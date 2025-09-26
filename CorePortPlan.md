# Iterative Plan: Porting QGroundControl Non-UI Logic to a C Library

_Date: September 26, 2025_

Goal: factor QGC’s mission-critical backend (telemetry, mission management, storage, device I/O, analytics) into a standalone C library (`libqgc_core`) that can be consumed by the existing Qt client and future UIs (Electron, native mobile, etc.). The plan emphasizes incremental delivery so the current product remains usable throughout.

## Phase 0 – Foundations (0–2 weeks)

- **Inventory subsystems**: Catalogue modules under `src/` and classify as core vs. presentation. Capture dependencies, side effects, and current Qt usage.
- **Define success metrics**: Decide what functionality must be preserved in each milestone (e.g., telemetry ingest, mission planning, log replay). Establish KPIs (latency budget, CPU usage) and regression tests to guard them.
- **Testing baseline**: Stabilize existing unit/integration tests; add harnesses that can run without the GUI (e.g., headless MAVLink replay, mission plan load/save).

## Phase 1 – Boundary Definition (2–4 weeks)

- **API contracts**: Draft C-friendly interfaces for core services (e.g., `qgc_core_vehicle_manager`, `qgc_core_mission_api`). Prefer opaque handles and callbacks. Document threading expectations.
- **Dependency audit**: Identify Qt types in core code. Begin replacing `QString`, `QVariant`, etc. with STL equivalents or thin adapters to ease future detachment.
- **Service isolation prototypes**: Wrap a small subsystem (e.g., parameter cache) behind the proposed API and integrate it back into the Qt app via the new interface to validate the approach.

## Phase 2 – Core Library Skeleton (4–8 weeks)

- **Create `libqgc_core` project**: Standalone CMake target producing a static/shared library plus exported headers. Set up CI to build and run tests for the library independently.
- **Logging + utilities**: Port cross-cutting utilities (logging, math helpers, MAVLink serialization) into the library, ensuring no Qt GUI dependencies remain.
- **C ABI surface**: Implement foundational API entry points (initialization, event loop hooks, shutdown) with explicit ownership semantics.

## Phase 3 – Subsystem Extraction (8–20 weeks, iterative)

Repeat the following loop for each subsystem, prioritizing lowest coupling first:

1. **Select subsystem**: e.g., telemetry pipeline, mission planner, terrain engine, log replay, joystick manager.
2. **Refactor for purity**: Eliminate UI calls, inject dependencies (timers, file I/O) via interfaces so the subsystem compiles without Qt GUI.
3. **Move into library**: Relocate code into `libqgc_core`, expose via C headers, update build to remove old location.
4. **Adapt Qt client**: Replace direct usage with calls through the C API. Maintain feature parity with minimal UI changes.
5. **Testing**: Add unit tests inside the library and integration tests in the Qt client exercising the new boundary. Run performance checks.
6. **Documentation**: Update API reference and migration notes for downstream consumers.

Aim to deliver one subsystem every 2–3 weeks to keep risk low and feedback fast.

## Phase 4 – Event & Data Transport (parallel with Phase 3)

- **Messaging model**: Standardize how events cross the UI boundary (e.g., observer callbacks, message queues). Provide default adapters for Qt signals/slots and alternative transports (gRPC, WebSockets) for future UIs.
- **Serialization**: Define neutral payload formats (protobuf, flatbuffers, or lightweight structs) for mission plans, telemetry, and UI commands.
- **Threading policy**: Ensure the library can run in its own thread or process. Provide reentrant APIs where necessary.

## Phase 5 – Tooling & Packaging (18–24 weeks)

- **SDK deliverables**: Publish headers, compiled binaries, and documentation. Include samples (C++, Rust, Python) consuming the new C API.
- **CI/CD**: Produce versioned releases of `libqgc_core`, run ABI compatibility checks, and integrate static analysis/sanitizers.
- **Deprecation plan**: Identify remaining Qt-specific shims inside the core and schedule their removal or encapsulation.

## Phase 6 – Readiness for New UI (24+ weeks)

- **Stability checkpoint**: Confirm the Qt client operates solely through the C library. Collect telemetry to prove performance/latency goals are met.
- **Greenfield enablement**: Provide reference bindings (e.g., Rust via `cxx`, Node.js via `napi-rs`, TypeScript via gRPC) to jump-start alternate UIs.
- **Documentation & governance**: Establish API versioning, change control, and contribution guidelines so future teams can iterate on `libqgc_core` without breaking consumers.

## Risk Mitigation

- Maintain dual-path operation (legacy direct calls + new C API) during early extractions to allow quick rollback.
- Invest in automated tests and flight-log replays that exercise the core without UI to detect regressions early.
- Keep scope slices small; avoid moving multiple subsystems simultaneously.
- Budget time for performance tuning after each extraction to counteract any overhead introduced by the new abstraction.

## Deliverables Checklist

- [ ] Subsystem inventory & dependency matrix
- [ ] C API specification and style guide
- [ ] `libqgc_core` build target with CI pipeline
- [ ] Unit/integration test suites covering migrated subsystems
- [ ] Migration playbook for Qt client developers
- [ ] SDK package (headers, binaries, sample bindings)
- [ ] Documentation of threading, memory, and error-handling contracts

Following this plan keeps the current product stable while steadily carving out a reusable core. Once complete, any new UI (Qt, Electron, native mobile) can consume `libqgc_core` as its backend.

