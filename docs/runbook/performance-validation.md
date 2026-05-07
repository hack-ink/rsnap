# Performance Validation Runbook

Goal: Explain which repo-native command to run for deterministic replay, local performance
benchmarks, or the remaining dedicated macOS GUI smoke, and how to save or compare local
baselines without confusing non-live evidence with the final live acceptance gate.

Read this when: You are investigating a scroll-capture correctness or performance regression,
refreshing local benchmark baselines, or deciding whether a change needs deterministic replay,
deterministic benches, dedicated desktop smoke, or some combination of those surfaces.

Inputs: `scripts/smoke/`; `scripts/perf/`; `docs/spec/performance.md`;
`docs/runbook/scroll-capture-benchmarks.md`;
`docs/reference/smoke-perf-validation-surface.md`

Depends on: `docs/spec/performance.md`

Outputs: A clear command choice for the regression class you are testing, plus a repeatable local
baseline workflow for the committed Criterion benchmark targets.

Current release status: v0.1.3 hides user-facing scroll capture in the native host. The replay and
benchmark commands in this runbook still own retained internal scroll-capture engine validation and
future re-enablement work, but they are not evidence that the v0.1.3 toolbar exposes scroll
capture.

## Command selection

Use the smallest command that matches the regression surface:

- Scroll-capture correctness or stitching-behavior regressions before final live validation:
  `scripts/smoke/replay-scroll-capture.sh`
- Replay-harness sanity check when no user-recorded trace is available yet:
  `scripts/smoke/replay-scroll-capture-self-check.sh`
- Scroll-capture semantic trace analysis (first bad frame, under-consumption, overshoot):
  `scripts/smoke/analyze-scroll-capture-trace.sh`
- Scroll-capture or image-processing hot-path regressions:
  `cargo bench -p rsnap-overlay --bench scroll_capture -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.1`
- Native-host live chrome regressions:
  `scripts/smoke/native-hud-follow-macos.sh`
- Native-host click/drag selection, border leakage, mask stability, visual/material, or
  live-to-frozen handoff regressions:
  `scripts/smoke/native-visual-contract-macos.sh`
- General local deterministic performance sweep before or after a change:
  `scripts/perf/local.sh`
- Dedicated macOS environment validation without driving the real smoke scenario:
  `scripts/perf/self-check-macos.sh`
- Full macOS performance sweep on a logged-in desktop session:
  `scripts/perf/macos.sh`

`scripts/smoke/replay-scroll-capture.sh` and
`scripts/smoke/analyze-scroll-capture-trace.sh` force
`scroll_capture_replay --force-worker-pairwise`, so the repo-native non-live
entrypoints exercise the same replay mode that current macOS production uses.
Use `scripts/smoke/replay-scroll-capture-self-check.sh` for the matching
worker-pairwise self-check path when no recorded user trace is available.

## What each high-level task does

- `scripts/perf/local.sh`
  - Runs the committed scroll-capture Criterion benchmark target with the repo's smoke-sized
    sample settings.
  - Use this for routine local comparisons and for regressions that do not require a real desktop
    session.
- `scripts/perf/self-check-macos.sh`
  - Runs `scripts/perf/local.sh`, then runs the native HUD-follow self-check through
    `scripts/smoke/self-check-macos.sh`.
  - Use this to validate that the dedicated macOS environment, permissions, and smoke harness are
    ready without treating it as an end-to-end performance assertion.
- `scripts/perf/macos.sh`
  - Runs `scripts/perf/local.sh`, the dedicated native-host HUD-follow perf smoke, the core native
    visual contract smoke.
  - Use this only on a dedicated logged-in macOS desktop session with the expected Screen
    Recording and automation permissions.
- `scripts/smoke/macos.sh`
  - Runs the core native visual contract smoke.
  - Runs the native HUD-follow responsiveness smoke.

For a future downward scroll-capture re-enablement, the expected verification sequence is:

1. `cargo make checks`
2. `scripts/smoke/replay-scroll-capture.sh`
3. `scripts/smoke/analyze-scroll-capture-trace.sh`
4. any targeted deterministic `cargo test -p rsnap-overlay ...`
5. one fresh release live touchpad run with a newly recorded trace

For the current ownership map of those scripts versus replay, runtime, and
session tests, read `docs/reference/smoke-perf-validation-surface.md`.

## Baseline workflow for local benchmarks

`scripts/perf/local.sh` intentionally uses short, repeatable Criterion settings for routine
checks. When you need a named before/after comparison, use the direct benchmark commands so
Criterion can save or load a baseline:

```bash
cargo bench -p rsnap-overlay --bench scroll_capture -- --save-baseline local-scroll-capture
cargo bench -p rsnap-overlay --bench scroll_capture -- --baseline local-scroll-capture
```

Keep baseline comparisons on the same machine class and checkout whenever possible. Criterion keeps
baseline data under `target/criterion`.

## Environment expectations

Local deterministic benches:

- Do not require a desktop session.
- Do not require Screen Recording or UI automation permissions.
- Are the primary surface for repeatable scroll-capture and image-processing comparisons.

Dedicated macOS smoke:

- Requires a logged-in macOS desktop session.
- Requires the expected Screen Recording and automation permissions for the smoke scripts.
- Covers the native-host HUD-follow desktop path. The hard follow gate uses active pointer-movement
  cadence (`live_chrome.active_layer_chrome_render_gap`) and frame-tick cadence
  (`live_chrome.frame_tick_gap`) rather than startup, Tab-expand, or close transition gaps.
- Requires the smoke harness to deliver enough mouse-movement input. For the default smooth event
  path, `native-hud-follow-macos.sh` expects at least half of the requested
  `(PATH_DURATION_MS / 1000) * PATH_RATE_HZ` event count; override with `MIN_MOUSE_EVENTS` only
  when validating a different input driver or intentionally degraded environment.
- Interpret cadence metrics by class:
  - display-bound visual presentation metrics are gated against
    `min(active display maximum refresh rate, 120 Hz)`, so a `60 Hz` monitor has a `16.67 ms`
    visible-frame budget.
  - sampling metrics such as `live_chrome.sample_refresh_gap` are gated against fixed `120 Hz`
    / `8.33 ms` while the live HUD/loupe is active, even on a `60 Hz` monitor.
  - live-to-frozen handoff timing is a one-shot latency path: use `snapshotSource`,
    `snapshotWaitMs`, `presentMs`, and `frozen_first_display_handoff` together, and do not treat a
    delayed post-latch ScreenCaptureKit frame as permission to delay the toolbar.
- Scroll-capture correctness is now exercised by deterministic replay.
- Is meant for dedicated-host or manual validation, not a flaky shared-runner PR gate.

## Interpreting failures

- direct benchmark regressions from the scroll-capture target:
  compare scenario-level numbers against your saved baseline and inspect the relevant benchmark
  group before escalating to GUI smoke.
- `scripts/smoke/replay-scroll-capture.sh` failures:
  treat them as authoritative regressions against the latest recorded live trace in shipping
  worker-pairwise overlay or session logic before attempting more desktop-session repro. If the
  command reports that no trace manifests were found, that is an operator/setup failure: record a
  fresh live trace first or rerun the example with `--trace <manifest-path>`.
- `scripts/smoke/replay-scroll-capture-self-check.sh` failures:
  treat them as deterministic regressions in the replay harness itself, not as evidence about the
  latest user-recorded live trace.
- clean replay plus trace analysis:
  this is necessary but not sufficient for XY-185 style sign-off; the remaining risk is
  isolated to the final fresh live touchpad run, not eliminated.
- `scripts/perf/self-check-macos.sh` failures:
  treat these first as environment or permission readiness failures unless local benches also
  regressed.
- `scripts/perf/macos.sh` failures with healthy local benches:
  suspect live overlay cadence, desktop-session conditions, or smoke-harness environment drift.
- `scripts/smoke/native-visual-contract-macos.sh` failures:
  treat them as native-host visual or behavior contract regressions. The smoke runs Liquid Glass and
  Classic Glass cases, screenshots Rsnap's own frozen overlay, and verifies frozen handoff timing,
  toolbar visibility, Liquid toolbar content draw, material selection, and that live-to-frozen did
  not display a pending half-frame before the complete frozen UI. It also samples outside the
  selection during release so a disappearing/reappearing scrim fails even if the final screenshot
  looks correct. The Liquid path keeps the stricter handoff threshold; the Classic path allows a
  wider first-frame budget because blur/material work can be heavier while still catching material
  disappearance and gross handoff regressions. The Classic blur may settle immediately after the
  first visible frozen toolbar frame so toolbar availability stays responsive.
  If toolbar visibility is correct but release feels delayed, compare `snapshotWaitMs` and
  `presentMs`: `snapshotWaitMs` indicates frame-source waiting, while `presentMs` indicates
  synchronous frozen-frame/material installation before the first visible frozen frame.

## Related docs

- `docs/reference/smoke-perf-validation-surface.md` for the smoke/perf ownership map and cleanup
  boundaries.
- `docs/runbook/scroll-capture-benchmarks.md` for the scroll-capture fixture contract and
  per-target baseline commands.
- `docs/reference/live-sampling.md` for the stream-first live cursor and loupe path that
  the dedicated macOS smoke validates.
