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
- `CaptureSessionController+ScrollCapture.swift`: native scroll-capture lifecycle, viewport
  sampling, observation scheduling, and scroll minimap preview refresh
- `NativeScrollCaptureWheelInput.swift`: native scroll-capture wheel interception, global monitor
  lifecycle, forwarded CGEvent posting, queued forwarded-delta draining, and motion-hint updates
- `NativeScrollCaptureObservationPipeline.swift`: conversion of ordered native samples and
  fallback frames into Rust scroll observations plus preview export batches
- `CaptureSessionController+Export.swift`: copy/save host effects, output naming, prepared export
  scheduling, and capture-image export orchestration
- `CaptureSessionController+TextRecognition.swift`: Vision OCR request execution and recognized
  text pasteboard publication
- `CaptureSessionController+Runtime.swift`: shared monitor/window lookup, overlay refresh,
  teardown, status message, and capture-stream release helpers
- `CaptureChrome.swift`: shared native chrome metrics, palette, and drawing geometry
- `CaptureOverlayWindow.swift`: AppKit `NSPanel` wrapper for capture overlay windows
- `CaptureOverlayController.swift`: overlay window set, focus, stream preparation, and mouse
  passthrough management
- `CaptureOverlayImageSampler.swift`: CoreGraphics below-overlay capture and display-point sample
  adaptation for live chrome, frozen capture effects, and scroll fallback acquisition
- `FrozenFrameAuthority.swift`: frozen-frame authority state, frame storage, setup completion, and
  telemetry bookkeeping
- `FrozenFrameAuthority+StreamSetup.swift`: ScreenCaptureKit frozen-frame stream setup,
  shareable-content lookup, self-capture-safe stream gating, and lifecycle reset
- `FrozenFrameAuthority+SnapshotResolution.swift`: frozen-frame latch-token resolution, RGB/loupe
  sampling, self-capture-safe frame gating, and fresh-frame authority decisions
- `FrozenFrameStreamOutput.swift`: ScreenCaptureKit stream-output delegate adaptation,
  usable-frame filtering, display-time conversion, and frame-record emission into the authority
- `FrozenFrameContentFilterPlanner.swift`: frozen-frame display target planning, shareable-content
  cache freshness, self-capture-excluding content filter construction, and stream configuration
- `NSScreenDisplayID.swift`: shared AppKit display-ID extraction for native capture surfaces
- `FrozenFramePixelBufferBridge.swift`: CVPixelBuffer lock/lifetime adaptation for frozen-frame
  CGImage creation, RGB sampling, and loupe patch extraction
- `CaptureHostAnnotationStyleWheelGate.swift`: frozen annotation-size wheel dead-zone and
  per-gesture step throttling
- `CaptureHostToolbarHoverState.swift`: frozen toolbar hover target state, change detection, and
  clearing behavior
- `CaptureHostFrozenToolbarCoordinator.swift`: frozen toolbar visible item planning, hit testing,
  hover state ownership, and toolbar action dispatch into the session controller
- `CaptureHostFrozenFirstDisplayHandoffState.swift`: frozen-entry first-display handoff state,
  completion queueing, pending-frame evidence, and deferred classic toolbar glass
- `CaptureHostScrollToolbarBackdropState.swift`: scroll toolbar backdrop capture generation,
  seed-patch cache, active frame, refresh cadence, and change-count state
- `CaptureHostView.swift`: AppKit view orchestration, hit testing, and frozen presentation
  rendering
- `CaptureHostMaterialViewCoordinator.swift`: Liquid Glass/material subview ownership, classic glass
  patch resolution, and scroll-toolbar backdrop refresh/capture scheduling
- `CaptureHostGlassPatchResolver.swift`: classic glass patch cache lookup, frozen display crop
  extraction, and CoreImage blur/tint adaptation for capture-host HUD, loupe, and toolbar surfaces
- `FrozenPreparedExportStore.swift`: frozen export render requests, prepared export cache keys,
  copy/save/recognize-text job result models, and thread-safe prepared image stores
- `FrozenSelectionImageRenderer.swift`: frozen selection render jobs, capture-frame effect
  application, overlay composition, display cropping, and PNG encoding for copy/save/OCR
  preparation
- `CaptureHostFrozenPresentationRenderer.swift`: frozen display surface, selection chrome, overlay,
  minimap, size badge, and classic toolbar drawing orchestration from an explicit host context
- `CaptureHostFrozenSelectionChromeRenderer.swift`: frozen selection scrim, dashed border, resize
  handles, and selection-size badge rendering
- `CaptureHostFrozenOverlayRenderer.swift`: frozen annotation overlay rendering for mosaic,
  spotlight, pen, arrow, and text overlays
- `CaptureHostScrollMinimapRenderer.swift`: frozen scroll-capture minimap presentation over the
  Rust-owned minimap layout plan and host-provided preview image
- `CaptureHostLiveSampleCache.swift`: capture-host live chrome/RGB sample reuse cache and pointer
  sample matching
- `CaptureHostLiveSampleResolver.swift`: capture-host live chrome/RGB sample resolution,
  loupe-patch reuse, and cache seeding policy
- `CaptureHostLiveInputTelemetry.swift`: capture-host live pointer/mouse input telemetry,
  pointer-event gap recording, and live-chrome input summary emission
- `CaptureHostLivePointerPreviewState.swift`: capture-host live pointer preview point,
  input-latency timestamp, sequence, and duplicate-move suppression state
- `CaptureHostLivePrimaryInteractionState.swift`: capture-host live primary drag, release,
  completion, and hover-suppression state transitions
- `CaptureHostMouseReleaseRecovery.swift`: capture-host local mouse-up monitor and live/frozen
  release-watchdog scheduling for AppKit interactions whose mouse-up event can be missed
- `CaptureHostPointerDispatch.swift`: capture-host pointer dispatch events, a shared delivery queue
  with per-track throttling state, and AppKit-to-controller pointer delivery
- `CaptureHostView+InputRouting.swift`: capture-host AppKit mouse, wheel, key, cursor, toolbar
  shortcut, and pointer-dispatch routing into the session controller
- `CaptureHostView+LivePrimaryInteraction.swift`: capture-host live primary interaction release
  recovery, pointer-preview mutation, mouse-up monitor wiring, and release-watchdog orchestration
- `CaptureHostView+LivePreview.swift`: capture-host live preview snapshots, HUD/loupe placement,
  live sample-cache use, and controller preview-demand updates
- `LiveOverlayRenderer.swift`: live overlay render orchestration for HUD, loupe, frame clock,
  chrome transactions, and layer setup
- `LiveOverlayRenderer+FocusRendering.swift`: live overlay frozen display, focus scrim,
  selection-flow, frozen-pending, drag-selection, and size-badge rendering
- `LiveHudColorRollCoordinator.swift`: live HUD color swatch/hex presentation, pending color roll
  animation state, resolved hex roll transitions, and roll-layer lifecycle
- `LiveOverlayTypography.swift`: shared native live overlay font and text metrics
- `LiveOverlayLayers.swift`: reusable Core Animation layer subclasses for live selection flow and
  scrim masking
- `LiveChromePlacement.swift`: live HUD/loupe text metrics, pending color text, and deterministic
  floating placement geometry shared by capture host and live chrome rendering
- `LiveOverlayWindowSnapshotFeed.swift`, `LiveOverlayChromeSampleFeed.swift`, and
  `LiveFrameClockDriver.swift`: live overlay support boundaries for target-window snapshots,
  chrome color/patch sampling, and display-rate frame ticks
- `FrozenToolbarLayoutPlanner.swift`: deterministic frozen-toolbar item availability, layout, and
  hit-test geometry shared by classic drawing, Liquid Glass content, and native probes
- `FrozenToolbarRenderView.swift`: shared frozen-toolbar content drawing for classic AppKit
  toolbar rendering and Liquid Glass toolbar content
- `CaptureGeometry.swift`, `CaptureHostAnnotationStyleWheelGate.swift`,
  `CaptureHostToolbarHoverState.swift`, `CaptureHostFrozenFirstDisplayHandoffState.swift`,
  `CaptureHostScrollToolbarBackdropState.swift`, `CaptureHostCursorSupport.swift`,
  `CaptureHostScrollMinimapRenderer.swift`, `CaptureHostLiveSampleCache.swift`,
  `CaptureHostLiveSampleResolver.swift`, `CaptureHostLiveInputTelemetry.swift`,
  `CaptureHostLivePointerPreviewState.swift`, `CaptureHostLivePrimaryInteractionState.swift`,
  `CaptureHostMouseReleaseRecovery.swift`, `CaptureHostPointerDispatch.swift`,
  `CaptureHostFrozenImageEffects.swift`, `CaptureHostGlassPatchResolver.swift`,
  `FrozenFramePixelBufferBridge.swift`, `FrozenPreparedExportStore.swift`,
  `LiveChromeRefreshTelemetryKey.swift`, and `NativeHostTextMetrics.swift`: focused support
  boundaries for shared capture geometry, frozen annotation-size wheel gating, frozen toolbar hover
  state, frozen first-display handoff state, scroll toolbar backdrop state, capture-host cursor
  presentation and NSCursor adaptation, frozen minimap presentation, live sample reuse, live sample
  resolution, live input telemetry, live pointer preview state, live primary interaction state,
  AppKit mouse-release recovery, pointer dispatch queue throttling, Rust-backed frozen image
  effects, capture-host glass patch caching and blur/tint adaptation, frozen-frame pixel-buffer
  image/sampling adaptation, prepared export cache ownership, live-chrome telemetry identity, and
  shared native text measurement
- `FrozenCaptureModels.swift`: Swift adapter models for Rust-owned frozen overlay editing
- `NativeHostFeedbackSound.swift`: host-side sound lookup/playback for completion effects
- `NativeHostImageBridge.swift`: RGBA/CoreGraphics image conversion used by the FFI bridge
- `Sources/RsnapHostBridge/HostSessionFFI.swift`: Swift bridge session protocol models and
  session-handle adaptation used by the native host
- `Sources/RsnapHostBridge/HostFFI.swift`: Swift bridge scroll, live-sampling, export, and
  remaining non-frozen FFI surfaces used by the native host
- `Sources/RsnapHostBridge/CaptureFrameFFI.swift`: capture-frame planning/rendering and wallpaper
  thumbnail bridge models over Rust-owned capture-frame algorithms
- `Sources/RsnapHostBridge/FrozenOverlayFFI.swift`: Swift bridge models and storage for frozen
  overlay edit/export FFI calls
- `Sources/RsnapHostBridge/HostFFISupport.swift`: shared Swift bridge status, geometry, and
  owned-buffer adaptation helpers
- `NativeHostSettingsView.swift`, `NativeHostSettingsNavigation.swift`, and
  `NativeHostSettingsSurface.swift`: SwiftUI settings view model, shell layout, navigation, and
  reusable settings surfaces
- `NativeHostAppearanceSettings.swift`, `NativeHostCaptureSettingsPanel.swift`,
  `NativeHostOutputSettingsPanel.swift`, `NativeHostCaptureFrameSettings.swift`,
  `PermissionsSettingsPanel.swift`, and `NativeHostAboutSettingsPanel.swift`: focused settings
  panels for appearance, capture shortcuts/input, output location/naming, capture-frame presets,
  permission/setup controls, and about/update controls

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
