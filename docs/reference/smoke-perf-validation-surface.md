# Smoke/Perf Validation Surface Reference

Purpose: Describe the current smoke/perf validation surface by ownership layer so cleanup work can
distinguish authoritative behavior coverage from thin entrypoint wrappers.

Read this when: You are deciding whether a smoke/perf asset is redundant, choosing where a new
validation case belongs, or auditing whether a script, benchmark, replay test, or runtime test is
the owner for a behavior.

Sources: `scripts/smoke/`; `scripts/perf/`; `packages/rsnap-overlay/src/scroll_capture/tests.rs`;
`packages/rsnap-overlay/src/overlay/tests/worker_tick_runtime.rs`;
`packages/rsnap-overlay/src/overlay/tests/worker_observation_runtime.rs`;
`packages/rsnap-overlay/src/overlay/replay_support.rs`; `apps/rsnap/benches/settings_window.rs`;
`packages/rsnap-overlay/benches/scroll_capture.rs`

Depends on: `docs/runbook/performance-validation.md`; `docs/spec/performance.md`

Covers: The current layer map for smoke/perf entrypoints, deterministic replay/bench surfaces,
overlay runtime integration tests, and scroll-capture session semantics tests.

## Layer definitions

| Layer | Owns | Typical artifact |
| --- | --- | --- |
| Script entrypoint | Human/agent command routing and stable command names | `scripts/smoke/*.sh`, `scripts/perf/*.sh` |
| Deterministic replay / bench | Recorded-trace replay and repeatable benchmark evidence | replay example/tests, Criterion benches |
| Overlay runtime integration | Worker scheduling, retries, stale-input handling, request issuance | `overlay/tests/worker_*_runtime.rs` |
| Scroll-capture session semantics | Stitching, overlap, pairwise commit/no-change semantics | `scroll_capture/tests.rs` |

## Coverage matrix

| Asset | Layer | Primary owner | What it should prove |
| --- | --- | --- | --- |
| `scripts/smoke/replay-scroll-capture.sh` | Script entrypoint | deterministic replay | Runs the latest recorded trace through worker-pairwise replay. |
| `scripts/smoke/replay-scroll-capture-self-check.sh` | Script entrypoint | deterministic replay | Runs the worker-pairwise replay self-check without a user trace. |
| `scripts/smoke/analyze-scroll-capture-trace.sh` | Script entrypoint | deterministic replay | Emits summary-only replay analysis for semantic drift triage. |
| `scripts/smoke/live-loupe-perf-macos.sh` | Script entrypoint | live macOS smoke | Drives the remaining live-loupe desktop smoke path. |
| `scripts/smoke/live-loupe-perf-self-check-macos.sh` | Script entrypoint | smoke readiness | Runs the live-loupe macOS environment/tooling self-check directly. |
| `scripts/smoke/self-check-macos.sh` | Script entrypoint | smoke readiness | Verifies macOS smoke tooling and replay self-check without the real GUI run. |
| `scripts/smoke/macos.sh` | Script entrypoint | smoke aggregation | Runs the macOS live-loupe smoke plus recorded-trace replay. |
| `scripts/perf/local.sh` | Script entrypoint | deterministic benches | Runs the committed Criterion smoke-sized benchmark sweep. |
| `scripts/perf/self-check-macos.sh` | Script entrypoint | perf aggregation | Runs local deterministic benches plus macOS smoke readiness. |
| `scripts/perf/macos.sh` | Script entrypoint | perf aggregation | Runs local deterministic benches plus the real macOS smoke path. |
| `packages/rsnap-overlay/src/overlay/replay_support.rs` tests | Deterministic replay / bench | replay harness | Trace round-trip, replay mode selection, and summary classification. |
| `apps/rsnap/benches/settings_window.rs` | Deterministic replay / bench | component perf | Stable settings-window layout/frame baselines. |
| `packages/rsnap-overlay/benches/scroll_capture.rs` | Deterministic replay / bench | hot-path perf | Stable fingerprint, overlap-match, and one-step session commit baselines. |
| `packages/rsnap-overlay/src/overlay/tests/worker_tick_runtime.rs` | Overlay runtime integration | overlay runtime | Request issuance, retry timing, backoff, and fresh-input worker scheduling. |
| `packages/rsnap-overlay/src/overlay/tests/worker_observation_runtime.rs` | Overlay runtime integration | overlay runtime | Latched, stale, and superseded worker observation context handling. |
| `packages/rsnap-overlay/src/scroll_capture/tests.rs` | Scroll-capture session semantics | scroll session | Downward stitching, overlap resolution, and worker-pairwise session semantics. |

## Cleanup rules

- Prefer deleting or merging a script only when another remaining script still provides the same
  stable entrypoint value.
- Prefer deleting or merging a runtime test only when its remaining assertions are already owned by
  `scroll_capture/tests.rs` and it no longer proves runtime state-machine behavior.
- Prefer extracting shared fixtures when two layers intentionally cover different behavior with the
  same synthetic frame families.
- Keep replay self-check aligned with the worker-pairwise production path; treat recorded-source
  replay as a secondary internal comparison surface.
