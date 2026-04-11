# Performance Validation Runbook

Goal: Explain which repo-native command to run for deterministic replay, local performance
benchmarks, or the remaining dedicated macOS GUI smoke, and how to save or compare local
baselines without confusing non-live evidence with the final live acceptance gate.

Read this when: You are investigating a scroll-capture correctness or performance regression,
refreshing local benchmark baselines, or deciding whether a change needs deterministic replay,
deterministic benches, dedicated desktop smoke, or some combination of those surfaces.

Inputs: `Makefile.toml`; `docs/spec/performance.md`; `docs/runbook/scroll-capture-benchmarks.md`

Depends on: `docs/spec/performance.md`

Outputs: A clear command choice for the regression class you are testing, plus a repeatable local
baseline workflow for the committed Criterion benchmark targets.

## Command selection

Use the smallest command that matches the regression surface:

- Scroll-capture correctness or stitching-behavior regressions before final live validation:
  `cargo make replay-scroll-capture`
- Replay-harness sanity check when no user-recorded trace is available yet:
  `cargo make replay-scroll-capture-self-check`
- Scroll-capture semantic trace analysis (first bad frame, under-consumption, overshoot):
  `cargo make analyze-scroll-capture-trace`
- Component render regressions in egui-heavy UI such as the settings window:
  `cargo make perf-bench-settings-window`
- Scroll-capture or image-processing hot-path regressions:
  `cargo make perf-bench-scroll-capture`
- General local deterministic performance sweep before or after a change:
  `cargo make perf-local`
- Dedicated macOS environment validation without driving the real smoke scenario:
  `cargo make perf-self-check-macos`
- Dedicated macOS end-to-end GUI performance smoke on a logged-in desktop session:
  `cargo make perf-macos`

`cargo make replay-scroll-capture` and `cargo make analyze-scroll-capture-trace`
force `scroll_capture_replay --force-worker-pairwise`, so the repo-native
non-live entrypoints exercise the same worker screenshot + pairwise
registration commit path that current macOS production uses. Invoke the example
directly without that flag only when you intentionally want to compare the
legacy recorded-source replay mode.

## What each high-level task does

- `perf-local`
  - Runs both committed Criterion benchmark targets with the repo's smoke-sized sample settings.
  - Use this for routine local comparisons and for regressions that do not require a real desktop
    session.
- `perf-self-check-macos`
  - Runs `perf-local`, then runs the live-loupe self-check plus recorded-live-trace
    scroll-capture replay.
  - Use this to validate that the dedicated macOS environment, permissions, and smoke harness are
    ready without treating it as an end-to-end performance assertion.
- `perf-macos`
  - Runs `perf-local`, then runs the real live-loupe GUI smoke task plus recorded-live-trace
    scroll-capture replay.
  - Use this only on a dedicated logged-in macOS desktop session with the expected Screen
    Recording and automation permissions.

For the downward scroll-capture rebuild, the expected verification sequence is:

1. `cargo make checks`
2. `cargo make replay-scroll-capture`
3. `cargo make analyze-scroll-capture-trace`
4. any targeted deterministic `cargo test -p rsnap-overlay ...`
5. one fresh release live touchpad run with a newly recorded trace

The low-level deterministic and smoke tasks remain available:

- `cargo make replay-scroll-capture`
- `cargo make replay-scroll-capture-self-check`
- `cargo make analyze-scroll-capture-trace`
- `cargo make smoke-live-loupe-perf-macos`
- `cargo make smoke-self-check-macos`
- `cargo make smoke-macos`

Use them when you need to isolate deterministic scroll-capture replay or the live-loupe smoke
harness instead of the high-level performance entrypoint.

## Baseline workflow for local benchmarks

The cargo-make tasks intentionally use short, repeatable Criterion settings for routine checks.
When you need a named before/after comparison, use the direct benchmark commands so Criterion can
save or load a baseline:

```bash
cargo bench -p rsnap --bench settings_window -- --save-baseline local-settings-ui
cargo bench -p rsnap --bench settings_window -- --baseline local-settings-ui

cargo bench -p rsnap-overlay --bench scroll_capture -- --save-baseline local-scroll-capture
cargo bench -p rsnap-overlay --bench scroll_capture -- --baseline local-scroll-capture
```

Keep baseline comparisons on the same machine class and checkout whenever possible. Criterion keeps
baseline data under `target/criterion`.

## Environment expectations

Local deterministic benches:

- Do not require a desktop session.
- Do not require Screen Recording or UI automation permissions.
- Are the primary surface for repeatable component-render and scroll-capture comparisons.

Dedicated macOS smoke:

- Requires a logged-in macOS desktop session.
- Requires the expected Screen Recording and automation permissions for the smoke scripts.
- Only covers the remaining live-loupe desktop path; scroll-capture correctness is now exercised by
  deterministic replay.
- Is meant for dedicated-host or manual validation, not a flaky shared-runner PR gate.

## Interpreting failures

- `perf-bench-settings-window` or `perf-bench-scroll-capture` regressions:
  compare scenario-level numbers against your saved baseline and inspect the relevant benchmark
  group before escalating to GUI smoke.
- `replay-scroll-capture` failures:
  treat them as authoritative regressions against the latest recorded live trace in shipping
  worker-pairwise overlay or session logic before attempting more desktop-session repro. If the
  command reports that no trace manifests were found, that is an operator/setup failure: record a
  fresh live trace first or rerun the example with `--trace <manifest-path>`.
- `replay-scroll-capture-self-check` failures:
  treat them as deterministic regressions in the replay harness itself, not as evidence about the
  latest user-recorded live trace.
- clean replay plus trace analysis:
  this is necessary but not sufficient for XY-185 style sign-off; the remaining risk is
  isolated to the final fresh live touchpad run, not eliminated.
- `perf-self-check-macos` failures:
  treat these first as environment or permission readiness failures unless local benches also
  regressed.
- `perf-macos` failures with healthy local benches:
  suspect live overlay cadence, desktop-session conditions, or smoke-harness environment drift.

## Related docs

- `docs/runbook/scroll-capture-benchmarks.md` for the scroll-capture fixture contract and
  per-target baseline commands.
- `docs/reference/live-sampling.md` for the stream-first live cursor and loupe path that
  the dedicated macOS smoke validates.
