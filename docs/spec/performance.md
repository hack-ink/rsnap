---
title: "Rsnap Performance Contract"
description: "Rsnap Performance Contract documentation for Rsnap."
type: "Spec"
status: active
authority: normative
owner: hack-ink/rsnap
last_verified: 2026-07-06
---
# Rsnap Performance Contract

Purpose: Define the normative performance-tracking contract for Rsnap so render cadence,
measurement surfaces, thresholds, and known contract gaps are explicit and shared across code,
benchmarks, smoke harnesses, and tracker issues.

Status: normative

Read this when: You are implementing, reviewing, or validating render scheduling, overlay redraw,
live/loupe responsiveness, component render benchmarks, or scroll-capture performance tracking.

Not this document: Step-by-step benchmark procedures, smoke-run instructions, or descriptive
implementation notes. Use `docs/runbook/` for procedures and `docs/reference/` for current
implementation context.

Primary procedures:
- `docs/runbook/performance-validation.md` for repo-native performance command entrypoints
- `docs/runbook/scroll-capture-benchmarks.md` for scroll-capture benchmark fixtures and baseline
  use

Defines:
- the active render cadence contract for Rsnap UI and overlay paths
- the tracked performance scenarios and their primary metrics
- the distinction between target cadence, diagnostic thresholds, and coarse smoke gates
- the current cadence implementation status and any known gap against the agreed contract

## Scope

This contract applies to performance tracking for actively rendered Rsnap surfaces:

- live overlay redraw
- HUD and loupe movement while live
- frozen-mode floating UI that redraws in response to active interaction
- render-heavy component paths that should be benchmarked against the same frame-budget family
- scroll-capture hot paths that need deterministic benchmark coverage even when they are not
  driven by live GUI cadence

This contract does not require idle surfaces to redraw continuously when there is no interaction
or animation.

## Cadence classes

Rsnap tracks two different cadence classes. They must not be collapsed into one number during
implementation, telemetry review, or smoke interpretation.

### Display-bound presentation cadence

Visible presentation is constrained by the active display. For actively rendered UI and overlay
paths that the user sees on screen, target cadence is:

`min(active display maximum refresh rate, 120 Hz)`

Practical meaning:

- On a `120 Hz` or faster display, the target frame budget is `8.33 ms`.
- On a `60 Hz` display, the target frame budget is `16.67 ms`.
- If the display refresh rate is unavailable, Rsnap uses the conservative `60 Hz` fallback.
- A `60 Hz` panel cannot physically display `120` distinct visual updates per second. On that
  hardware, the visual contract is one responsive update per display frame, not impossible
  `8.33 ms` presentation.

Target frame budget:

| Target cadence | Target frame budget |
| --- | --- |
| `120 Hz` | `8.33 ms` |
| `60 Hz` | `16.67 ms` |

Display-bound surfaces include:

- live overlay, HUD, loupe, and selection chrome presentation
- frozen toolbar and frozen selection chrome presentation
- CALayer/AppKit frame-clock metrics such as `live_chrome.frame_tick_gap`,
  `live_chrome.layer_chrome_render_gap`, and `live_chrome.active_layer_chrome_render_gap`

### Fixed 120 Hz sampling and state cadence

Sampling and state feeds are not limited by what the display can present. While the relevant live
surface is active, these paths target `120 Hz` / `8.33 ms` even on a `60 Hz` display:

- live color RGB sampling, including stationary pointer sampling over dynamic backgrounds
- loupe patch sampling
- internal sample-feed refresh metrics such as `live_chrome.sample_refresh_gap`
- input-state ingestion or prediction work where a faster internal feed avoids missed visible
  frames

The user may only see the latest sampled value on the next display frame, but the backing value
must still be refreshed at the fixed sampling cadence so dynamic content does not appear stale.

### One-shot handoff latency

Mode transitions are not continuous cadence loops, but they are user-visible. The live-to-frozen
release handoff must avoid waiting for a future ScreenCaptureKit frame when a freshness-proven
frozen authority frame is already available. The intended path is:

1. use a `post_token` frozen authority frame, a fresh `latest_unchanged` authority frame, or a
   current `screenshot_manager` capture through the active self-capture-safe ScreenCaptureKit
   filter for an unchanged/static desktop,
2. present the frozen frame, toolbar, and scrim in one continuous handoff,
3. perform cleanup such as secondary-window collapse after the first frozen frame is installed.

The handoff must not use cache-only live-sampler latest-monitor snapshots unless they carry real
source frame age and sequence metadata. A wrapper that stamps cached pixels with the current call
time can hide seconds-stale screenshots and is a correctness regression.

The handoff must not show a pending half-frame, remove/re-add the outside-selection scrim, or
delay toolbar visibility on a static desktop just because ScreenCaptureKit has not emitted a new
post-latch frame.

The first capture after app launch is part of this contract. Initial stream prewarm may happen in
the background, but it must not make the first Frozen toolbar appear visibly later than the frozen
display image.

This cadence contract is normative even when current logs or smoke harnesses use coarser warning
thresholds.

## Measurement model

The performance contract distinguishes three layers:

1. Target cadence
   - The per-surface target frame interval defined by the active render cadence contract.
   - This is the standard that implementation and benchmark work should aim to satisfy.
2. Diagnostic thresholds
   - Structured timing or warning thresholds emitted by the runtime.
   - These are useful for localizing regressions, but they do not by themselves prove cadence
     compliance.
3. Smoke gates
   - Coarse pass/fail thresholds used by automated GUI smoke.
   - These catch gross regressions and instability, but they are not a substitute for direct
     cadence-aware benchmarks or phase timing.

## Tracked scenarios

### Scenario 1: live overlay, HUD, and loupe interaction

Surface:
- live overlay redraw
- HUD movement
- loupe movement
- live cursor sample apply path

Primary metrics:
- effective active redraw cadence against the display-bound target frame budget
- phase timings for redraw-related work
- live sample cadence and apply latency against the fixed `120 Hz` sampling target

Diagnostic signals:
- `live_chrome.frame_tick_gap`
- `live_chrome.layer_render_duration`
- `live_chrome.layer_chrome_render_duration`
- `live_chrome.layer_chrome_render_gap`
- `live_chrome.active_layer_chrome_render_gap`
- `live_chrome.sample_refresh_gap`
- `Slow operation detected` entries for redraw-related operations

Current coarse smoke surface:
- `scripts/smoke/native-hud-follow-macos.sh`

### Scenario 2: render-heavy component paths

Surface:
- native host chrome, loupe, and toolbar paths that remain visible to users during live or frozen
  capture
- any future deterministic UI benchmark surfaces added for native-host rendering hot spots

Primary metrics:
- live frame timing and redraw stability for representative component render scenarios
- phase timings where benchmark design can isolate UI build, tessellation, upload, or command
  encoding work

Required measurement style:
- dedicated macOS smoke today, with deterministic component baselines added only when they measure
  the native-host path directly

Current coarse smoke surfaces:
- `scripts/smoke/native-hud-follow-macos.sh` for live HUD/loupe follow cadence
- `scripts/smoke/native-visual-contract-macos.sh` for live-to-frozen handoff, toolbar visibility
  and content draw, self-screenshot readiness, no pending-frame/no-scrim-drop flash, and Liquid
  Glass versus Classic Glass material contract

### Scenario 3: scroll-capture and image-processing hot paths

Surface:
- scroll stitching
- overlap or fingerprint matching
- image-helper hot paths used in scroll capture

Primary metrics:
- deterministic benchmark results on fixed fixtures
- instruction-count or stable wall-clock comparisons, depending on the selected benchmark surface

Required measurement style:
- non-GUI benchmark coverage that does not depend on desktop automation

### Scenario 4: active scroll-capture UI cadence

Surface:
- active scroll toolbar Liquid Glass backdrop sampling
- scroll minimap/preview updates during wheel input
- live-frame sampling used to feed visible scroll capture progress

Primary metrics:
- toolbar backdrop refresh gap against the display-bound target frame budget
- toolbar backdrop refresh duration for the main-thread scheduling path
- preview refresh cadence and preview export duration while committed scroll progress is changing
- source frame age for live region frames consumed during active scroll capture

Required behavior:
- toolbar backdrop sampling targets the display-bound presentation cadence while scroll capture is
  active and must not intentionally fall to seconds-apart updates
- scroll preview targets at least `30 Hz` while committed progress is changing, subject to only
  displaying proof-backed stitched state
- smooth-scroll and dense wheel-input paths should continue producing incremental proof-backed
  preview updates; if proof cannot keep up, Rsnap must fail closed for those frames instead of
  showing guessed progress or waiting to jump the preview at the end
- wheel input observed in the same target window but outside the selected viewport, for example in a
  right gutter or page margin, must still drive toolbar and stitch sampling for the selected
  viewport
- active scroll-capture UI sampling must use fresh native live frames in the steady state; repeated
  below-overlay screenshot capture is not an acceptable way to satisfy the cadence contract
- stale source frames must be rejected by frame age or sequence metadata instead of being displayed
  as current progress

### Scenario 5: screen-monitoring stream lifecycle

Surface:
- macOS live screen-monitoring streams used for live RGB/loupe sampling, Frozen handoff, scroll
  capture, and launch prewarm

Primary metrics:
- time from last active capture need to live/frozen stream release request
- time from release request to stream stopped telemetry, when the platform exposes it
- stream reuse/cancel events when a new capture starts during the release grace window

Required behavior:
- idle screen-monitoring streams are released after the configured `3s` grace window unless a new
  capture need cancels the pending release
- resource lifetime must be observable enough to distinguish intentional grace reuse from a leaked
  stream or a platform stop that is delayed after Rsnap has requested release
- stream lifecycle tuning must not regress first-capture Frozen toolbar latency or scroll-capture
  entry responsiveness

Current coarse smoke surface:
- `scripts/smoke/native-scroll-capture-macos.sh` for keyboard and toolbar entry, scroll growth,
  toolbar backdrop gap/duration metrics, and live desktop end-to-end regressions

## Execution environment classes

The performance contract distinguishes between environment classes because the artifact type
changes how evidence should be interpreted:

- Local deterministic benchmark
  - Component render benchmarks and scroll-capture hot-path benchmarks should be runnable on a
    normal development machine without requiring desktop automation.
  - These surfaces are the primary source for repeatable baseline comparisons.
- Dedicated desktop-session smoke
  - GUI smoke that drives a logged-in macOS desktop session is still required for end-to-end live
    overlay validation.
  - This evidence depends on Screen Recording, desktop automation, and a stable interactive
    session, so it should be treated as dedicated-host evidence rather than a generic shared-runner
    CI gate.

Passing one environment class does not automatically satisfy the other.

## Current runtime signals and their meaning

The native host and Rust product core expose several useful diagnostic signals:

- Native-host `live_chrome.pointer_event_gap` reports incoming pointer-event cadence while live
  capture is active.
- Native-host `live_chrome.input_summary` reports delivered mouse events and follow-path update
  counts for HUD/loupe smoke interpretation.
- Native-host `live_chrome.frame_tick_gap` reports the active live-frame clock interval. It should
  cluster around the display-bound target budget during live capture.
- Native-host `live_chrome.layer_render_duration` reports the in-overlay CALayer presentation path
  for live/frozen preview rendering when live chrome is not moved as separate windows.
- Native-host `live_chrome.layer_chrome_render_duration` reports the in-overlay HUD/loupe content
  refresh path after live chrome movement has been split from full overlay preview rendering.
- Native-host `live_chrome.layer_chrome_render_gap` reports the actual in-overlay HUD/loupe visual
  update cadence, including lightweight event-driven renders between frame-clock ticks.
- Native-host `live_chrome.active_layer_chrome_render_gap` reports the same visual cadence only
  while recent pointer input is active; HUD-follow smoke gates this metric so startup, Tab expand,
  and close transitions do not mask or invent moving-pointer regressions.
- Native-host `live_chrome.sample_refresh_gap` reports the color/loupe sampling feed cadence while
  live capture is active.
- Native-host `live_chrome.sample_refresh_gap` is gated against the fixed `120 Hz` sampling target,
  even when the active display target is `60 Hz`.
- Native-host `scroll_capture.toolbar_backdrop_refresh_gap` reports the active scroll toolbar
  backdrop scheduling cadence.
- Native-host `scroll_capture.toolbar_backdrop_refresh_duration` reports the main-thread cost of
  scheduling or applying active scroll toolbar backdrop updates. It must stay low because slow
  source capture belongs off the visible update path.
- Native-host `scroll_capture.toolbar_backdrop_changed_gap` reports the cadence of actual sampled
  toolbar backdrop image changes during scroll capture. The scroll smoke gates this separately from
  refresh scheduling so a path that calls the refresh loop but leaves Liquid Glass visually frozen is
  a regression.
- Native-host `capture.scroll_preview_refreshed` reports committed stitched-preview exports during
  scroll capture, including `exportMs` for the preview image generation step.
- Scroll preview refreshes must use the lightweight preview image path and stay below the smoke
  `MAX_SCROLL_PREVIEW_EXPORT_MS` gate. Preview generation must not clone or convert the full
  committed export image during active scroll.
- Native-host `capture.stream_release_scheduled`, `capture.stream_release_canceled`,
  `capture.stream_release_requested`, and `capture.stream_release_completed` report the macOS
  screen-monitoring release lifecycle and whether a new capture reused the pending release grace.

These values are useful for diagnosis, but they are not the full performance contract:

- `24 ms` render warnings are too coarse to prove compliance with the active target budget.
- a passing live HUD smoke run only shows that the selected path avoided severe regressions under
  the current harness thresholds.
- direct cadence-aware benchmarks and phase timing are still required for contract compliance.

## Current cadence implementation status

Implementation status should be judged against the cadence class for the surface being measured:

- visible interaction paths must target `min(active display maximum refresh rate, 120 Hz)`
- sampling paths must target fixed `120 Hz` while active, including stationary pointers over
  dynamic backgrounds
- movement and sampling work may be coalesced to the relevant cadence, but must not be
  intentionally downsampled below that cadence class

Remaining performance work should focus on measurement coverage, redraw localization, benchmark
baselines, and keeping display-bound presentation metrics separate from fixed-120Hz sampling
metrics before claiming contract compliance.

## Minimum artifact set for contract compliance

The Rsnap performance-tracking project should maintain all of the following:

- one normative spec for cadence, scenarios, metrics, and known gaps
- one or more direct benchmark surfaces for render-heavy components
- deterministic non-GUI benchmark coverage for scroll-capture hot paths
- structured runtime timing for native-host redraw localization and Rust product-core work
- coarse GUI smoke gates for gross regression detection

No single artifact type is sufficient on its own.
