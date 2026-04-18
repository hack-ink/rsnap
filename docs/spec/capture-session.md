# rsnap Capture Session Contract

Purpose: Define the product-level normative contract for rsnap capture flow, Frozen-mode
behavior, export readiness, and macOS-first scroll capture.

Status: normative

Read this when: You are implementing, reviewing, or validating capture behavior, live-mode
feedback, export flow, or scroll-capture behavior.

Not this document: Platform window ownership, native host implementation strategy, or historical
macOS shell design. Use `docs/spec/platform-host-boundary.md` for the host/core boundary,
`docs/reference/host-core-reset.md` for active migration context, and `docs/reference/` for
descriptive implementation notes.

Defines:
- capture-session entry, live-mode, frozen-mode, and export invariants
- the display-first Frozen contract
- the distinction between display readiness and export readiness
- the current macOS-first scroll-capture contract
- the presence of Frozen-mode annotation state in session output
- the existence of a separate Frozen-toolbar layout contract for primary-toolbar anchoring

This repository currently ships macOS-first, but this contract is intentionally written at the
product level rather than binding itself to a particular window toolkit or shell structure.

## Architecture posture

- rsnap product behavior must remain valid independently from any specific capture-window
  implementation strategy.
- Platform-native ownership of windows, focus, activation, cursor, IME, permissions, clipboard,
  save panels, OCR, and native capture capabilities is governed by
  `docs/spec/platform-host-boundary.md`.
- Superseded implementation details such as passive AppKit shells, dedicated key-focus shells, or
  visible `winit` capture-window ownership are not part of this contract.

## Required behavior

1. Menubar-only app (no Dock icon) on macOS.
2. Global hotkey starts capture session (default `Alt+X`, macOS: Option+X) and can be customized
   from Settings.
3. When the capture session UI is visible, underlying desktop content MUST NOT be interactive.
4. The visible capture UI should be transparent and non-dimming by default.
5. On macOS, rsnap overlay, HUD, loupe, frozen toolbar, and scroll preview surfaces MUST remain
   externally capturable by system screenshot and screen-recording tools. Internal self-capture
   correctness comes from rsnap's own capture filters and handoff logic, not window content
   protection.
6. The product contract does not require any specific platform window implementation. Focus,
   cursor, keyboard, and IME correctness are mandatory outcomes, regardless of how the native host
   achieves them.

## Live mode

- Live mode presents a capture surface for the monitor under the cursor.
- Live mode shows a HUD near the cursor with:
  - global cursor coordinates `x,y`
  - pixel color `rgb(r,g,b)` under the cursor
- Hovering over a window in live mode shows an obvious targeting outline that follows the current
  target.
- Left click + drag freezes a cropped region on the cursor monitor.
- Left click without drag hit-tests the window under the cursor on the same monitor and freezes
  that window bounds.
- If no window is hit, the click path falls back to freezing the current monitor fullscreen.
- Capture scope is single-monitor only for now. Cross-monitor region selection and cross-monitor
  window capture remain out of scope.

## Focus, activation, and session cleanup

- Entering capture MUST NOT leave the target app permanently deactivated after the selection
  completes.
- On macOS, live drag and window-click entry into Frozen mode must restore the pre-capture target
  after selection instead of leaving rsnap focused.
- Exiting capture must restore the originally captured frontmost application where the platform
  allows that behavior.
- Normal interaction must not require a visible Dock activation or an implementation artifact such
  as a temporary key/main window promotion just to complete ordinary capture flows.
- Session exit cleanup must ensure the next capture session does not inherit stale focus, stale
  key ownership, stale cursor ownership, or missing pointer input.

## Frozen mode

- `Esc` cancels capture.
- `Space` copies the frozen PNG of the selected region/window/fullscreen to the system clipboard,
  then exits.
- Cmd+S (macOS) / Ctrl+S saves the frozen PNG to disk, then exits.
- On macOS, Frozen mode may expose `Recognize Text`, which runs native OCR on the current frozen
  capture, copies the recognized text to the clipboard, and exits.
- In Frozen mode, the toolbar remains part of the floating HUD set. The live loupe is hidden after
  freeze and may be recreated only when a later live-mode transition needs it.
- In Frozen mode, a dragged-region capture may be repositioned by dragging inside the bright
  selected area; width and height remain fixed and the moved rect stays on the current monitor.
- Frozen toolbar placement and expansion invariants are governed by
  `docs/spec/frozen-toolbar-layout.md`.
- Pen behavior is governed by `docs/spec/annotation-pen.md`.

## Display-first Frozen entry

- On macOS, Frozen entry is display-first: entering Frozen mode MUST commit a display image in a
  single visible handoff instead of waiting on a later visible capture swap.
- Frozen-mode display readiness and export readiness are separate:
  - pointer drag/reposition, pen, arrow, text, spotlight, and toolbar visibility are
    display-driven
  - copy, save, OCR, scroll capture, mosaic, and any final-byte-dependent action are gated on
    export readiness
- Background region/fullscreen/window-background freezes SHOULD complete directly from a fresh
  live-stream snapshot when one is available.
- If no usable display-first path is available yet, the session may wait briefly for a follow-up
  display candidate before escalating to exceptional fallback behavior.
- Window matte freezes MAY seed display from a live-stream snapshot first and continue preparing
  export authority in the background. A later export-authority response MUST NOT overwrite an
  already-visible display image.
- Normal-path Frozen entry MUST NOT depend on hiding capture UI. Any hidden-window or equivalent
  fallback is exceptional, measurable, and outside the normal path.

## Scroll capture

- On macOS, scroll capture is available only from a dragged-region freeze.
- The frozen toolbar may expose `Scroll Capture Down`, and plain `s` may start scroll capture,
  whenever the frozen capture source is a dragged region on macOS.
- Scroll capture uses discrete monitor-region screenshots from the native platform capture API as
  the source of truth for committed downward growth.
- Pairwise image registration plus overlap proof between adjacent discrete screenshots is the
  source of truth for downward scroll progress, viewport reacquisition, and append eligibility.
- Stitching is downward-only:
  - downward motion may append committed rows
  - upward rewind may be observed, but must never append stitched growth
- After an upward rewind, growth stays blocked until trustworthy proof reacquires the last
  committed viewport and then re-advances past the resume frontier.
- If pairwise proof is weak, ambiguous, stale, or otherwise not trustworthy, the system must fail
  closed: no append, no position advance, and no best-guess resume.
- Preview, `Space`, and save/export must all render from the same committed stitched canvas.
  Provisional or preview-only state must never produce a different clipboard or saved result from
  what the user sees.
- `Space` copies the stitched image and exits. Cmd+S (macOS) / Ctrl+S saves it and exits.
  `Esc` / `Back` stops scroll capture and restores the original Frozen capture.
- Verification order is part of the contract: deterministic and replay entrypoints must pass
  before any final live touchpad acceptance run is treated as authoritative.

## HUD and control defaults

The current product surface includes three floating widgets:

- Main HUD (live info + action hint)
- Loupe (Tab-held magnified sample)
- Frozen toolbar (only visible in frozen + captured states)

Default settings:

- HUD opacity: `50` (stored `0.5`)
- HUD blur: `50` (stored `0.5`)
- Tint amount: `50` (stored `0.5`)
- Hue: `215` (stored `215.0 / 360.0`)
- Loupe activation: `Hold` (`Tab`)
- Loupe sample size: `Medium (21x21)`
- Toolbar placement: `Bottom`

Slider semantics:

- Opacity / Blur / Tint are in `0..100` percentage points.
- Hue input is an integer degree value in `0..360`.
- The HUD widget controls are disabled when `Glass HUD` is off.

## Performance and redraw

- Live RGB/Loupe sampling should be frame-stream based rather than "take a screenshot on cursor
  move".
- Cursor tracking should keep the HUD aligned and keep hover/selection feedback responsive.
- Render cadence, performance scenarios, and tracking requirements are governed by
  `docs/spec/performance.md`.

## Current non-goals

- Cross-monitor selection and cross-monitor window capture behavior.
- Rich editing workflows beyond the current frozen-toolbar tools and bounded annotation set.
