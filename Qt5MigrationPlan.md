# Qt5 Migration Plan

_Date: September 26, 2025_

## Executive Summary

QGroundControl 6.x currently targets **Qt 6.8.3** and makes extensive use of Qt 6-only build tooling (`qt_add_executable`, `qt_add_qml_module`, `qt_policy`), QML registration macros from **QtQmlIntegration**, newly consolidated modules (e.g., `Qt::Core5Compat`, `Qt::Multimedia` with FFmpeg backend), and Qt Quick 3D. Migrating “backwards” to Qt 5 (most likely Qt 5.15.16 LTS) is **non-trivial** and would amount to a major fork of the codebase. Feature parity may not be achievable without removing or re-implementing multiple subsystems (notably the 3D viewer and modern video pipeline). Expect several months of effort plus long-term maintenance burden for security patches that Qt 5 no longer receives.

## Compatibility Snapshot

| Area | Qt 6 Usage Today | Qt 5 Status | Migration Impact |
|------|------------------|-------------|------------------|
| **Build tooling** | `qt-cmake` + `qt_standard_project_setup`, `qt_add_executable`, `qt_policy` | Qt 5 uses legacy `Qt5::` targets, `qt5_wrap_cpp`, older CMake macros | Rewrite top-level CMake configuration, adjust generator expressions, remove Qt 6 policies |
| **Module availability** | `Quick3D`, `QmlIntegration`, `Core5Compat`, `TextToSpeech`, `MultimediaQuickPrivate`, `QuickControls2` | `QtQuick3D` only tech-preview in 5.15, `QtQmlIntegration` absent, Quick Controls 2 available, Multimedia differs | Must replace 3D view (Qt 3D or disable), substitute QML registration macros, audit feature gaps |
| **QML registration** | `QML_ELEMENT`, `QML_NAMED_ELEMENT`, `QML_SINGLETON` via `<QtQmlIntegration>` | Qt 5 relies on `qmlRegisterType`, `qmlRegisterSingletonType`, or `QQmlEngineExtensionPlugin` | Bulk code changes across controllers, settings objects, plugins |
| **Multimedia/Video** | FFmpeg-based Qt Multimedia, GStreamer 1.24 integration, Qt6 shader tooling | Qt 5 Multimedia uses GStreamer 1.18 backend, limited hardware acceleration, no `QtShaderTools` | Rework video pipeline, rebuild custom GStreamer sink against Qt 5 APIs, adjust shader build system |
| **Rendering** | Modern QRhi, `QQuick3D` scenegraph | Qt 5 Quick scene graph + optional Qt 3D module | For 3D features, either regress to Qt 3D or drop functionality |
| **Platform support** | macOS 12+, Windows 10+, Ubuntu 22+ | Qt 5.15 still runs on older platforms but lacks Apple Silicon native support (needs Rosetta) | Lose native arm64 performance on macOS unless maintaining dual binary builds |
| **Tooling ecosystem** | `qt-cmake` and Qt 6 online installer deliver consistent kits | Qt 5 installer retired; archives available, but security updates limited | Increased maintenance overhead, potential tooling rot |

## Feasibility Considerations

1. **Long-term support**: Qt 5.15 receives only commercial security fixes; OSS branches are community-maintained. Depending on licensing, staying on Qt 5 may introduce unaddressed CVEs.
2. **Apple Silicon**: Qt 5 lacks official arm64 macOS binaries; cross-compilation or Rosetta may be required, impacting performance/testing.
3. **Dependency ecosystem**: Third-party modules (GStreamer plugins, `libevents`, SDL3, etc.) expect Qt 6 headers. Downgrading may force forks or patches.
4. **CI/toolchain**: All build scripts, Docker images, and developer environments are tuned for Qt 6. Recreating Qt 5-based images requires significant rework.

## Migration Strategy (If Pursued)

### Phase 0 – Scope Validation (2–3 weeks)
- Audit all Qt 6 API usages (scripts: `clang-tidy`, `rg "Qt6"`, search for `QML_ELEMENT`, `QML_SINGLETON`, `qt6_add_*`).
- Prototype a minimal build with Qt 5.15 to surface blocking linker/compiler errors. Expect immediate failures in build system macros and QML registration.
- Decide which features may be dropped or replaced (e.g., 3D viewer, advanced video overlays).

### Phase 1 – Build System Backport (3–4 weeks)
- Replace `qt_add_executable`, `qt_add_qml_module`, `qt_add_resources` with Qt 5 equivalents (`add_executable`, `qt5_add_resources`, `qt5_add_qml_module` is unavailable; use manual qrc + qml import paths).
- Remove Qt 6-specific CMake policies (`qt_policy` calls, CMP0168 handling tuned for Qt 6 toolchain).
- Update `find_package(Qt6 ...)` to `find_package(Qt5 COMPONENTS ...)` and verify component availability. Provide fallback shims for modules missing in Qt 5.

### Phase 2 – API Translation Layer (4–6 weeks)
- Replace `QML_ELEMENT` / `QML_SINGLETON` macros with explicit `qmlRegisterType`, `qmlRegisterSingletonType`, or plugin metadata using `qt5_add_resources` and `QQmlExtensionPlugin`.
- Introduce a compatibility header to emulate Qt 6 conveniences (e.g., alias `QStringView`, `std::string_view`, QDateTime zones) using `Qt5::Core5Compat` equivalence or custom wrappers.
- Audit `std::u8string`/`QStringView` conversions, adjust for Qt 5’s API surface.

### Phase 3 – Feature Re-implementation (6–12+ weeks)
- **3D Viewer**: Either port to Qt 3D (rewrite materials/shaders, restructure scene graph) or disable the feature behind a compile flag.
- **Video Pipeline**: Rebuild the custom GStreamer sink using Qt 5 scene graph interfaces; adjust shader compilation workflow (Qt 5 lacks `qt_add_shaders`, so precompile GLSL or embed raw shaders).
- **Multimedia API**: Replace Qt 6 Multimedia APIs (camera, audio output) with Qt 5 equivalents or third-party libs.
- **QML Imports**: Ensure QML files reference Qt 5 modules (e.g., `import QtQuick 2.x`, `QtQuick.Controls 2.x`), adjust for API differences and deprecated properties.

### Phase 4 – Packaging & Platform Validation (4–6 weeks)
- Update macOS/Windows packaging scripts to use Qt 5 deploy tools (`macdeployqt`, `windeployqt`), ensuring notarization and codesigning still succeed.
- Re-test hardware integrations (serial, USB, joystick). Qt 5 plug-in loading paths differ.
- Regression-test mission planning, telemetry, video playback across all OS targets.

### Phase 5 – Stabilization & Support (ongoing)
- Establish a forked maintenance branch with security patch plan (Qt 5 no longer upstream-maintained).
- Document developer environment setup (archived Qt 5 installers, license considerations).
- Monitor upstream dependencies for continued Qt 5 compatibility; expect increasing divergence over time.

## Risk Assessment

| Risk | Impact | Mitigation |
|------|--------|------------|
| Missing Qt 6 APIs (Qt Quick 3D, ShaderTools) | Loss of key functionality (3D viewer, shader pipeline) | Accept feature regression or rewrite using Qt 3D / custom OpenGL |
| Apple Silicon performance degradation | Native macOS build quality drops | Ship x86_64-only builds, instruct users to use Rosetta, or invest in custom Qt 5 arm64 build (high cost) |
| Security patches unavailable | Potential exposure to CVEs | Commercial Qt 5 license (if permissible) or maintain internal backports |
| Developer tooling drift | Higher onboarding friction | Provide prebuilt Docker images/VMs with Qt 5 toolchain |
| Timeline overrun | Delayed releases | Prioritize essential features, drop optional modules early |

## Alternative Recommendations

- **Maintain Qt 6**: Adjust application architecture (e.g., separate core library, new UI) without regressing the framework version.
- **Dual-track builds**: If Qt 5 compatibility is required for specific platforms, factor out a limited feature build using legacy Qt 5 while keeping mainline on Qt 6. This adds ongoing maintenance overhead but isolates risk.
- **Explore lighter-weight UI ports (Electron/TypeScript or native)** consuming a shared backend instead of investing in a Qt downgrade.

## Estimated Effort

- Minimum viable backport (core features only, 3D/video disabled): **3–4 developer months**.
- Full parity attempt: **6–9 developer months**, plus continuous maintenance cost.

Given the scale of changes, reverting to Qt 5 should only be considered if driven by strong platform or licensing constraints. Otherwise, focusing on forward-compatible architecture (e.g., extracting a reusable backend) is likely to deliver better long-term value.

