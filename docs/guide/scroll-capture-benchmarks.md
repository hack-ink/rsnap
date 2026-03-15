# Scroll-Capture Benchmark Guide

Goal: Run the deterministic `rsnap-overlay` scroll-capture benchmarks, understand the committed
fixture shape, and save or compare local baselines without touching a desktop session.

Read this when: You are validating scroll-capture performance, comparing image-processing changes,
or refreshing the local baseline for `XY-111` style non-GUI benchmarks.

Inputs: `docs/spec/performance_tracking.md`; `packages/rsnap-overlay/benches/scroll_capture.rs`;
`packages/rsnap-overlay/src/scroll_capture.rs`

Depends on: `docs/spec/performance_tracking.md`

Outputs: A repeatable local benchmark run, an optional saved Criterion baseline, and a clear
understanding of what the synthetic fixture is intended to cover.

## Fixture contract

The committed benchmark fixture is code-generated inside `scroll_capture::bench_support`; it does
not depend on external PNG assets.

Properties:

- Each scenario builds a synthetic RGBA document with deterministic row and column structure.
- The document deliberately includes low-information side margins and more informative interior
  bands so the same informative-span and overlap-selection logic used in shipping scroll capture is
  exercised in benchmarks.
- Benchmark windows are cropped from that document at fixed offsets, so repeated runs always feed
  the same base frame, shifted comparison frame, and fingerprint frame.
- The current scenarios are `baseline` and `wide`.

Covered benchmark groups:

- `scroll_capture_fingerprint`: lower-level image fingerprint generation on a fixed window.
- `scroll_capture_overlap_match`: direct overlap and motion matching between two fixed windows.
- `scroll_capture_session_commit`: a one-step downward `ScrollSession` commit using the same
  fixture pair.

## Run the benchmark target

Use the direct crate benchmark target when you only need the scroll-capture hot paths:

```bash
cargo bench -p rsnap-overlay --bench scroll_capture -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.1
```

That command is the fast local smoke-sized run used for verification in this repo.

## Save a local baseline

When you want a reusable before/after comparison on the same machine, save a named Criterion
baseline:

```bash
cargo bench -p rsnap-overlay --bench scroll_capture -- --save-baseline local-scroll-capture
```

Criterion stores the baseline under `target/criterion`, so keep comparisons on the same checkout
and machine class when possible.

## Compare against a saved baseline

After code changes, compare the same target against the saved baseline:

```bash
cargo bench -p rsnap-overlay --bench scroll_capture -- --baseline local-scroll-capture
```

Use this comparison to spot relative regressions in the fingerprint, overlap-match, and
session-commit groups before escalating to live desktop smoke.

## When to use a different surface

- If the regression is in live overlay cadence, HUD movement, or loupe timing, use the overlay
  instrumentation and desktop smoke surfaces instead of this guide.
- If the fixture itself needs to change because the scroll-capture algorithm contract changed,
  update the code-generated fixture in `scroll_capture::bench_support` and keep the scenario names
  explicit in the commit so baseline history remains interpretable.
