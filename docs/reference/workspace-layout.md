# Workspace Layout Reference

Purpose: Explain the current Rsnap workspace layout, which crate owns which behavior today, and
which directories are source versus generated or runtime-local.

Read this when: You are deciding where a change belongs in the current tree, checking whether the
directory structure still matches the implementation, or routing a docs/code question to the right
crate or folder.

Inputs: `Cargo.toml`; `README.md`; `docs/spec/capture-session.md`;
`docs/spec/platform-host-boundary.md`

Depends on: `docs/spec/capture-session.md`; `docs/reference/host-core-reset.md`

Covers: The checked-in workspace layout, crate ownership boundaries today, and the local
directories that should not be treated as repository source.

## Important posture

This document describes the checked-in repository structure today. It is not the architecture
target for the reset lane.

For the active target architecture and migration direction, read:

- `docs/reference/host-core-reset.md`
- `docs/spec/platform-host-boundary.md`
- `docs/decisions/native-host-rust-core-reset.md`

## Current top-level layout

| Path | Role |
| --- | --- |
| `native/macos-host/` | SwiftPM AppKit-first macOS host shell: menu bar entry, full-screen capture windows, in-window HUD/toolbar, and native bridging into `rsnap-host-ffi` |
| `apps/rsnap/` | Thin launcher/bootstrap crate: startup logging, build metadata, stable-bundle resolution, and `cargo run -p rsnap` handoff into the staged native macOS host |
| `packages/rsnap-overlay/` | Transitional Rust runtime and implementation reservoir: legacy overlay runtime, retained scroll-capture logic, frozen edit/export logic, and macOS adapters that have not yet moved into `rsnap-capture-core` |
| `packages/rsnap-capture-core/` | Durable Rust product-semantics and image-algorithm crate: shared geometry, semantic scene model, host/core protocol enums, reset-native session core, export/crop/PNG encoding, capture-frame rendering, wallpaper thumbnail, minimap, mosaic, selection transform, auto-center, and live-sample helpers |
| `packages/rsnap-host-ffi/` | Thin C ABI bridge crate used by the native macOS host to call the Rust product core and retained Rust transition modules |
| `docs/` | Agent-facing repository docs split into `spec`, `runbook`, `reference`, and `decisions` |
| `assets/` | Shared app-icon source plus generated bundle/runtime assets |
| `scripts/` | Packaging helpers plus structured smoke/perf entrypoints under `scripts/smoke/` and `scripts/perf/` |
| `.github/` | CI workflows and repository rules |

This top-level split reflects the codebase as checked in today. It remains useful for navigation,
but it should not be mistaken for the durable host/core target boundary.

## Crate ownership map

### `apps/rsnap/`

Treat `apps/rsnap/` as the launcher/bootstrap layer for the native host.

It owns:

- app-level logging/bootstrap
- build metadata emission
- stable native-bundle lookup from the current worktree
- fallback handoff into `scripts/build_and_run.sh` when the staged native bundle is missing
- explicit unsupported-platform failure for non-macOS builds

Key paths:

- `apps/rsnap/src/lib.rs`: public runtime entry points for the launcher crate
- `apps/rsnap/src/native_launcher_macos.rs`: staged native-bundle lookup and launch handoff
- `apps/rsnap/src/startup.rs`: startup logging/bootstrap helpers
- `apps/rsnap/src/unsupported_platform.rs`: explicit non-macOS error path

### `packages/rsnap-overlay/`

Treat `packages/rsnap-overlay/` as the current transitional container for most capture-session and
legacy overlay behavior.

Today it owns:

- legacy capture-session lifecycle and overlay runtime paths not yet used by the native-host reset
- retained scroll-capture session logic, replay support, and benchmarks
- frozen edit/export logic that is still Rust-owned but has not yet moved into `rsnap-capture-core`
- text annotation rendering helpers reused by Rust export paths
- macOS capture and live-frame adapter code that remains quarantined behind explicit host modules

Important:

- This is the checked-in ownership shape today.
- It is not the long-lived reset target.
- New work should avoid deepening `rsnap-overlay` as the authority for OS-facing window, focus,
  cursor, IME, or capture-capability semantics when that work belongs on the native-host side of
  the reset boundary.

Key paths:

- `packages/rsnap-overlay/src/lib.rs`: explicit `session` façade plus quarantined
  `host_macos` / `host_effects_macos` transition modules
- `packages/rsnap-overlay/src/overlay.rs`: current overlay root plus its focused
  runtime/rendering support modules
- `packages/rsnap-overlay/src/live_frame_stream_macos.rs`: current macOS live-stream support
- `packages/rsnap-overlay/src/scroll_capture.rs`: current scroll-capture session entry with
  focused support modules under `scroll_capture/`
- `packages/rsnap-overlay/src/scroll_capture/worker_pairwise.rs`: ordered worker-pairwise frame
  registration, committed-frontier catchup, rewind/reacquire handling, and growth-block decisions
- `packages/rsnap-overlay/src/scroll_capture/types.rs`: scroll-capture data model, observation
  outcomes, registration candidates, and telemetry structs shared by the session modules
- `packages/rsnap-overlay/src/scroll_capture/fingerprint.rs`: sampled frame fingerprinting used by
  session duplicate detection and structural change tests
- `packages/rsnap-overlay/src/scroll_capture/support.rs`: shared pixel matching,
  static-region rejection, image stacking/resizing, and image-analysis helpers used by scroll
  capture
- `packages/rsnap-overlay/src/scroll_capture/downward_resolution.rs`: downward viewport candidate
  scoring and resolution helpers for session-owned stitching decisions

### `packages/rsnap-capture-core/`

Treat `packages/rsnap-capture-core/` as the new durable product-semantics landing zone.

It owns:

- portable geometry types
- semantic scene snapshots
- explicit host/core protocol enums and structs
- the first reset-native reference session core
- RGBA/BGRA pixel helpers used by host bridge code
- export crop mapping and lossless PNG encoding
- capture-frame layout, background planning, shadowing, wallpaper thumbnail decode/cache, and final
  RGBA composition
- scroll minimap planning
- mosaic patch generation
- frozen selection hit-testing and transform geometry
- auto-center content-bound detection and margin-balance rules

This crate must stay free of:

- top-level window ownership
- `winit` or `egui` runtime authority
- AppKit ownership
- host-side permission or capture capability code

### `packages/rsnap-host-ffi/`

Treat `packages/rsnap-host-ffi/` as the thin ABI companion to `rsnap-capture-core/`.

It owns:

- opaque session handles for foreign hosts
- FFI-safe config, event, report, scene, and request types
- exported `extern "C"` functions that forward into `rsnap-capture-core`
- exported `extern "C"` functions that bridge retained Rust transition modules while they migrate
  toward `rsnap-capture-core`
- the checked-in C header consumed by the native macOS host:
  `packages/rsnap-host-ffi/include/rsnap_host_ffi.h`

It does not own product behavior beyond ABI adaptation.

### `native/macos-host/`

Treat `native/macos-host/` as the new native macOS landing zone for the reset.

It owns:

- the SwiftPM-built `.app` host shell
- the AppKit window/view tree used for live and frozen capture UI
- native cursor, focus, event routing, menu bar entry, and host-side effects
- OS-only resource discovery such as the current wallpaper path
- conversion between AppKit/CoreGraphics image objects and bridgeable RGBA buffers
- presentation of Rust-rendered images and models in native windows
- the checked-in bridge probe used by `cargo make test-host-reset`

The main host-kit files are split by responsibility:

- `NativeHostApp.swift`: app delegate, menu bar lifecycle, settings/hotkey wiring, launch,
  permission, onboarding, update, and status-menu orchestration
- `CaptureSessionController.swift`: shared state and lifecycle hooks for the Swift host-session
  coordinator around `RsnapHostSession`
- `CaptureSessionController+Live.swift`: live capture startup, live sampling warmup, pointer
  movement, and live primary interaction routing
- `CaptureSessionController+FrozenInteraction.swift`: frozen selection movement/resizing,
  annotation commands, auto-center, loupe, and toolbar command forwarding
- `CaptureSessionController+HostRequests.swift`: Rust host-request draining, freeze-snapshot
  commit handling, host-owned frozen scene preparation, and host-effect dispatch
- `CaptureSessionController+ScrollCapture.swift`: native scroll monitor lifecycle, scroll-event
  forwarding, viewport sampling, and scroll minimap preview refresh
- `NativeScrollCaptureObservationPipeline.swift`: conversion of ordered native samples and
  fallback frames into Rust scroll observations plus preview export batches
- `CaptureSessionController+Export.swift`: copy/save host effects, output naming, capture-image
  export, capture-frame effect application, and Rust-backed PNG encoding
- `CaptureSessionController+TextRecognition.swift`: Vision OCR request execution and recognized
  text pasteboard publication
- `CaptureSessionController+Runtime.swift`: shared monitor/window lookup, overlay refresh,
  teardown, status message, and capture-stream release helpers
- `CaptureChrome.swift`: shared native chrome metrics, palette, and drawing geometry
- `CaptureOverlayWindow.swift`: AppKit `NSPanel` wrapper for capture overlay windows
- `CaptureOverlayController.swift`: overlay window set, focus, stream preparation, and below-overlay
  capture source management
- `CaptureHostView.swift`: AppKit view rendering, hit testing, and native pointer/key routing
- `CaptureHostLivePrimaryInteractionState.swift`: capture-host live primary drag, release,
  completion, and hover-suppression state transitions
- `CaptureHostPointerDispatch.swift`: capture-host pointer dispatch events, a shared delivery queue
  with per-track throttling state, and AppKit-to-controller pointer delivery
- `LiveOverlayRenderer.swift`: live overlay layer composition for HUD, loupe, scrims, and selection
  affordances
- `LiveChromePlacement.swift`: live HUD/loupe text metrics, pending color text, and deterministic
  floating placement geometry shared by capture host and live chrome rendering
- `LiveOverlayWindowSnapshotFeed.swift`, `LiveOverlayChromeSampleFeed.swift`, and
  `LiveFrameClockDriver.swift`: live overlay support boundaries for target-window snapshots,
  chrome color/patch sampling, and display-rate frame ticks
- `FrozenToolbarLayoutPlanner.swift`: deterministic frozen-toolbar item availability, layout, and
  hit-test geometry shared by classic drawing, Liquid Glass content, and native probes
- `FrozenToolbarRenderView.swift`: shared frozen-toolbar content drawing for classic AppKit
  toolbar rendering and Liquid Glass toolbar content
- `CaptureGeometry.swift`, `CaptureHostCursorSupport.swift`,
  `CaptureHostLivePrimaryInteractionState.swift`, `CaptureHostPointerDispatch.swift`,
  `CaptureHostFrozenImageEffects.swift`, `LiveChromeRefreshTelemetryKey.swift`, and
  `NativeHostTextMetrics.swift`: focused support boundaries for shared capture geometry,
  capture-host cursor presentation and NSCursor adaptation, live primary interaction state,
  pointer dispatch queue throttling, Rust-backed frozen image effects, live-chrome telemetry
  identity, and shared native text measurement
- `FrozenCaptureModels.swift`: Swift adapter models for Rust-owned frozen overlay editing
- `NativeHostFeedbackSound.swift`: host-side sound lookup/playback for completion effects
- `NativeHostImageBridge.swift`: RGBA/CoreGraphics image conversion used by the FFI bridge
- `NativeHostSettingsView.swift`, `NativeHostSettingsNavigation.swift`,
  `NativeHostSettingsSurface.swift`, `NativeHostAppearanceSettings.swift`, and
  `NativeHostCaptureFrameSettings.swift`: SwiftUI settings shell, navigation, reusable settings
  surfaces, appearance controls, and capture-frame preset controls

It depends on:

- `packages/rsnap-host-ffi/` for the C ABI contract
- `packages/rsnap-capture-core/` indirectly through that ABI

It must not grow a second product-semantic model or duplicate Rust-owned image algorithms. Scene
state, host requests, export bytes, capture-frame renders, minimap plans, selection transforms,
auto-center decisions, and similar deterministic outputs come from the Rust side.

## Documentation placement

- `README.md`: user-facing product and development overview for the whole workspace
- `docs/spec/`: normative behavior and architecture contracts
- `docs/runbook/`: procedural and maintenance runbooks
- `docs/reference/`: descriptive layout, ownership, and implementation references
- `docs/decisions/`: durable rationale records for accepted tradeoffs

## Local-only and generated directories

These paths are intentionally ignored and should not be treated as tracked repository structure:

- `target/`: Rust build products, benchmark outputs, and local analysis artifacts
- `target/rsnap-native-host/`: locally staged native-host `Rsnap.app` bundles from
  `scripts/build_and_run.sh`
- `.worktrees/`: local git worktree lanes
- `.workspaces/`: local clone-backed workspace lanes from older workflows
- `.codex/`: local agent/runtime state

If one of these directories appears in a filesystem listing, treat it as local environment noise
unless a task explicitly concerns runtime lane management or generated outputs.

## Structure assessment

The current directory structure is still navigable and serviceable for the checked-in codebase:

- the top-level split between `apps/` and `packages/` still reflects the actual tree
- `docs/`, `assets/`, and `scripts/` remain at the correct shared-workspace level
- several large files have already been split into focused submodules

But the active architecture direction has changed:

- the durable story is no longer "app shell + one overlay/runtime authority"
- the durable story is "native host owns OS semantics, Rust core owns product semantics"

Use this reference for current filesystem routing, not as the final architecture source of truth.
