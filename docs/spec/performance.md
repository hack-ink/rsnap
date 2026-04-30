# rsnap Performance Contract

Purpose: Define the normative performance-tracking contract for rsnap so render cadence,
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
- the active render cadence contract for rsnap UI and overlay paths
- the tracked performance scenarios and their primary metrics
- the distinction between target cadence, diagnostic thresholds, and coarse smoke gates
- the current cadence implementation status and any known gap against the agreed contract

## Scope

This contract applies to performance tracking for actively rendered rsnap surfaces:

- live overlay redraw
- HUD and loupe movement while live
- frozen-mode floating UI that redraws in response to active interaction
- render-heavy component paths that should be benchmarked against the same frame-budget family
- scroll-capture hot paths that need deterministic benchmark coverage even when they are not
  driven by live GUI cadence

This contract does not require idle surfaces to redraw continuously when there is no interaction
or animation.

## Cadence classes

rsnap tracks two different cadence classes. They must not be collapsed into one number during
implementation, telemetry review, or smoke interpretation.

### Display-bound presentation cadence

Visible presentation is constrained by the active display. For actively rendered UI and overlay
paths that the user sees on screen, target cadence is:

`min(active display maximum refresh rate, 120 Hz)`

Practical meaning:

- On a `120 Hz` or faster display, the target frame budget is `8.33 ms`.
- On a `60 Hz` display, the target frame budget is `16.67 ms`.
- If the display refresh rate is unavailable, rsnap uses the conservative `60 Hz` fallback.
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
release handoff must avoid waiting for a future ScreenCaptureKit frame when an already-warm frame
is available. The intended path is:

1. use an already-warm frozen-authority or live-sampler frame immediately,
2. present the frozen frame, toolbar, and scrim in one continuous handoff,
3. perform cleanup such as secondary-window collapse after the first frozen frame is installed.

The handoff must not show a pending half-frame, remove/re-add the outside-selection scrim, or
delay toolbar visibility on a static desktop just because ScreenCaptureKit has not emitted a new
post-latch frame.

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
- `overlay.window_renderer_acquire_frame`
- `overlay.event_loop_stall`
- `overlay.live_sample_apply_latency`
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

The current overlay runtime already exposes several useful diagnostic thresholds:

- `LIVE_PRESENT_INTERVAL_MIN = 8.33 ms` for the maximum `120 Hz` target present interval.
- `SLOW_OP_WARN_RENDER = 24 ms` for coarse render warnings.
- `OVERLAY_EVENT_LOOP_STALL_THRESHOLD = 250 ms` for severe event-loop stalls.
- `overlay.live_sample_apply_latency` is logged once latency reaches `12 ms`.
- Native-host `live_chrome.hud.apply_latency`, `live_chrome.loupe.apply_latency`,
  `live_chrome.hud.window_update_duration`, `live_chrome.loupe.window_update_duration`, and
  `live_chrome.update_duration` report external frozen-toolbar/window update paths when those
  paths are active; live HUD/loupe position should not depend on moving external windows.
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

The rsnap performance-tracking project should maintain all of the following:

- one normative spec for cadence, scenarios, metrics, and known gaps
- one or more direct benchmark surfaces for render-heavy components
- deterministic non-GUI benchmark coverage for scroll-capture hot paths
- structured runtime timing for overlay redraw localization
- coarse GUI smoke gates for gross regression detection

No single artifact type is sufficient on its own.
