# Qt Reduction Plan

_Date: September 26, 2025_

The goal of this document is to highlight where Qt is used today, score each major module by how tightly it depends on Qt-specific APIs, and point out low-effort steps to peel those dependencies away without breaking the app. Scores focus on **unnecessary** Qt usage—that is, places where the same job could be handled by portable C++ tooling or a thin shim while keeping the current Qt-based UI alive during the transition.

## Scoring rubric
- **1 — Qt-lite:** Module is already close to plain C++; Qt is limited to logging/containers and can be swapped quickly.
- **2 — Light touch:** Primarily non-UI logic that leans on Qt convenience types (QString, QTimer, QIODevice). Replacing these needs wrappers but is localized.
- **3 — Moderate:** Business logic expressed through QObject, signals/slots or Qt network/location helpers. Feasible to extract by introducing facades, but requires refactoring effort.
- **4 — Heavy:** Core behaviors exposed directly to QML or dependent on Qt meta-object features (Q_PROPERTY, QVariant plumbing). Separation demands substantial redesign.
- **5 — Qt-bound:** UI/QML layers or subsystems whose raison d’être is Qt Quick. Porting means rebuilding the feature in another UI stack.

## Module snapshot
| Module | Score | Qt usage highlights | Immediate Qt-reduction ideas |
| --- | --- | --- | --- |
| Core application (`main.cc`, `QGCApplication`)|5|`QGuiApplication`, global singletons, startup wiring, QQmlApplicationEngine|Document responsibilities before introducing platform-neutral bootstrap; long-term plan only|
| ADSB |3|ADSB TCP link uses `QTcpSocket`, models expose `QObject`/`Q_PROPERTY` to QML|Split parser/state into plain C++ structs; wrap networking with POSIX/Asio adapter; leave Qt-facing view model|
| AnalyzeView |4|Controllers are QObject-derived, exposing telemetry/log parsing into QML, heavy QVariant|Carve out MAVLink log parsing into pure C++ services; feed Qt layer via adapters|
| Android |4|Qt Android extras and custom serial port plugin wrappers|Treat as platform shim; replacing requires platform-native glue late in migration|
| API |4|Plugin/options API built on QObject, QQmlEngine registrations|Define abstract interfaces in C++; keep Qt plugin implementation as one adapter|
| AutoPilotPlugins |4|Large QObject hierarchy with Facts and QML bindings|Factor firmware-specific logic into plain C++ strategy classes; Qt layer keeps Q_PROPERTY proxies|
| Camera |4|Camera manager & controls rely on QObject, QML exposure, QTimer|Extract MAVLink camera protocol handling into service, keep Qt for device discovery/UI|
| Comms |2|Serial/TCP/UDP links use `QSerialPort`, `QTcpSocket`, `QTimer`; no UI|Introduce transport facade with C++20 `std::chrono`/Asio backends; Qt class becomes shim|
| FactSystem |4|Facts, metadata, QML bindings depend on Qt meta-object & QVariant|Investigate replacement schema using plain structs + reflection; requires wide redesign|
| FirmwarePlugin |3|Plugins are QObject-based but mostly logic; relies on Qt containers|Wrap plugin API in header-only pure C++ interface; Qt implementation registers via adapter|
| FlightDisplay |5|Entirely QML/Qt Quick gauges and overlays|Only replace when a new UI stack exists|
| FlightMap |5|Qt Location/QML map items|Same as above; dependent on Qt Quick|
| FollowMe |3|Uses `QObject`, `QTimer`, Qt positioning|Move motion-report computation to pure C++; inject platform GPS provider abstraction|
| Gimbal |3|Small QObject wrappers with timers and MAVLink messaging|Refactor command math into plain helper; keep Qt only for signal wiring|
| GPS |3|QThread-based GNSS reader with `QSerialPort`|Replace with std::thread + cross-platform serial lib; expose observer interface|
| Joystick |3|Relies on Qt input APIs and QObject|Assess SDL/libhid replacement; start with abstraction over event callbacks|
| MAVLink |1|Mostly protocol helpers using QString/QLoggingCategory|Normalize on std::string/spdlog; zero-impact rewrite|
| MissionManager |4|Mission items and controllers are QObject-heavy, Fact-driven|Extract mission serialization/planning core to C++; Qt layer wraps for QML|
| PositionManager |3|Qt Positioning for GPS, timers, signals|Define platform-agnostic sensor API; Qt implementation becomes one provider|
| QmlControls |5|QML-only UI components|Qt-exclusive; defer until alternate UI|
| QtLocationPlugin |4|Custom Qt location plugin & QGeo types|Isolate mapping providers behind neutral map service interfaces; high-effort|
| RunGuard |2|Uses `QLockFile`, QString helpers|Switch to std::filesystem + platform file locks; retain Qt-friendly wrapper|
| Settings |4|Fact-backed settings objects with QML bindings|Need new config schema (JSON/YAML + std types) plus migration tooling|
| Terrain |3|`QObject` + `QNetworkAccessManager` for tile caching|Lift cache/indexing into pure C++ service; keep Qt network adapter temporarily|
| UI |5|Top-level QML scenes/layouts|Depends on Qt Quick; migrate only with new UI|
| UTMSP |4|Mix of QML and QObject services|Extract REST clients & state machines into pure C++; leave QML visuals|
| Utilities |2|Helper libs (compression, JSON, logging) mostly use QtCore types|Gradually rehome onto std::filesystem, nlohmann/json, zlib wrappers|
| Vehicle |4|Large QObject graph, Q_PROPERTY, Fact integration|Refactor core state machine/telemetry cache into headless controller; Qt acts as adapter|
| VideoManager |4|Qt Multimedia & GStreamer glue with QObject|Abstract capture/decoding pipeline using libgstreamer C++ wrappers; Qt only for surface binding|
| Viewer3D |4|Qt Quick 3D + QObject scene graph helpers|Requires alternative 3D stack; consider decoupling terrain/tile loaders first|

## Wave 1 – Qt-lite clusters (scores 1–2)
- **MAVLink**: Replace QString/logging with `std::string` and fmt/spdlog. Expose clean packet + CRC utilities usable from any UI.
- **Comms**: Design a transport interface (`ILink`, `ILinkConfiguration`). Provide an Asio backend and keep Qt-based classes as thin adapters that forward signals until UI swap.
- **RunGuard**: Reimplement lock-file logic with `std::filesystem` and POSIX/Win32 APIs. Maintain a Qt wrapper that defers to the new core for compatibility.
- **Utilities**: Migrate each helper to portable libraries (e.g., zlib/minizip, nlohmann/json). Ensure new APIs surface standard types so future modules are not forced to pull in QtCore.

Target outcome: A reusable **headless utility library** that current Qt UI links against but future non-Qt front-ends can also consume.

## Wave 2 – Moderately entangled modules (score 3)
Focus on introducing **interface layers** so the pure C++ core owns the logic while Qt acts as a presentation layer.
- **FirmwarePlugin, Terrain, FollowMe, GPS, Gimbal, Joystick, PositionManager**: Define service interfaces (e.g., `ITerrainProvider`, `IVehicleFirmwareProfile`) and move timers/threads to `std::chrono`/`std::thread`. Provide dependency injection so tests and future front-ends can reuse them.
- **Camera, UTMSP REST clients, VideoManager ingestion, Comms AirLink**: Factor protocol handling into standalone components, leaving Qt to publish telemetry via signals only.
- **Mission serialization helpers** inside MissionManager: extract planners to plain C++ while keeping QML command editors intact short-term.

Deliverables for this wave include interface definitions, adapter implementations, and unit tests that run without Qt.

## Wave 3 – Qt-heavy layers (scores 4–5)
These subsystems are either QML-based UI or depend on Qt meta-object machinery. Approach them after Waves 1–2 establish a reusable core.
- **UI/FlightDisplay/FlightMap/QmlControls/Viewer3D**: Begin by documenting UI contracts (data models, commands) provided by the C++ layer. Parallel-prototype alternative UI (e.g., Electron + Rust backend) against the new service interfaces.
- **FactSystem, Settings, AutoPilotPlugins, Vehicle, MissionManager UI controllers, AnalyzeView**: Refactor gradually by replacing Qt `Fact` usage with a schema-driven data model (e.g., generated structs + reflection metadata). Maintain Qt shims that translate to QML until the UI migrates.
- **Android platform glue**: Once core services expose platform-neutral APIs, re-implement Android-specific pieces against native NDK/Jetpack code if Qt is retired on that platform.

Expect multi-quarter efforts here; success requires the foundations laid in earlier waves.

## Cross-cutting recommendations
- Establish a **core library** (`libqgc_core`) containing the Wave 1 and select Wave 2 modules compiled without Qt. Enforce this via CMake targets that forbid Qt usage.
- Introduce a **telemetry/event bus** abstraction (e.g., templated dispatcher) so Qt signals become just one subscriber. Other front-ends can subscribe through std::function callbacks.
- Adopt **code generation or reflection** for parameter metadata to replace QML `Fact` plumbing. Evaluate existing JSON schemas to auto-generate both C++ structs and Qt adapters.
- Incrementally add **unit/integration tests** that run under plain CTest to ensure the non-Qt core stays portable.
- Track progress with a dependency matrix: each module should declare whether it links to Qt. Use CI to flag regressions when introducing new Qt usage into the core library.

With these steps, we can make tangible progress toward a Qt-independent core while keeping the current application functional throughout the transition.
