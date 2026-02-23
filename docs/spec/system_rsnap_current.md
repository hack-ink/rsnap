# rsnap Current System Spec (as-is)

Purpose: Define the observable, normative behavior of `rsnap` as implemented in this repository
at version `0.1.0`.

Audience: LLM-first. This document uses explicit nouns, stable field names, and concrete
constraints.

## Scope

This spec covers:

- The capture trigger surfaces (tray menu, global hotkey).
- The capture flow (overlay selection -> capture -> cache -> editor reveal).
- The overlay sidecar discovery and its stdout JSON contract.
- The backend IPC commands exposed to the frontend.
- Persistence locations and output formats.

This spec does not cover:

- Future v1 behavior (see `docs/spec/system_rsnap_v1.md`).
- Packaging/signing details beyond sidecar discovery.

## Supported platforms

- Primary targets: macOS and Windows.
- Linux support is not defined by this spec.

## Terms

- "Main process": the Tauri application binary `rsnap`.
- "Overlay sidecar": the native helper binary `rsnap-overlay` spawned by the main process.
- "Editor": the frontend web UI loaded in the Tauri `main` webview window.
- "Virtual desktop": the bounding rectangle that contains all monitors in global pixel
  coordinates.

## Capture triggers

The main process MUST support the following capture triggers:

- A tray menu item with id `capture-now`.
- A global shortcut with default chord `Ctrl+Shift+S`.

When a capture trigger fires, the main process MUST attempt to run a capture and then reveal the
editor window.

## Capture flow overview

1. Main process spawns the overlay sidecar.
2. Main process reads exactly one line of UTF-8 JSON from sidecar stdout.
3. If the sidecar returns a selection, the main process performs capture using `xcap`:
   - `window` selection -> capture that window id.
   - `region` selection -> capture and crop the selected region in global pixel coordinates.
4. Capture output MUST be written to the app cache directory as `last_capture.png`.
5. Main process reveals and focuses the editor window and forces a frontend refresh.

If the sidecar fails, the main process MUST fall back to capturing the primary display.

## Overlay sidecar: stdout JSON contract

Transport:

- Sidecar writes exactly one JSON object as a single line to stdout, flushes, then exits.
- Main process reads exactly one line with a 10s timeout.

Message schema (tagged union):

```jsonc
{ "type": "cancel" }

{ "type": "window", "window_id": 123 }

{
  "type": "region",
  "rect": { "x": 100, "y": 200, "width": 640, "height": 480 }
}

{ "type": "error", "message": "..." }
```

Semantics:

- `cancel`: main process MUST treat as "no-op capture" and return success.
- `window`: main process MUST capture the selected window by matching `window_id` against
  `xcap::Window::all()`.
- `region`: main process MUST capture all monitors, composite into a virtual desktop image, then
  crop the region using the `rect` in global pixel coordinates.
- `error`: main process MUST log the error and fall back to primary-display capture.

Coordinates:

- All `rect` coordinates are in global screen pixel space, matching `xcap::Monitor::{x,y,width,height}`.

## Overlay sidecar: discovery rules

The main process MUST locate the sidecar binary using the following priority order:

1. If environment variable `RSNAP_OVERLAY_PATH` is set, use that single path.
2. Otherwise, build a candidate list of names:
   - Always include `rsnap-overlay` (and `rsnap-overlay.exe` on Windows).
   - If compile-time env `RSNAP_TARGET_TRIPLE` is present, also include
     `rsnap-overlay-$RSNAP_TARGET_TRIPLE` (and `.exe` on Windows).
3. Search for candidates in:
   - The directory containing the current executable.
   - The Tauri resource directory.
4. Filter candidates by existence on disk. If no candidates exist, the main process MUST return an
   error and fall back to primary-display capture.

Timeout behavior:

- Main process MUST kill the sidecar if no stdout line is received within 10 seconds.
- Main process MAY kill the sidecar if it does not exit shortly after producing the first line.

## Capture semantics (backend)

### Primary display capture

- Main process enumerates monitors using `xcap::Monitor::all()`.
- Main process selects the monitor where `is_primary()==true`, otherwise the first monitor.
- Main process captures that monitor's image.

### Window capture

- Main process enumerates windows using `xcap::Window::all()`.
- Main process selects the window whose `id()` matches `window_id`.
- Main process captures that window's image.

### Region capture

- Main process enumerates all monitors.
- Main process captures each monitor image.
- Main process composites all monitor images into a single RGBA "virtual desktop" image using each
  monitor's `(x, y)` as the origin.
- Main process crops the selected `rect` intersection with the virtual desktop bounds.

Errors:

- If a selected region has `width==0` or `height==0`, the backend MUST treat it as an error.
- If a selected region is outside the virtual desktop bounds, the backend MUST treat it as an
  error.

## Persistence

Cache:

- The last capture MUST be stored as a PNG file named `last_capture.png` in the platform app cache
  directory resolved by Tauri `app_cache_dir()`.
- The backend MUST overwrite `last_capture.png` on each successful capture.

There is no capture history in the current implementation.

## IPC commands (Tauri invoke handler)

All commands are invoked from the frontend using Tauri `invoke()` with snake_case names.

### `capture_now`

- Input: none.
- Output: `Ok(())` or `Err(String)`.
- Behavior: run the capture flow and reveal the editor window.

### `get_last_capture_base64`

- Input: none.
- Output: `Ok(String)` or `Err(String)`.
- Behavior: read `last_capture.png` and return Base64 (standard alphabet) of the file bytes.

### `save_png_base64`

- Input: `pngBase64: String` (Base64 payload, optionally with a leading data URL header).
- Output: `Ok(String)` (saved file path string) or `Err(String)`.
- Behavior:
  - Decode Base64 into PNG bytes.
  - Choose a default filename `rsnap-capture-$timestamp_ms.png`.
  - Resolve the user's downloads directory.
  - Write the file to the downloads directory.

Only `.png` output is supported.

### `copy_png_base64`

- Input: `pngBase64: String` (Base64 payload, optionally with a leading data URL header).
- Output: `Ok(())` or `Err(String)`.
- Behavior: decode bytes as PNG and write the image to the system clipboard.

### `open_pin_window`

- Input: none.
- Output: `Ok(())` or `Err(String)`.
- Behavior:
  - Reveal and focus the editor window.
  - Set the editor window to always-on-top.

## Editor UI (current)

The editor UI is a single page that provides:

- Capture trigger button (`Capture`) that calls `capture_now`, then refreshes `get_last_capture_base64`.
- Crop selection by pointer drag on the canvas.
- Export actions:
  - `Save` exports either the crop selection or full image via `save_png_base64`.
  - `Copy` exports either the crop selection or full image via `copy_png_base64`.
  - `Pin` calls `open_pin_window`.

The editor does not implement annotations in the current implementation.

## Privacy and networking

- The current implementation MUST be local-only (no network calls are required for capture/edit/export).
- The current implementation MUST NOT include telemetry.
