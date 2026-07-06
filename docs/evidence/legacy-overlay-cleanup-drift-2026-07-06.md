---
title: "Legacy Overlay Cleanup Drift Audit"
description: "Drift audit for removal of the retired Rust overlay UI runtime and egui dependency."
type: "Drift Audit"
status: active
authority: normative
owner: hack-ink/rsnap
last_verified: 2026-07-06
---

# Legacy Overlay Cleanup Drift Audit

## Watched Claims

- The active Rust dependency graph no longer contains `egui`, `egui-wgpu`, `egui-winit`, or
  `egui-phosphor`.
- The checked-in `rsnap-overlay` crate no longer exposes or compiles the retired Rust overlay UI
  runtime, backend worker, trace recorder, or deleted replay harness.
- Active docs and README routes no longer direct agents to deleted replay scripts or deleted
  overlay runtime tests.
- Retained scroll-capture correctness evidence is represented by Rust session tests, deterministic
  perf surfaces, and native macOS smoke, not deleted replay scripts.

## Evidence Anchors

- `Cargo.toml` and `packages/rsnap-overlay/Cargo.toml` remove the `egui*` workspace dependencies
  and crate dependencies.
- `packages/rsnap-overlay/src/lib.rs` exposes only frozen edit/export and macOS live sampling
  transition helpers; scroll stitching has since moved to `packages/rsnap-capture-core/`.
- `packages/rsnap-overlay/src/point.rs`, `packages/rsnap-overlay/src/frozen_export.rs`,
  `packages/rsnap-overlay/src/frozen_export/stroke_raster.rs`, and
  `packages/rsnap-overlay/src/text_rendering.rs` replace the former UI-toolkit point/font coupling
  with crate-local export/text types.
- `docs/reference/workspace-layout.md`,
  `docs/reference/smoke-perf-validation-surface.md`, and
  `docs/runbook/performance-validation.md` describe the current retained validation surface.
- Validation commands run on this change: `cargo check --workspace --all-features --all-targets`,
  `cargo clippy --all-features --all-targets --workspace -- -D clippy::all -D clippy::too_many_lines
  -D clippy::unwrap_used -D clippy::use_self -D clippy::wildcard_imports -D missing-docs -D
  unused-crate-dependencies -D warnings`, `cargo test -p rsnap-capture-core scroll_capture --lib`,
  `cargo +nightly fmt --all -- --check`, `git diff --check`, and docs-reference link checks.

## Reverse Checks

- `rg -n 'name = "egui|egui-|"egui"|"egui-' Cargo.lock Cargo.toml
  packages/rsnap-overlay/Cargo.toml packages/rsnap-host-ffi/Cargo.toml apps/rsnap-perf/Cargo.toml`
  returned no matches.
- `cargo tree -p rsnap-overlay -i egui --depth 4` and
  `cargo tree -p rsnap-host-ffi -i egui --depth 4` both reported that no `egui` package matches the
  package ID.
- `rg -n "replay-scroll-capture|scroll_capture_replay|replay_recorded|replay_support|trace_recording|overlay/tests|worker_tick_runtime|worker_observation_runtime|recorded-trace flow|deterministic replay|recorded replay" README.md docs/index.md docs/reference docs/runbook docs/spec docs/decisions scripts packages apps -S`
  returned no active-route matches after the docs update.
- `rg -n "egui|egui-wgpu|egui-winit|egui-phosphor" Cargo.toml Cargo.lock packages apps native
  scripts README.md docs -S` now returns only normative or historical documentation mentions.

## Verdict

pass

## Required Updates

- None for the retired Rust overlay UI runtime or `egui` dependency surface.
- Future cleanup should continue migrating the remaining transition helpers from `rsnap-overlay`
  into `rsnap-capture-core` when their ownership boundary is ready.

## Citations

- `docs/reference/workspace-layout.md`
- `docs/reference/smoke-perf-validation-surface.md`
- `docs/runbook/performance-validation.md`
- `packages/rsnap-overlay/src/lib.rs`
