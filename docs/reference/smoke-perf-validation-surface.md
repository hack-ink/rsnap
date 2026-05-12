# Smoke/Perf Validation Surface Reference

Purpose: Describe the current smoke/perf validation surface by ownership layer so cleanup work can
distinguish authoritative behavior coverage from thin entrypoint wrappers.

Read this when: You are deciding whether a smoke/perf asset is redundant, choosing where a new
validation case belongs, or auditing whether a script, benchmark, replay test, or runtime test is
the owner for a behavior.

Sources: `scripts/smoke/`; `scripts/perf/`; `packages/rsnap-overlay/src/scroll_capture/tests.rs`;
`packages/rsnap-overlay/src/overlay/tests/worker_tick_runtime.rs`;
`packages/rsnap-overlay/src/overlay/tests/worker_observation_runtime.rs`;
`packages/rsnap-overlay/src/overlay/replay_support.rs`; `packages/rsnap-overlay/benches/scroll_capture.rs`

Depends on: `docs/runbook/performance-validation.md`; `docs/spec/performance.md`

Covers: The current layer map for smoke/perf entrypoints, deterministic replay/bench surfaces,
overlay runtime integration tests, and scroll-capture session semantics tests.

Release exposure note: v0.2.2 exposes user-facing Scroll Capture for dragged-region Frozen captures
in the native host. The scroll-capture entries in this reference remain the retained validation
assets and recovery surfaces; follow `docs/runbook/scroll-capture-recovery-plan.md` before making a
release-scope readiness claim for broader target apps.

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
| `scripts/smoke/native-hud-follow-macos.sh` | Script entrypoint | live macOS perf smoke | HUD/loupe follow-cadence smoke for performance work, including delivered mouse-event count, sample refresh cadence, active-layer chrome cadence, and frame-tick cadence. |
| `scripts/smoke/native-visual-contract-macos.sh` | Script entrypoint | live macOS smoke | Core native-host behavior contract: repeated real click freezes, repeated held drag freezes, in-drag and frozen screenshots, click/drag editability, border-leak, scrim, and handoff telemetry gates. |
| `scripts/smoke/native-scroll-capture-macos.sh` | Script entrypoint | live macOS scroll smoke | Real frozen-region scroll-capture smoke on a deterministic scrollable native window; asserts the session is unlocked, ScreenCaptureKit exposes a display, drag freeze occurs, Scroll Capture starts in `manual_universal` mode, overlay-local wheel forwarding is the input path, real wheel input moves the target through short all-overlay passthrough windows, selected-region frames sample through the live stream or below-overlay fallback, no legacy auto-scroll telemetry appears, and multiple committed growth events append before copy/export. The default driver is `SCROLL_DRIVER=wheel`; `SCROLL_DRIVER=notification` is retained for direct background-control diagnosis. |
| `scripts/smoke/self-check-macos.sh` | Script entrypoint | smoke readiness | Verifies native HUD-follow smoke tooling readiness without the real GUI run. |
| `scripts/smoke/macos.sh` | Script entrypoint | smoke aggregation | Runs the core native visual contract and HUD-follow responsiveness smoke. |
| `scripts/perf/local.sh` | Script entrypoint | deterministic benches | Runs the committed Criterion smoke-sized benchmark sweep. |
| `scripts/perf/self-check-macos.sh` | Script entrypoint | perf aggregation | Runs local deterministic benches plus macOS smoke readiness. |
| `scripts/perf/macos.sh` | Script entrypoint | perf aggregation | Runs local deterministic benches, the HUD-follow perf smoke, and the core native visual contract. |
| `packages/rsnap-overlay/src/overlay/replay_support.rs` tests | Deterministic replay / bench | replay harness | Trace round-trip, replay mode selection, and summary classification. |
| `packages/rsnap-overlay/benches/scroll_capture.rs` | Deterministic replay / bench | hot-path perf | Stable fingerprint, overlap-match, and one-step session commit baselines. |
| `packages/rsnap-overlay/src/overlay/tests/worker_tick_runtime.rs` | Overlay runtime integration | overlay runtime | Request issuance, retry timing, backoff, and fresh-input worker scheduling. |
| `packages/rsnap-overlay/src/overlay/tests/worker_observation_runtime.rs` | Overlay runtime integration | overlay runtime | Latched, stale, and superseded worker observation context handling. |
| `packages/rsnap-overlay/src/scroll_capture/tests.rs` | Scroll-capture session semantics | scroll session | Downward stitching, overlap resolution, and worker-pairwise session semantics. |

## Cleanup rules

- Prefer deleting or merging a script only when another remaining script still provides the same
  stable entrypoint value.
- Keep native-host smoke entrypoints on `scripts/build_and_run.sh`; raw `swift build` is not a
  substitute because the Swift executable must be relinked after Rust `rsnap-host-ffi` changes.
- Prefer deleting or merging a runtime test only when its remaining assertions are already owned by
  `scroll_capture/tests.rs` and it no longer proves runtime state-machine behavior.
- Prefer extracting shared fixtures when two layers intentionally cover different behavior with the
  same synthetic frame families.
- Keep replay self-check aligned with the worker-pairwise production path; treat recorded-source
  replay as a secondary internal comparison surface.
