---
title: "Scroll Capture Architecture"
description: "Scroll Capture Architecture documentation for Rsnap."
type: "Decision"
status: active
authority: normative
owner: acg-box/rsnap
last_verified: 2026-07-06
---
# Scroll Capture Architecture

Status: accepted and implemented for the current macOS validation path

Date: 2026-05-10

Context: Earlier scroll-capture attempts passed deterministic tests but failed live use: tearing,
sparse appends after the page had already reached the bottom, false joins after rewind, and a first
frozen toolbar frame dominated by tint. The product target is closer to CleanShot/Xnip behavior:
start from any dragged region over a scrollable app, let scrolling feel native, append only proven
content, and fail closed instead of creating a bad stitched image.

Decision: Rsnap uses one generic product path for macOS Scroll Capture: the focused overlay receives
wheel input inside the selected viewport, forwards that input to the underlying target with the
original wheel magnitude through short all-overlay passthrough windows, samples ordered
ScreenCaptureKit region frames only around input bursts, falls back to a below-overlay region capture
when the live stream does not provide a fresh region, and Rust owns monotonic registration/commit.
Accessibility is not part of the product path; Rsnap does not use Accessibility target acquisition,
settable AX scroll bars, app scripting, browser/DOM access, or a cancellable CGEvent tap.

Consequences: Scroll Capture is no longer limited to apps exposing settable AX scroll bars. It needs
Screen Recording because frames come from ScreenCaptureKit. Rsnap observes wheel input only as an
input, forwarding, and sampling signal. Wheel deltas are not treated as content movement authority,
because trackpad/mouse deltas do not reliably map to viewport pixels.

Supporting research contract: `docs/research/scroll-capture-prior-art-2026-05-10.md`.

## Prior-Art Findings

| Source | What matters | Rsnap decision |
| --- | --- | --- |
| ScrollSnap | Open-source Swift uses ScreenCaptureKit region screenshots, overlay mouse passthrough, a repeating capture timer, and Vision translational registration. | Copy the macOS shape: region capture, temporary overlay passthrough, and Vision as an offset proposal. Do not copy its correctness model: it advances the previous frame after failed registration and crops committed output on upward motion. |
| wayscrollshot | Captures the selected region continuously while the user scrolls, skips duplicate signatures, and appends only the new bottom slice after overlap proof. | Copy the universal loop: user scrolls naturally; capture runs continuously; stitching is fail-closed and append-only. |
| ShareX | Uses repeated rectangle captures, configurable scroll methods, duplicate-image stopping, best-match overlap, and bottom-edge ignore for sticky chrome. | Copy the idea that scroll capture is a loop with explicit stop/failure status and overlap search that ignores unstable edges. Do not make a platform-specific message/scrollbar path the only product path. |
| Xnip public guide | Documents same-portion matching and pausing when matching fails; warns about fast, dynamic, nonvertical, and upward scrolls. | Bad input must pause/no-op, not guess. Downward growth is the only committed direction. |
| CleanShot URL API | Exposes scrolling capture as a first-class mode, including auto-scroll parameters. | Product entry should be direct and obvious, but proprietary internals are not evidence for a required AX design. |

## Architecture

### Entry

Scroll Capture starts only from an editable dragged-region frozen capture. The toolbar button and
plain `s` both call the same native entry point. On start:

- freeze the selected region and create the Rust scroll stitch session from that exact first frame;
- switch the frozen toolbar to non-editing scroll-capture state;
- forward overlay-local wheel events through short all-overlay passthrough windows;
- keep a global scroll-wheel observer only as diagnostics/fallback telemetry;
- start the ordered ScreenCaptureKit region-frame sampling loop.

### Sampling

The capture loop drains ordered live region frames by sequence number. It does not debounce to a
single latest frame and does not sample from a stale cache as commit authority. When the live stream
has no ordered frame for the selected region, Swift captures the same region below the overlay and
still sends it through the Rust overlap gate instead of appending blindly. Sampling is bounded to
short windows after scroll input instead of running forever on the main actor, so toolbar clicks and
cancel remain responsive while intermediate repaint states are still observed and rejected or
committed in order.

Wheel events are not movement authority. Overlay-local wheel handling treats each event as an input
signal, forwards the real wheel magnitude to the target while all overlay windows temporarily ignore
mouse events, and samples the resulting ScreenCaptureKit frames. Reverse/upward input may move the
underlying viewport, but it cannot mutate or crop the committed canvas. The marker on synthetic events
prevents feedback loops.

### Registration And Commit

Rust owns all commit decisions:

- Rust pairwise registration can propose downward motion between adjacent ordered frames.
- Pixel overlap corroboration must confirm the proposal before append.
- Growth is monotonic and downward-only.
- A blocked overshot frame does not become the committed frontier.
- Upward motion records rewind/observation state but never crops or mutates committed output.
- After rewind, growth resumes only after the previous committed frontier is reacquired and the
  viewport advances beyond it.
- Ambiguous overlap, low-information content, sticky/changing bands, large skipped gaps, and dynamic
  repaint states become no-commit states.

Copy/save always exports the same committed canvas shown in the minimap preview.

### Permissions

Required:

- Screen Recording, because ScreenCaptureKit supplies the capture frames.

Not required for Scroll Capture:

- Accessibility or Input Monitoring;
- AX target acquisition or settable scroll bars;
- target app scripting or browser/DOM access.

## Rejected Options

| Option | Why rejected |
| --- | --- |
| Latest-frame passive sampling | It can drop intermediate frames and append only a tiny tail after the page already reached the bottom. |
| AX-controlled product path | It only works for targets exposing controllable accessibility scroll bars, which violates the goal of one generic mode across arbitrary apps. |
| Permanent overlay passthrough | It makes the toolbar unreachable and loses control of the capture UI. Rsnap instead uses short all-overlay passthrough windows only while forwarding one wheel event. |
| ScrollSnap-style timer as commit clock | A timer is useful as a sampling mechanism, but correctness cannot advance the comparison anchor after failed registration. |
| Wheel delta as motion authority | Wheel/trackpad delta is input intent, not content motion. It can differ by device, target app, acceleration, rubber-banding, and scroll position. |
| Browser/DOM full-page capture | Useful as a future specialized browser path, but it does not cover arbitrary macOS apps/windows. |

## Implementation Status

- The native host starts Scroll Capture in `manual_universal` mode and emits
  `capture.scroll_capture_mode outcome=manual_universal`.
- The native host emits `capture.scroll_input_tap outcome=not_used` and uses overlay-local wheel
  forwarding as the release-quality input path.
- Synthetic wheel forwarding preserves the real wheel magnitude for target app feel; wheel magnitude
  is never used as pixel motion authority, and reverse/upward viewport motion is observed without
  append.
- Rust's worker-pairwise path handles ordered frames, overlap corroboration, overshot blocking, and
  rewind/reacquire behavior.
- Settings exposes Screen Recording as the only required permission for Scroll Capture.
- The first frozen toolbar frame is covered by the native visual contract tint check.
- Active scroll-capture toolbar glass keeps the configured Settings tint/material but leaves the
  toolbar backing in live HUD-style transparency so Liquid Glass samples the real live content
  instead of a frozen/tinted surrogate.

## Validation Contract

Before calling Scroll Capture fixed:

- `scripts/smoke/native-scroll-capture-macos.sh` must pass with `SCROLL_DRIVER=wheel` for keyboard
  start and toolbar start.
- `scripts/smoke/native-visual-contract-macos.sh` must pass and keep toolbar tint dominance below
  its threshold.
- Rust scroll-capture tests must pass, including overshot and rewind/reacquire cases.
- `cargo make checks` must pass before handoff.

## Sources

- ScrollSnap: https://github.com/Brkgng/ScrollSnap
- wayscrollshot: https://github.com/jswysnemc/wayscrollshot
- ShareX scrolling capture: https://github.com/ShareX/ShareX
- Xnip scrolling capture guide: https://www.xnipapp.com/scrolling-capture/
- CleanShot URL API: https://cleanshot.com/docs-api
