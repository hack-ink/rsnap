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

## Active render cadence contract

For actively rendered rsnap UI and overlay paths, target cadence is:

`120 Hz`

Practical meaning:

- The target frame budget is always `8.33 ms` for the relevant active interaction path.
- rsnap MUST NOT lower this target by reading the active display refresh rate, deriving a
  per-monitor display refresh ceiling, or treating an unknown display refresh rate as a separate
  acceptance class.
- Passing the cadence contract requires achieving the fixed `120 Hz` target. A lower display
  refresh rate is not an alternate target and does not relax the requirement.

Target frame budget:

| Target cadence | Target frame budget |
| --- | --- |
| `120 Hz` | `8.33 ms` |

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
- effective active redraw cadence against the fixed `120 Hz` target frame budget
- phase timings for redraw-related work
- live sample apply latency

Diagnostic signals:
- `overlay.window_renderer_acquire_frame`
- `overlay.event_loop_stall`
- `overlay.live_sample_apply_latency`
- `Slow operation detected` entries for redraw-related operations

Current coarse smoke surface:
- `scripts/smoke/live-loupe-perf-macos.sh`

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

- `LIVE_PRESENT_INTERVAL_MIN = 8.33 ms` for the fixed `120 Hz` target present interval.
- `SLOW_OP_WARN_RENDER = 24 ms` for coarse render warnings.
- `OVERLAY_EVENT_LOOP_STALL_THRESHOLD = 250 ms` for severe event-loop stalls.
- `overlay.live_sample_apply_latency` is logged once latency reaches `12 ms`.
- Native-host `live_chrome.hud.apply_latency` and `live_chrome.loupe.apply_latency` measure the
  first successful visible apply for a live input sequence.
- Native-host `live_chrome.hud.window_update_duration`,
  `live_chrome.loupe.window_update_duration`, and `live_chrome.update_duration` report the external
  live chrome window update path used by Liquid Glass HUD/loupe presentation.
- Native-host `live_chrome.frame_tick_gap` reports the active live-frame clock interval. It should
  cluster around the fixed `120 Hz` budget (`8.33 ms`) during active live capture.
- Native-host `live_chrome.layer_render_duration` reports the in-overlay CALayer presentation path
  for live/frozen preview rendering when live chrome is not moved as separate windows.
- Native-host `live_chrome.layer_chrome_render_duration` reports the in-overlay HUD/loupe content
  refresh path after live chrome movement has been split from full overlay preview rendering.
- The live chrome follow clock may use a small scheduling headroom below `8.33 ms`, but acceptance
  is still judged against the fixed `120 Hz` frame budget.

These values are useful for diagnosis, but they are not the full performance contract:

- `24 ms` render warnings are too coarse to prove compliance with the `8.33 ms` target budget.
- a passing live-loupe smoke run only shows that the path avoided severe regressions under the
  current harness thresholds.
- direct cadence-aware benchmarks and phase timing are still required for contract compliance.

## Current cadence implementation status

The current contract no longer treats display refresh-rate discovery as part of cadence
derivation or acceptance. Implementation status should be judged against the fixed target:

- active interaction paths must target `120 Hz` directly
- cadence derivation must not query or branch on the active display refresh rate
- any implementation path that still lowers cadence from a known monitor refresh rate remains a
  contract gap until removed

Remaining performance work should focus on measurement coverage, redraw localization, benchmark
baselines, and removing any refresh-rate-derived cadence paths before claiming contract
compliance.

## Minimum artifact set for contract compliance

The rsnap performance-tracking project should maintain all of the following:

- one normative spec for cadence, scenarios, metrics, and known gaps
- one or more direct benchmark surfaces for render-heavy components
- deterministic non-GUI benchmark coverage for scroll-capture hot paths
- structured runtime timing for overlay redraw localization
- coarse GUI smoke gates for gross regression detection

No single artifact type is sufficient on its own.
