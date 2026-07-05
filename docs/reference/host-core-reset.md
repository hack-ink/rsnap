# Host/Core Reset Reference

Purpose: Describe the active target architecture for the Rsnap reset lane and how new work should
behave while the checked-in codebase is still transitional.

Read this when: You are planning architecture work, deciding whether a change deepens the right
boundary, or trying to understand the intended end state of the reset project.

Inputs: `docs/spec/platform-host-boundary.md`; `docs/spec/capture-session.md`;
`docs/decisions/native-host-rust-core-reset.md`; `docs/reference/workspace-layout.md`

Depends on: `docs/spec/platform-host-boundary.md`;
`docs/decisions/native-host-rust-core-reset.md`

Covers: The target architecture, migration posture, and the relationship between the target design
and the current checked-in repository layout.

## Target architecture

The active reset target is:

- native platform hosts own operating-system semantics
- Rust owns cross-platform product semantics

In practical terms:

- native hosts own capture-window lifecycle, focus/activation, cursor, IME, permissions, and
  native capture capabilities. On macOS, Swift also discovers OS-only resources such as the current
  wallpaper path, converts captured images into bridgeable buffers, presents returned pixels, and
  performs host-side effects such as clipboard, save-panel, OCR, and update UI.
- Rust owns capture-session state, geometry, annotations, export composition, capture-frame
  planning/rendering, wallpaper thumbnail decoding/caching, scroll stitching, minimap planning,
  selection transforms, auto-centering, replay, and deterministic product logic.
- host and core communicate through an explicit protocol instead of sharing ownership of OS-facing
  behavior

## What this means for the current tree

The checked-in repository does not yet fully match the target design.

Today:

- `apps/rsnap/` is now the thin launcher/bootstrap layer for the staged native host bundle
- `packages/rsnap-overlay/` is still a large transitional runtime container, but its public root
  now centers on session/replay surfaces while remaining macOS host adapters stay behind explicit
  host modules
- `packages/rsnap-capture-core/` is now the checked-in landing zone for portable geometry,
  semantic scene models, host/core protocol types, export/crop/PNG encoding, capture-frame
  planning/rendering, wallpaper thumbnail decode/cache, minimap planning, mosaic generation,
  frozen-selection transforms, auto-centering, and live-sample pixel helpers
- `packages/rsnap-host-ffi/` is now the checked-in thin C ABI bridge for the native macOS host and
  ships the checked-in header at `packages/rsnap-host-ffi/include/rsnap_host_ffi.h`
- `native/macos-host/` is now the visible app shell and owns clipboard, save, and deferred OCR
  publication for the reset lane. It calls `RsnapHostBridge` for Rust-owned session, export,
  capture-frame, wallpaper thumbnail, minimap, selection-transform, auto-center, and sampling
  algorithms rather than keeping parallel Swift implementations.

During the reset, treat these as implementation containers rather than the final architecture
story.

## Current macOS Swift residual map

The remaining Swift code is not expected to collapse to zero during the reset. It should shrink only
where Swift is still holding deterministic product logic that belongs in Rust.

The current native-host Swift split is:

- `NativeHostApp.swift`: app delegate, menu bar lifecycle, settings/hotkey wiring, launch,
  permission orchestration, onboarding, software-update wiring, and status-menu state.
- `CaptureSessionController.swift` plus `CaptureSessionController+*.swift`: the Swift host-session
  coordinator around the Rust `RsnapHostSession`. The base file owns shared controller state and
  lifecycle hooks; the extensions split live capture/input, frozen selection interactions,
  host-request draining, native scroll-capture sampling, copy/save/export effects, Vision OCR, and
  runtime teardown/window helpers.
- `NativeScrollCaptureObservationPipeline.swift`: native scroll-capture sample batching, fallback
  sample adaptation, Rust scroll-observation calls, and preview export refresh packaging. It keeps
  ordered frame acquisition and AppKit scheduling in Swift while leaving stitching decisions in
  Rust.
- `CaptureChrome.swift`: shared native chrome metrics, palette, dashed-border geometry, and
  AppKit color/image helpers used by live and frozen capture UI.
- `CaptureOverlayWindow.swift`: the AppKit `NSPanel` wrapper that embeds `CaptureHostView` for each
  capture overlay window.
- `CaptureOverlayController.swift`: AppKit overlay-window set management, focus/first-responder
  routing, capture-stream preparation, mouse passthrough, and CoreGraphics capture sources needed
  to sample below native overlay windows.
- `CaptureHostAnnotationStyleWheelGate.swift`: frozen annotation-size wheel dead-zone and
  per-gesture step throttling.
- `CaptureHostToolbarHoverState.swift`: frozen toolbar hover target state, change detection, and
  clearing behavior.
- `CaptureHostFrozenFirstDisplayHandoffState.swift`: capture-host frozen-entry first-display
  handoff state, completion queueing, pending-frame evidence, and deferred classic toolbar glass.
- `CaptureHostScrollToolbarBackdropState.swift`: capture-host scroll toolbar backdrop capture
  generation, seed-patch cache, active frame, refresh cadence, and change-count state.
- `CaptureHostView.swift`: AppKit/Quartz drawing, hit testing, Liquid Glass surfaces, live/frozen
  HUD and toolbar orchestration, and native pointer/key event routing into the session controller.
- `CaptureHostFrozenOverlayRenderer.swift`: frozen annotation overlay rendering for mosaic,
  spotlight, pen, arrow, and text overlays.
- `CaptureHostLiveSampleCache.swift`: capture-host live chrome/RGB sample reuse cache and pointer
  sample matching.
- `CaptureHostLivePrimaryInteractionState.swift`: capture-host live primary drag, release,
  completion, and hover-suppression state transitions.
- `CaptureHostPointerDispatch.swift`: capture-host pointer dispatch events, a shared delivery queue
  with per-track throttling state, and AppKit-to-controller pointer delivery support.
- `LiveOverlayRenderer.swift`: live overlay render orchestration for HUD, loupe, frozen-pending
  preview, and live chrome layers.
- `LiveOverlayLayers.swift`: reusable Core Animation layer subclasses for live selection flow and
  scrim masking.
- `LiveChromePlacement.swift`: live HUD/loupe text metrics, pending color text, and deterministic
  floating placement geometry shared by capture host and live chrome rendering.
- `LiveOverlayWindowSnapshotFeed.swift`, `LiveOverlayChromeSampleFeed.swift`, and
  `LiveFrameClockDriver.swift`: native live overlay support boundaries for target-window snapshots,
  chrome color/patch sampling, and display-rate frame ticks.
- `FrozenToolbarLayoutPlanner.swift`: deterministic frozen-toolbar item availability, layout, and
  hit-test geometry used by AppKit drawing, Liquid Glass toolbar content, and native probes.
- `FrozenToolbarRenderView.swift`: shared frozen-toolbar content drawing for classic AppKit
  toolbar rendering and Liquid Glass toolbar content.
- `CaptureGeometry.swift`, `CaptureHostAnnotationStyleWheelGate.swift`,
  `CaptureHostToolbarHoverState.swift`, `CaptureHostFrozenFirstDisplayHandoffState.swift`,
  `CaptureHostScrollToolbarBackdropState.swift`, `CaptureHostCursorSupport.swift`,
  `CaptureHostLiveSampleCache.swift`, `CaptureHostLivePrimaryInteractionState.swift`,
  `CaptureHostPointerDispatch.swift`, `CaptureHostFrozenImageEffects.swift`,
  `LiveChromeRefreshTelemetryKey.swift`, and `NativeHostTextMetrics.swift`: focused support
  boundaries for shared capture geometry, frozen annotation-size wheel gating, frozen toolbar hover
  state, frozen first-display handoff state, scroll toolbar backdrop state, capture-host cursor
  presentation and NSCursor adaptation, live sample reuse, live primary interaction state, pointer
  dispatch queue throttling, Rust-backed frozen image effects, live-chrome telemetry identity, and
  native text measurement.
- `FrozenCaptureModels.swift`: Swift view-adapter state for Rust-owned frozen overlay editing,
  including conversion from Rust edit snapshots into AppKit draw models.
- `NativeHostFeedbackSound.swift`: host-side `NSSound` lookup/playback for capture and OCR
  completion effects.
- `NativeHostImageBridge.swift` and `RsnapHostBridge`: conversion and FFI glue between
  CoreGraphics/AppKit images and Rust-owned RGBA snapshots.
- `NativeHostSettingsView.swift`, `NativeHostSettingsNavigation.swift`,
  `NativeHostSettingsSurface.swift`, `NativeHostAppearanceSettings.swift`, and
  `NativeHostCaptureFrameSettings.swift`: SwiftUI settings shell, navigation, reusable settings
  surfaces, appearance controls, and capture-frame preset controls.

This means a large Swift line count can still be reasonable when those lines are AppKit,
CoreGraphics, ScreenCaptureKit, Vision, pasteboard, save-panel, sound, update, or window lifecycle
glue. A large Swift line count is suspicious only when it reintroduces product-state machines,
portable geometry decisions, export byte generation, image algorithms, or duplicate planning logic
that already has a Rust entrypoint.

## Migration posture

New work in the reset lane should prefer changes that:

- clarify host versus core ownership
- pull OS-facing semantics toward native hosts
- make product semantics more explicit and portable inside Rust
- replace legacy mixed-ownership paths with protocol boundaries

New work should avoid spending the reset lane on changes that only make the old architecture more
comfortable, such as:

- preserving a split-window or split-shell ownership model as the active target
- filing or landing cleanup that only splits files without clarifying the future boundary
- hard-coding product behavior around a specific platform shell implementation

Current reset posture for the scroll-capture slice:

- the native app host owns scroll-capture permission checks, external scroll-input observer
  lifecycle, native scroll-input normalization, and screenshot capability acquisition
- the Rust overlay core owns scroll-capture session state, overlap proof, stitching, and
  fail-closed product semantics
- capability start/stop, frame delivery, and host-side failures must cross the boundary as explicit
  host/core protocol calls instead of implicit worker ownership inside the overlay runtime

Current reset posture for the boundary slice:

- durable geometry and scene protocol types now belong in `rsnap-capture-core`
- native-host ABI entry points now belong in `rsnap-host-ffi`
- final-byte and performance-sensitive image algorithms should move behind Rust ABI entry points as
  reusable cross-platform core work, while Swift stays limited to OS acquisition, presentation, and
  host-side effects
- targeted reset-slice validation now lives at `cargo make test-host-reset`
- `apps/rsnap/` and `rsnap-overlay/` should treat those crates as the migration target instead of
  inventing parallel durable protocol types inside legacy containers

If further optimization is needed, prefer this order:

1. Continue structure-only Swift splits when a file mixes app lifecycle, capture-session
   orchestration, and AppKit rendering in a way that makes review risky.
2. Move deterministic planners or pixel algorithms to `rsnap-capture-core` and expose them through
   `rsnap-host-ffi` only when Swift still owns the decision or byte generation.
3. Keep OS acquisition, AppKit presentation, Vision OCR, pasteboard/save-panel, focus, cursor,
   permissions, and update UI in Swift unless a future platform host supplies an equivalent native
   adapter.

## Vertical-slice model

The reset is intended to land as vertical slices rather than as one giant rewrite.

Expected slice order:

1. docs and architecture boundary
2. native host ownership of window/input/focus on macOS
3. live targeting plus display-first Frozen entry on the new boundary
4. export-authority effects on the new boundary
5. text and IME on the new boundary
6. scroll capture on the new boundary
7. validation and performance hardening across the new boundary

## Historical material

Superseded shell-era history is intentionally excluded from the active reset corpus.

Plan new work from the current specs, runbooks, references, and accepted decision record instead.
