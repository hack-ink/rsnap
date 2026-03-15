# rsnap Performance Tracking Contract

Purpose: Define the normative performance-tracking contract for rsnap so render cadence,
measurement surfaces, thresholds, and known contract gaps are explicit and shared across code,
benchmarks, smoke harnesses, and tracker issues.

Status: normative

Read this when: You are implementing, reviewing, or validating render scheduling, overlay redraw,
live/loupe responsiveness, component render benchmarks, or scroll-capture performance tracking.

Not this document: Step-by-step benchmark procedures, smoke-run instructions, or saved execution
plans. Use `docs/guide/` for procedures and `docs/plans/` only for tool-managed plan artifacts.

Primary procedures:
- `docs/guide/performance-checks.md` for repo-native performance command entrypoints
- `docs/guide/scroll-capture-benchmarks.md` for scroll-capture benchmark fixtures and baseline use

Defines:
- the active render cadence contract for rsnap UI and overlay paths
- the tracked performance scenarios and their primary metrics
- the distinction between target cadence, diagnostic thresholds, and coarse smoke gates
- the currently known gap between the agreed cadence contract and the live implementation

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

`min(120 Hz, active display refresh ceiling)`

Practical meaning:

- If the active display refresh rate is `<= 120 Hz`, rsnap should not run below that display
  refresh rate during the relevant active interaction path.
- If the active display refresh rate is `> 120 Hz`, the current contract caps the target at
  `120 Hz`.

Derived target frame budgets:

| Active display refresh ceiling | Target cadence | Target frame budget |
| --- | --- | --- |
| `60 Hz` | `60 Hz` | `16.67 ms` |
| `75 Hz` | `75 Hz` | `13.33 ms` |
| `90 Hz` | `90 Hz` | `11.11 ms` |
| `120 Hz` | `120 Hz` | `8.33 ms` |
| `144 Hz` | `120 Hz` | `8.33 ms` |
| `240 Hz` | `120 Hz` | `8.33 ms` |

This cadence contract is normative even when current logs or smoke harnesses use coarser warning
thresholds.

## Measurement model

The performance contract distinguishes three layers:

1. Target cadence
   - The per-surface target frame interval derived from the active render cadence contract.
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
- effective active redraw cadence against the target frame budget for the active display
- phase timings for redraw-related work
- live sample apply latency

Diagnostic signals:
- `overlay.window_renderer_acquire_frame`
- `overlay.event_loop_stall`
- `overlay.live_sample_apply_latency`
- `Slow operation detected` entries for redraw-related operations

Current coarse smoke surface:
- `scripts/live-loupe-perf-smoke-macos.sh`

### Scenario 2: render-heavy component paths

Surface:
- settings window render path
- other egui-heavy component paths selected by benchmark value

Primary metrics:
- benchmark time for representative component render scenarios
- phase timings where benchmark design can isolate UI build, tessellation, upload, or command
  encoding work

Required measurement style:
- repeatable local benchmark runs with saved baselines

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

- `LIVE_PRESENT_INTERVAL_MIN = 8.33 ms` for the 120 Hz floor-derived present interval.
- `SLOW_OP_WARN_RENDER = 24 ms` for coarse render warnings.
- `OVERLAY_EVENT_LOOP_STALL_THRESHOLD = 250 ms` for severe event-loop stalls.
- `overlay.live_sample_apply_latency` is logged once latency reaches `12 ms`.

These values are useful for diagnosis, but they are not the full performance contract:

- `24 ms` render warnings are too coarse to prove compliance with `8.33 ms` or `16.67 ms`
  target budgets.
- a passing live-loupe smoke run only shows that the path avoided severe regressions under the
  current harness thresholds.
- direct cadence-aware benchmarks and phase timing are still required for contract compliance.

## Current cadence implementation status

Overlay repaint interval derivation now applies the cadence contract directly:

- known refresh rates below `120 Hz` use the known display ceiling
- known refresh rates above `120 Hz` are capped at `120 Hz`
- unknown refresh rates fall back to the contract cap of `120 Hz`

Remaining performance work should treat cadence derivation as aligned and focus on measurement
coverage, redraw localization, and benchmark baselines rather than re-arguing the core cap logic.

## Minimum artifact set for contract compliance

The rsnap performance-tracking project should maintain all of the following:

- one normative spec for cadence, scenarios, metrics, and known gaps
- one or more direct benchmark surfaces for render-heavy components
- deterministic non-GUI benchmark coverage for scroll-capture hot paths
- structured runtime timing for overlay redraw localization
- coarse GUI smoke gates for gross regression detection

No single artifact type is sufficient on its own.
