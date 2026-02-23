# rsnap v1 System Spec (to-be)

Purpose: Define the normative v1 user-facing behavior for `rsnap`, independent of implementation
details (native sidecar vs Tauri window). This spec is the contract that v1 must satisfy.

Audience: LLM-first. This spec uses explicit nouns, stable field names, and concrete constraints.

## Product goal

`rsnap` v1 is a small, fast screenshot utility for macOS and Windows that provides:

- A tray/menubar presence.
- A global hotkey that enters a "capture session" overlay UX.
- Crop + basic annotation editing inside the capture session.
- Export actions: copy, save (PNG), and pin (always-on-top).

## Supported platforms

- v1 targets: macOS and Windows.
- v1 does not define Linux support.

## Non-goals (v1)

The following are explicitly out of scope for v1:

- Fullscreen capture across all displays (virtual desktop composite output).
  - Tracking issue: https://github.com/hack-ink/rsnap/issues/2
- Cross-display region selection (dragging a region that spans multiple monitors).
  - Tracking issue: https://github.com/hack-ink/rsnap/issues/3
- Scrolling capture, OCR, ruler, color picker, GIF/video capture, cloud sync.

## Terms

- "Capture session": the full-screen, always-on-top UX that the user interacts with from hotkey
  press until the user completes export or cancels.
- "Selection": the target to capture, one of:
  - a region rectangle (Region mode),
  - a window (Window mode),
  - the active monitor (Fullscreen mode).
- "Active monitor": the monitor that contains the mouse cursor at the time the fullscreen
  selection is chosen.

## Capture session UX contract

### Entry points

The app MUST provide:

- A global hotkey (configurable) to start a capture session.
- A tray menu entry to start a capture session.

The default global hotkey SHOULD be `Ctrl+Shift+S`.

### Primary invariant: user-perceived continuity

From the user's perspective, capture + edit + export MUST feel like a single, continuous overlay
experience.

- The implementation MAY transition between windows/processes internally.
- The implementation MUST avoid noticeable focus flicker and MUST present the editor immediately
  after a selection is established.

### Modes

The capture session MUST support the following capture modes:

- `smart` (default): drag to select a region; click to select a window.
- `region`: drag to select a region.
- `window`: click to select a window.
- `fullscreen`: capture the active monitor.

The capture session MUST expose mode switching via:

- Keyboard: `R` (region), `W` (window), `F` (fullscreen).
- UI control: a visible affordance for the current mode (button/toggle/hint).

### Cancellation and confirmation

- `Esc` MUST cancel the capture session without exporting.
- The session MUST provide a "Done" action that finalizes the capture for export.
  - The implementation MAY map `Enter` to "Done".

### Selection constraints

Region selection:

- Region selection MUST be clamped to the bounds of a single monitor.
- Region selection MUST NOT span across multiple monitor bounds in v1.

Window selection:

- Window selection MUST highlight the selected window before confirmation (hover highlight or
  equivalent).
- The capture pipeline MUST capture the selected window by native window id (not by cropping a
  fullscreen bitmap), unless the platform cannot provide window capture.

Fullscreen selection:

- Fullscreen MUST capture the active monitor.
- Active monitor MUST be defined by the monitor whose global bounds contain the mouse cursor at
  the moment fullscreen is selected (for example, when `F` is pressed).

Fallback:

- If the active monitor cannot be resolved, fullscreen MUST fall back to the primary display.

### Editing inside the capture session

Once a selection is established, the capture session MUST immediately provide an editor view for
that selection.

The editor MUST support (minimum):

- Crop adjustment inside the session (resize/move selection).
- Basic annotations:
  - pen,
  - arrow,
  - rectangle,
  - text.
- Undo/redo (minimum viable).

The editor MUST allow export of either:

- the full selected content (selection bounds), or
- a refined crop (if the user changes the crop).

## Export actions (v1)

The capture session MUST provide:

- Copy: write the final PNG to the system clipboard.
- Save: write the final PNG to disk.
- Pin: present the final image in an always-on-top view.

PNG is the only required output format for v1.

## File locations and naming (v1)

### Default save directory

- Default save directory MUST be the user's Desktop directory.
- If the Desktop directory cannot be resolved, the app MUST fall back to the user's Downloads
  directory.

### Default filename

The default filename template MUST be timestamp-based and MUST end in `.png`.

Default template:

- `rsnap-capture-{timestamp_ms}.png`

`timestamp_ms` MUST be milliseconds since Unix epoch.

## Settings contract (v1)

Settings MUST be persisted in the platform configuration directory (Tauri `app_config_dir()` or
equivalent).

### Settings schema (normative)

```jsonc
{
  "version": 1,
  "hotkeys": {
    "start_capture_session": "Ctrl+Shift+S",
    "capture_region_direct": null,
    "capture_window_direct": null,
    "capture_fullscreen_direct": null
  },
  "capture": {
    "default_mode": "smart",
    "remember_last_mode": true,
    "fullscreen_target": "active_monitor"
  },
  "save": {
    "default_directory": "desktop",
    "filename_template": "rsnap-capture-{timestamp_ms}.png",
    "auto_save": false
  },
  "copy": {
    "auto_copy": false
  }
}
```

Constraints:

- `capture.default_mode` MUST be one of: `smart`, `region`, `window`, `fullscreen`.
- `capture.fullscreen_target` MUST be `active_monitor` in v1.
- `save.default_directory` MUST be `desktop` in v1.
- If `auto_save==true`, the app MUST still provide user feedback that a save occurred.
- If `auto_copy==true`, the app MUST still provide user feedback that a copy occurred.

## Privacy and networking (v1)

- v1 MUST be local-only by default (no network calls required for capture/edit/export).
- v1 MUST NOT include telemetry.
- v1 logs MUST NOT contain screenshot image data.
