# Scroll-Capture Recovery Plan

Goal: Keep macOS Scroll Capture product-quality without repeating the old patch loop where
deterministic checks passed but live use still tore, lagged, or stitched only a small final region.

Read this when: scroll capture feels wrong in the running native host, a change touches scroll
sampling, stitching, rewind, toolbar entry, permission copy, or release/readiness claims.

Inputs: `docs/spec/capture-session.md`; `docs/decisions/scroll-capture-architecture.md`;
`docs/reference/smoke-perf-validation-surface.md`; current user-visible live failures.

Outputs: A procedural recovery path, explicit acceptance gates, and stop conditions for the current
universal user-scroll architecture.

## Current Status

Scroll Capture on this branch uses the accepted universal macOS path:

- dragged-region Frozen capture as the base frame;
- overlay-local wheel forwarding through short all-overlay passthrough windows;
- ordered ScreenCaptureKit region frames with a below-overlay region fallback when the live stream is
  not producing fresh selected-region frames;
- Rust worker-pairwise Vision proposal plus overlap corroboration;
- append-only monotonic downward commits;
- rewind/no-overlap fail-closed behavior;
- first-frozen-toolbar tint protection and active scroll toolbar live-HUD glass backing.

It does not use Accessibility to acquire or drive target scroll bars, and it does not use wheel
deltas as content movement authority. The remaining readiness bar is live validation: the
deterministic smoke must prove real overlay-local wheel input is forwarded to the target, the target
scrolls during short passthrough windows, pairwise registration sees ordered live frames, and multiple
committed growth events appear.

The original failure symptoms this plan guards against are:

- visible tearing in the stitched output;
- preview not following the user's scroll;
- reaching the bottom while only a small amount of content is appended;
- false append after upward rewind;
- first frozen toolbar rendering as an almost fully tint-colored toolbar.

## Prior-Art Anchors

Authoritative prior-art analysis lives in `docs/decisions/scroll-capture-architecture.md` and the
machine-readable run at `docs/research/scroll-capture-prior-art-2026-05-10.json`.

Procedural takeaways:

- ScrollSnap shows the right macOS shell: ScreenCaptureKit region capture, temporary overlay
  passthrough, and Vision registration.
- wayscrollshot shows the right generic loop: continuous selected-region capture while the user
  scrolls, duplicate skipping, and append only after overlap proof.
- ShareX shows the right workflow shape: selected rectangle, repeated captures, explicit status,
  duplicate stopping, and unstable-edge handling.
- Xnip's public guide confirms that match failure, fast scroll, dynamic content, upward scroll, and
  nonvertical movement must pause/no-op instead of producing a guessed stitch.

## Non-Negotiable Rules

- Never append rows from weak, ambiguous, stale, or large-gap registration.
- Never treat wheel delta as viewport movement authority.
- Never let raw fast wheel bursts pass directly to the target during release-quality Scroll Capture.
- Never repost a global wheel event that the target app already received.
- Never let a blocked overshot frame become the new committed frontier.
- Never crop or mutate committed output on upward rewind.
- Growth resumes after rewind only when the last committed viewport is reacquired and the viewport
  advances beyond it.
- Preview, copy, and save must render the same committed canvas.
- A ready claim requires a fresh unlocked-desktop native scroll smoke.

## Recovery Checklist

1. Confirm product entry.
   - Drag-region Frozen capture must expose the toolbar Scroll Capture button.
   - Plain `s` and the toolbar button must both emit `capture.scroll_capture_entry` with the correct
     source and then `capture.scroll_capture_started`.
   - `capture.scroll_capture_mode` must report `outcome=manual_universal`.

2. Confirm input delivery.
   - During Scroll Capture, `capture.scroll_input_tap` must report `outcome=not_used`.
   - Real wheel input inside the selected viewport must emit `capture.scroll_wheel_intercepted` with
     `source=overlay`.
   - The global wheel observer may emit `capture.scroll_wheel_observed` as diagnostic telemetry.
   - It must not produce legacy `capture.scroll_auto_*` telemetry.
   - The deterministic smoke background must report `offsetY > 0` for `SCROLL_DRIVER=wheel`.

3. Confirm ordered sampling.
   - Samples must come from `ordered_live_stream_region`.
   - The smoke must show `capture.scroll_sample_observed`.
   - Missing live stream frames may show a waiting status, but a passing smoke must eventually sample
     and commit.

4. Confirm stitching authority.
   - Multiple `outcome=committed` samples must appear before copy/save.
   - Export height growth must exceed the smoke threshold.
   - Unsupported direction and no-overlap states must leave export height unchanged.
   - Rust tests for overshot blocked frames and rewind/reacquire must remain green.

5. Confirm visual polish.
   - The native visual contract must pass.
   - Toolbar tint dominance must stay below the configured threshold on the first frozen frame.
   - Active scroll toolbar Liquid Glass must preserve Settings tint/material semantics; do not lower
     tint to hide the issue.

## Validation Order

Run these in order for this branch:

1. `scripts/smoke/native-scroll-capture-macos.sh` with default `SCROLL_DRIVER=wheel` and
   `SCROLL_START_METHOD=keyboard`.
2. `scripts/smoke/native-scroll-capture-macos.sh` with `SCROLL_DRIVER=wheel` and
   `SCROLL_START_METHOD=toolbar`.
3. `scripts/smoke/native-visual-contract-macos.sh`.
4. `cargo make test-host-reset`.
5. `cargo make checks`.

For broader release readiness, add at least one manual release-build acceptance pass on a long
webpage or representative scrollable app:

- slow scroll: continuous, non-torn growth;
- medium scroll: tracks or pauses without tearing;
- fast flick: no bad append;
- upward rewind: no append while moving upward;
- resume after rewind: appends only after reacquiring the committed viewport;
- bottom: final canvas includes the observed path instead of only the final viewport.

## Stop Conditions

Stop and revisit the architecture instead of stacking another patch when any of these are true:

- Forwarded wheel input does not reach the underlying target during short passthrough windows.
- ScreenCaptureKit cannot provide coherent ordered frames for the selected region.
- Ordinary user scroll regularly jumps beyond overlap before Rsnap can sample intermediate frames.
- Rust registration cannot distinguish real growth from repeated or dynamic content without
  accepting false joins.

In those cases the honest outcome is a narrower supported contract or a separate specialized mode,
not a best-guess stitcher.
