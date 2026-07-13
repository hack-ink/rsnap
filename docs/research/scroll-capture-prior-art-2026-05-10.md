---
title: "Scroll-Capture Prior-Art Research"
description: "Research contract for the scroll-capture prior-art review completed on 2026-05-10."
type: "Research Contract"
status: active
authority: normative
owner: acgxv/rsnap
last_verified: 2026-07-06
---

# Scroll-Capture Prior-Art Research

## Question

Which scroll screenshot architecture should Rsnap implement after comparing open-source scroll
capture projects and mature product behavior?

## Scope

Success criteria:

- Separate source-code evidence from proprietary product behavior.
- Choose an architecture that can work across arbitrary macOS apps without false joins, sparse
  appends, or rewind corruption.
- Record the permission consequence so implementation and Settings do not drift.

Constraints:

- Do not infer proprietary internals from CleanShot X or Xnip beyond public behavior and docs.
- Prefer open-source code and local runtime evidence for implementation details.
- Keep Rsnap's Rust stitcher as the durable confidence gate; Swift remains macOS integration glue.

Stop rule: stop once the evidence can justify one concrete product path and its failure boundaries.

## Evidence

- ScrollSnap uses ScreenCaptureKit region capture, overlay mouse passthrough, a repeating capture
  timer, and Vision translational registration against the previous screenshot.
- ScrollSnap advances `previousImage` even when offset detection fails and crops the running image
  on upward offsets, which can lose the committed frontier or mutate committed output during
  rewind.
- wayscrollshot captures the selected region continuously while the user scrolls, skips duplicate
  signatures, and appends only the new bottom slice after overlap matching.
- wayscrollshot treats failed confidence as `NoMatch`, insufficient positive offset as
  `NoProgress`, and only appends when overlap proof produces enough new height.
- ShareX repeatedly captures a selected rectangle, detects duplicate images for stopping, searches
  overlap against the accumulated result, ignores unstable bottom edge rows, and reports success or
  partial success.
- Xnip documents same-portion matching and pause/failure causes including fast scroll, dynamic
  content, nonvertical scroll, and upward scroll.
- CleanShot exposes scrolling capture as a first-class URL action with start and autoscroll
  parameters, but the public API does not prove a specific internal stitching implementation.
- Rsnap's previous AX-gated path made Scroll Capture usable only in targets exposing controllable
  accessibility scroll bars, which violated the goal of one generic mode across arbitrary apps.
- Rsnap's Rust worker-pairwise tests cover successive growth, overshot blocked frames that must not
  append tails, and rewind/reacquire before growth resumes.
- Live wheel smoke showed permanent overlay passthrough can make the toolbar unreachable and can
  let the target jump beyond capturable overlap; the current product path uses short all-overlay
  passthrough windows for each forwarded wheel event instead.

## Options

- AX-controlled product path.
- Timer-driven latest-frame capture with Vision registration only.
- Wheel-delta motion authority.
- Browser/DOM-specific capture.
- Universal overlay-local wheel forwarding plus ordered frame sampling and Rust fail-closed
  stitching.

## Judgment

The universal overlay-local wheel path was decision-ready only if wheel events remain
non-authoritative, the input path forwards real target scrolling without blocking toolbar control,
and docs do not reintroduce AX target control.

## Challenge

Falsifiers:

- If open-source implementations require app-specific Accessibility control to work across apps,
  the universal manual path is weaker.
- If Rsnap's Rust pairwise path cannot reject overshot and rewind cases, user-driven capture is
  unsafe.
- If overlay-local wheel forwarding cannot move the target app during short passthrough windows,
  the chosen Swift input path is not viable.

Missing evidence at decision time:

- Live keyboard-start and toolbar-start smokes must pass with real wheel input after
  implementation.
- Representative app coverage beyond the deterministic native scroll smoke remains a product
  acceptance task.

## Decision

Rsnap should implement one universal product path: overlay-local wheel forwarding through short
passthrough windows, selected-region frame sampling around input bursts with a live-stream-first
path plus below-overlay fallback, and Rust worker-pairwise fail-closed registration. Accessibility
is not required and must not be used for target acquisition, settable AX scroll bars, or
target-app-specific automation.

Rejected options:

- `ax_controlled_product_path`
- `latest_frame_debounce`
- `scrollsnap_timer_without_committed_frontier`
- `pure_overlay_passthrough`
- `wheel_delta_motion_authority`
- `browser_dom_only`

## Promotion

Promoted into the active scroll-capture architecture and validation docs. The active docs remain
authoritative over this research contract:

- `docs/decisions/scroll-capture-architecture.md`
- `docs/spec/capture-session.md`
- `docs/runbook/performance-validation.md`
- `docs/reference/smoke-perf-validation-surface.md`

## Drift Impact

This research is supporting provenance. If the active product path changes, update the active spec,
runbook, and reference first, then either update this research contract with a supersession note or
add a new research contract.

## Citations

- `https://github.com/Brkgng/ScrollSnap`
- `ScrollSnap/Managers/StitchingManager.swift`
- `https://github.com/jswysnemc/wayscrollshot`
- `wayscrollshot/src/stitch.rs`
- `https://github.com/ShareX/ShareX`
- `https://www.xnipapp.com/scrolling-capture/`
- `https://cleanshot.com/docs-api`
- `packages/rsnap-capture-core/src/scroll_capture/tests.rs`
