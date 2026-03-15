# Performance Checks Guide

Goal: Explain which repo-native performance command to run, when to use local deterministic
benchmarks versus dedicated macOS GUI smoke, and how to save or compare local baselines.

Read this when: You are investigating a performance regression, refreshing local benchmark
baselines, or deciding whether a change needs deterministic benches, dedicated desktop smoke, or
both.

Inputs: `Makefile.toml`; `docs/spec/performance_tracking.md`; `docs/guide/scroll-capture-benchmarks.md`

Depends on: `docs/spec/performance_tracking.md`

Outputs: A clear command choice for the regression class you are testing, plus a repeatable local
baseline workflow for the committed Criterion benchmark targets.

## Command selection

Use the smallest command that matches the regression surface:

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

## What each high-level task does

- `perf-local`
  - Runs both committed Criterion benchmark targets with the repo's smoke-sized sample settings.
  - Use this for routine local comparisons and for regressions that do not require a real desktop
    session.
- `perf-self-check-macos`
  - Runs `perf-local`, then runs the existing macOS smoke scripts in `--self-check` mode.
  - Use this to validate that the dedicated macOS environment, permissions, and smoke harness are
    ready without treating it as an end-to-end performance assertion.
- `perf-macos`
  - Runs `perf-local`, then runs the real macOS GUI smoke tasks.
  - Use this only on a dedicated logged-in macOS desktop session with the expected Screen
    Recording and automation permissions.

The low-level smoke tasks remain available:

- `cargo make smoke-live-loupe-perf-macos`
- `cargo make smoke-scroll-capture-macos`
- `cargo make smoke-self-check-macos`
- `cargo make smoke-macos`

Use them when you need to isolate one smoke harness instead of the high-level performance entrypoint.

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
- Is meant for dedicated-host or manual validation, not a flaky shared-runner PR gate.

## Interpreting failures

- `perf-bench-settings-window` or `perf-bench-scroll-capture` regressions:
  compare scenario-level numbers against your saved baseline and inspect the relevant benchmark
  group before escalating to GUI smoke.
- `perf-self-check-macos` failures:
  treat these first as environment or permission readiness failures unless local benches also
  regressed.
- `perf-macos` failures with healthy local benches:
  suspect live overlay cadence, desktop-session conditions, or smoke-harness environment drift.

## Related guides

- `docs/guide/scroll-capture-benchmarks.md` for the scroll-capture fixture contract and per-target
  baseline commands.
- `docs/guide/live-sampling-streams.md` for the stream-first live cursor and loupe path that the
  dedicated macOS smoke validates.
