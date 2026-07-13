---
title: "Smoke/Perf Validation Surface Reference"
description: "Smoke/Perf Validation Surface Reference documentation for Rsnap."
type: "Reference"
status: active
authority: normative
owner: acgxv/rsnap
last_verified: 2026-07-06
---
# Smoke/Perf Validation Surface Reference

Purpose: Describe the current smoke/perf validation surface by ownership layer so cleanup work can
distinguish authoritative behavior coverage from thin entrypoint wrappers.

Read this when: You are deciding whether a smoke/perf asset is redundant, choosing where a new
validation case belongs, or auditing whether a script, benchmark, native smoke, or session test is
the owner for a behavior.

Sources: `scripts/smoke/`; `scripts/perf/`;
`packages/rsnap-capture-core/src/scroll_capture/tests.rs`;
`packages/rsnap-capture-core/benches/scroll_capture.rs`; `apps/rsnap-perf/`

Depends on: `docs/runbook/performance-validation.md`; `docs/spec/performance.md`

Covers: The current layer map for smoke/perf entrypoints, deterministic bench surfaces, native
macOS smoke surfaces, and scroll-capture session semantics tests.

Release exposure note: v0.2.5 exposes user-facing Scroll Capture for dragged-region Frozen captures
in the native host. The scroll-capture entries in this reference remain retained validation assets
and recovery surfaces; follow `docs/runbook/scroll-capture-recovery-plan.md` before making a
release-scope readiness claim for broader target apps.

## Layer definitions

| Layer | Owns | Typical artifact |
| --- | --- | --- |
| Script entrypoint | Human/agent command routing and stable command names | `scripts/smoke/*.sh`, `scripts/perf/*.sh` |
| Deterministic bench | Repeatable non-GUI performance evidence | `apps/rsnap-perf/`, Criterion benches |
| Native macOS smoke | Live desktop behavior that depends on AppKit, ScreenCaptureKit, OS input, or permissions | `scripts/smoke/native-*-macos.sh` |
| Scroll-capture session semantics | Stitching, overlap, pairwise commit/no-change semantics | `scroll_capture/tests.rs` |

## Coverage matrix

| Asset | Layer | Primary owner | What it should prove |
| --- | --- | --- | --- |
| `scripts/smoke/native-hud-follow-macos.sh` | Script entrypoint | live macOS perf smoke | HUD/loupe follow-cadence smoke for performance work, including delivered mouse-event count, sample refresh cadence, active-layer chrome cadence, and frame-tick cadence. |
| `scripts/smoke/native-visual-contract-macos.sh` | Script entrypoint | live macOS smoke | Core native-host behavior contract: repeated real click freezes, repeated held drag freezes, in-drag and frozen screenshots, click/drag editability, border-leak, scrim, and handoff telemetry gates. |
| `scripts/smoke/native-scroll-capture-macos.sh` | Script entrypoint | live macOS scroll smoke | Real frozen-region scroll-capture smoke on a deterministic scrollable native window; asserts the session is unlocked, ScreenCaptureKit exposes a display, drag freeze occurs, Scroll Capture starts in `manual_universal` mode, overlay-local wheel forwarding is the input path, real wheel input moves the target through short all-overlay passthrough windows, selected-region frames sample through the live stream or below-overlay fallback, no legacy auto-scroll telemetry appears, and multiple committed growth events append before copy/export. The default driver is `SCROLL_DRIVER=wheel`; `SCROLL_DRIVER=notification` is retained for direct background-control diagnosis. |
| `scripts/smoke/native-prepared-export-macos.sh` | Script entrypoint | native export smoke | Prepared export readiness for native-host copy/save/OCR-facing image preparation. |
| `scripts/smoke/self-check-macos.sh` | Script entrypoint | smoke readiness | Verifies native HUD-follow smoke tooling readiness without the real GUI run. |
| `scripts/smoke/macos.sh` | Script entrypoint | smoke aggregation | Runs the core native visual contract and HUD-follow responsiveness smoke. |
| `scripts/perf/local.sh` | Script entrypoint | deterministic perf sweep | Runs `rsnap-perf` against fixed export and scroll-capture fixtures. |
| `scripts/perf/self-check-macos.sh` | Script entrypoint | perf aggregation | Runs local deterministic benches plus macOS smoke readiness. |
| `scripts/perf/macos.sh` | Script entrypoint | perf aggregation | Runs local deterministic benches, the HUD-follow perf smoke, and the core native visual contract. |
| `packages/rsnap-capture-core/benches/scroll_capture.rs` | Deterministic bench | hot-path perf | Stable fingerprint, overlap-match, and one-step session commit baselines. |
| `packages/rsnap-capture-core/src/scroll_capture/tests.rs` | Scroll-capture session semantics | scroll session | Downward stitching, overlap resolution, and worker-pairwise session semantics. |
| `apps/rsnap-perf/` | Deterministic bench | perf sweep | Fixed-fixture export, capture-frame, frozen-edit, and scroll-capture timing with checksum-backed correctness checks. |

## Cleanup rules

- Prefer deleting or merging a script only when another remaining script still provides the same
  stable entrypoint value.
- Keep native-host smoke entrypoints on `scripts/build_and_run.sh`; raw `swift build` is not a
  substitute because the Swift executable must be relinked after Rust `rsnap-host-ffi` changes.
- Prefer deleting or merging a deterministic Rust test only when its remaining assertions are
  already owned by `scroll_capture/tests.rs`, `rsnap-perf`, or the Criterion benchmark target.
- Prefer extracting shared fixtures when two layers intentionally cover different behavior with the
  same synthetic frame families.
- Do not revive legacy overlay runtime or recorded-replay harnesses as validation entrypoints.
  Stitching semantics belong in `scroll_capture/tests.rs`; live behavior belongs in native macOS
  smoke; repeatable performance belongs in `rsnap-perf` and the scroll-capture benchmark target.
