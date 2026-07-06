---
title: "Workspace Layout Reference"
description: "Workspace Layout Reference documentation for Rsnap."
type: "Reference"
status: active
authority: normative
owner: hack-ink/rsnap
last_verified: 2026-07-06
---
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
| `apps/rsnap-perf/` | Deterministic local performance sweep: fixed fixture construction, checksum-backed correctness checks, case timing, and budget assertions |
| `packages/rsnap-overlay/` | Narrow Rust transition crate: retained frozen edit/export logic, text rendering, and macOS live-sampling adapters that have not yet moved into durable owners |
| `packages/rsnap-capture-core/` | Durable Rust product-semantics and image-algorithm crate: shared geometry, semantic scene model, host/core protocol enums, reset-native session core, export/crop/PNG encoding, capture-frame rendering, wallpaper thumbnail, minimap, mosaic, selection transform, auto-center, scroll stitching, and live-sample helpers |
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

### `apps/rsnap-perf/`

Treat `apps/rsnap-perf/` as the deterministic local performance sweep.

It owns:

- fixed export, capture-frame, frozen-edit, and scroll-capture performance cases
- checksum-backed correctness checks that guard benchmark fixture drift
- deterministic fixture construction under `apps/rsnap-perf/src/fixtures.rs`
- timing and budget result reporting under `apps/rsnap-perf/src/measurement.rs`

Key paths:

- `apps/rsnap-perf/src/main.rs`: perf sweep orchestration plus semantic verification for each
  case family
- `apps/rsnap-perf/src/fixtures.rs`: deterministic fixture, checksum, and expected-value support
- `apps/rsnap-perf/src/measurement.rs`: per-case timing, formatting, and budget enforcement

### `packages/rsnap-overlay/`

Treat `packages/rsnap-overlay/` as a narrowed transition crate for Rust-owned behavior that still
has native-host callers or deterministic validation surfaces but has not yet moved into
`rsnap-capture-core`.

Today it owns:

- frozen edit/export logic that is still Rust-owned but has not yet moved into `rsnap-capture-core`
- text annotation measurement/rendering helpers reused by Rust export paths
- macOS live-frame and live-sampling adapters used through `rsnap-host-ffi`

Important:

- The legacy Rust overlay UI/runtime, backend worker, replay harness, trace recorder, shaders, and
  window/input/rendering tree have been removed from the compiled and tracked source.
- This crate is still not the long-lived reset target.
- New work should avoid making `rsnap-overlay` authoritative for OS-facing window, focus, cursor,
  IME, permission, or capture-capability semantics; those belong on the native-host side of the
  reset boundary.

Key paths:

- `packages/rsnap-overlay/src/lib.rs`: explicit public surface for the remaining transition
  helpers: frozen edit/export and macOS live sampling
- `packages/rsnap-overlay/src/frozen_edit.rs`: Rust-owned frozen overlay edit session state
  machine, including edit geometry, text hit bounds, and movement clamping policy; element models
  live under `frozen_edit/elements.rs`
- `packages/rsnap-overlay/src/frozen_export.rs`: Rust-owned frozen-overlay export compositor;
  export element schema lives under `frozen_export/model.rs`, and stroke rasterization lives under
  `frozen_export/stroke_raster.rs`
- `packages/rsnap-overlay/src/text_rendering.rs`: text annotation rasterization for Rust export
  paths
- `packages/rsnap-overlay/src/system_fonts.rs`: system font discovery and font payload selection
  for Rust text rendering
- `packages/rsnap-overlay/src/point.rs`: crate-local pixel point type used by export/text
  geometry that no longer depends on a UI toolkit point type
- `packages/rsnap-overlay/src/host_live_sampling_macos.rs`: macOS live sampler adapter exposed
  through `rsnap-host-ffi`
- `packages/rsnap-overlay/src/live_frame_stream_macos.rs`: macOS ScreenCaptureKit live-frame stream
  support; focused worker, setup, lifecycle, output, filtering, and buffer modules live under
  `live_frame_stream_macos/`
- `packages/rsnap-overlay/src/macos_color.rs`: CoreGraphics color-managed image conversion helpers
- `packages/rsnap-overlay/src/state.rs`: transition payload types and re-exported capture-core
  geometry used by FFI-facing live sampling

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
- scroll-capture stitching/session logic, deterministic tests, and the Criterion benchmark target
- mosaic patch generation
- frozen selection hit-testing and transform geometry
- auto-center content-bound detection and margin-balance rules

This crate must stay free of:

- top-level window ownership
- `winit` or `egui` runtime authority
- AppKit ownership
- host-side permission or capture capability code

Key scroll-capture paths:

- `packages/rsnap-capture-core/src/scroll_capture.rs`: scroll-capture session entry with focused
  support modules under `scroll_capture/`
- `packages/rsnap-capture-core/src/scroll_stitching.rs`: public native-host stitching wrapper used
  by `rsnap-host-ffi`
- `packages/rsnap-capture-core/src/scroll_capture/worker_pairwise.rs`: ordered worker-pairwise
  frame registration, committed-frontier catchup, rewind/reacquire handling, and growth-block
  decisions
- `packages/rsnap-capture-core/src/scroll_capture/types.rs`: scroll-capture data model,
  observation outcomes, registration candidates, and telemetry structs shared by the session
  modules
- `packages/rsnap-capture-core/src/scroll_capture/support.rs`: shared pixel matching,
  static-region rejection, image stacking/resizing, and image-analysis helpers used by scroll
  capture
- `packages/rsnap-capture-core/benches/scroll_capture.rs`: deterministic hot-path benchmark target
  for scroll stitching, fingerprinting, overlap matching, and one-step session commit

### `packages/rsnap-host-ffi/`

Treat `packages/rsnap-host-ffi/` as the thin ABI companion to `rsnap-capture-core/`.

It owns:

- opaque session handles for foreign hosts
- FFI-safe config, event, report, scene, request, live-sample, frozen-overlay, capture-frame, and
  scroll-capture types under the `abi/` module tree
- exported `extern "C"` functions that forward into `rsnap-capture-core`
- exported `extern "C"` functions that bridge retained Rust transition modules while they migrate
  toward `rsnap-capture-core`
- the checked-in C header consumed by the native macOS host:
  `packages/rsnap-host-ffi/include/rsnap_host_ffi.h`

It does not own product behavior beyond ABI adaptation.

Key paths:

- `packages/rsnap-host-ffi/src/abi.rs`: ABI constants plus the canonical re-export surface
- `packages/rsnap-host-ffi/src/abi/handles.rs`: opaque Rust-owned handles for foreign callers
- `packages/rsnap-host-ffi/src/abi/geometry.rs`: shared FFI-safe geometry, RGB, and owned-buffer
  payloads
- `packages/rsnap-host-ffi/src/abi/session.rs`: session config, host event/report, scene, toolbar,
  and host-request payloads
- `packages/rsnap-host-ffi/src/abi/live.rs`: live-sampler cursor sample payloads
- `packages/rsnap-host-ffi/src/abi/frozen_overlay.rs`: frozen overlay edit/export and selection
  transform payloads
- `packages/rsnap-host-ffi/src/abi/capture_frame.rs`: capture-frame planning/rendering payloads
- `packages/rsnap-host-ffi/src/abi/scroll.rs`: scroll minimap and observation payloads

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
- `CaptureSessionController+ScrollCapture.swift`: native scroll-capture lifecycle startup,
  viewport geometry, sampling-loop scheduling, and scroll minimap chrome setup
- `CaptureSessionController+ScrollCaptureObservation.swift`: native scroll-capture live/fallback
  sample drains, observation batching, stitch result application, and preview refresh
- `NativeScrollCaptureWheelInput.swift`: native scroll-capture wheel interception, global monitor
  lifecycle, forwarded CGEvent posting, queued forwarded-delta draining, and motion-hint updates
- `NativeScrollCaptureObservationPipeline.swift`: conversion of ordered native samples and
  fallback frames into Rust scroll observations plus preview export batches
- `CaptureSessionController+Export.swift`: copy/save host effects, output naming, and
  capture-image export orchestration
- `CaptureSessionController+PreparedExport.swift`: frozen export render request construction,
  prepared export invalidation, scroll/annotation quiet-delay scheduling, and OCR image prewarming
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
- `CaptureHostScrollToolbarBackdropWorker.swift`: scroll toolbar backdrop live-frame freshness,
  signature hashing, fallback capture selection, and capture result shaping
- `CaptureHostView.swift`: AppKit view orchestration, hit testing, and frozen presentation
  rendering
- `CaptureHostMaterialViewCoordinator.swift`: Liquid Glass/material subview ownership, classic glass
  patch resolution, and scroll-toolbar backdrop refresh scheduling plus view installation
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
- `LiveHudHexRollPlan.swift`: deterministic pending/resolved hex-roll digit sequences,
  direction choices, durations, and phase offsets for live HUD color animation
- `LiveHudColorRollTextLayerFactory.swift`: CATextLayer construction and text application helpers
  for live HUD color roll stacks
- `LiveOverlayTypography.swift`: shared native live overlay font and text metrics
- `LiveOverlayLayers.swift`: reusable Core Animation layer subclasses for live selection flow and
  scrim masking
- `LiveChromePlacement.swift`: live HUD/loupe text metrics, pending color text, and deterministic
  floating placement geometry shared by capture host and live chrome rendering
- `LiveOverlayWindowSnapshotFeed.swift`, `LiveOverlayChromeSampleFeed.swift`,
  `LiveOverlayChromeSamplePolicy.swift`, and `LiveFrameClockDriver.swift`: live overlay support
  boundaries for target-window snapshots, chrome color/patch sampling and cache policy, and
  display-rate frame ticks
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
- `FrozenAnnotationStyles.swift`: frozen annotation colors, style toolbar state, and Swift/Rust
  frozen overlay style conversion
- `FrozenCaptureModels.swift`: Swift adapter models for Rust-owned frozen overlay editing,
  annotation draw models, capture chrome state, scroll minimap state, and toolbar layout models
- `NativeHostFeedbackSound.swift`: host-side sound lookup/playback for completion effects
- `NativeHostImageBridge.swift`: RGBA/CoreGraphics image conversion used by the FFI bridge
- `Sources/RsnapHostBridge/HostSessionFFI.swift`: Swift bridge session protocol models and
  session-handle adaptation used by the native host
- `Sources/RsnapHostBridge/HostFFI.swift`: shared RGB/RGBA models plus selection-transform,
  auto-center, and BGRA frame-sampling bridge surfaces used by the native host
- `Sources/RsnapHostBridge/ScrollCaptureFFI.swift`: Swift bridge scroll minimap planning,
  scroll-capture observation models, session-handle lifecycle, and stitched preview/export image
  adaptation
- `Sources/RsnapHostBridge/ExportEncoderFFI.swift`: Swift bridge PNG encoding, frozen display
  crop, mosaic privacy patch, and frozen overlay export-image adaptation
- `Sources/RsnapHostBridge/LiveSamplerFFI.swift`: Swift bridge live sampler models,
  sampler-handle lifecycle, and live RGBA region frame adaptation
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
