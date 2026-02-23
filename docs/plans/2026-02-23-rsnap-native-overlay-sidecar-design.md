# rsnap Native Overlay Sidecar Design

**Goal:** Implement an Xnip-style **native** capture overlay for **macOS + Windows** while keeping the existing Tauri editor UI. The overlay must support **window click capture** and **drag region capture**, with a unified interface and minimal shared complexity.

**Positioning:** Tauri remains the app shell (tray, global hotkey, editor). A separate native sidecar binary provides a best-in-class overlay without WebView constraints.

---

## Scope

### In scope (Milestone B)

- Start capture flow via tray menu or global hotkey.
- Launch a native overlay (sidecar process) on-demand.
- Overlay interactions:
  - Hover highlight top window under cursor.
  - Click to select a window for capture.
  - Drag to select a region for capture.
  - `Esc` cancels.
- Capture pipeline:
  - Window capture: capture the selected window by native window id.
  - Region capture: capture the selected region in **global screen pixel coordinates**.
- Open the existing editor window with the resulting capture (reuse existing export/copy/pin features).

### Not in scope (Milestone B)

- Full tool parity with Xnip (OCR, scrolling capture, rulers, color picker, etc.).
- Annotation improvements beyond the current editor MVP.
- Linux/Wayland support (design should not block it, but no implementation work yet).
- Advanced window selection UX (cycle underlying windows, search list) beyond a minimal MVP.

---

## Architecture

### Components

1) **Tauri main process (`src-tauri`)**
   - Owns tray/menu + global shortcut.
   - Starts the capture flow by launching the sidecar.
   - Performs actual capture using `xcap` and writes `last_capture.png` to the app cache dir.
   - Reveals/focuses the editor window and refreshes the frontend.

2) **Native overlay sidecar (`rsnap-overlay`)**
   - A separate executable spawned on-demand.
   - Creates one transparent, borderless, always-on-top overlay window per monitor.
   - Handles mouse + keyboard to produce a single **selection result**.
   - Exits immediately after producing a result (or cancel).

### Why a sidecar (vs in-process)

- Cross-platform GUI event loops commonly require “main-thread + single event loop per process”.
- A sidecar avoids event-loop clashes with Tauri’s runtime and provides crash isolation.

---

## Unified data contract (IPC)

### Transport

- Tauri spawns the sidecar and reads **exactly one line** of UTF-8 JSON from stdout.
- Sidecar writes one JSON message, flushes, then exits.
- Tauri enforces a timeout and kills the sidecar if it does not respond.

### Messages

All coordinates are in **global screen pixel space**, matching `xcap::Monitor::{x,y,width,height}`.

```jsonc
// cancel
{ "type": "cancel" }

// window selection
{ "type": "window", "window_id": 123456 }

// region selection
{
  "type": "region",
  "rect": { "x": 100, "y": 200, "width": 640, "height": 480 }
}

// error (sidecar only; main should treat as failure)
{ "type": "error", "message": "..." }
```

---

## Selection semantics (overlay UX)

- Default state: show crosshair cursor and (optional) a dimming mask.
- Hover:
  - Compute “topmost window under cursor” from an ordered window list and highlight its bounds.
  - Hide highlight when cursor is not inside any window bounds.
- Drag region:
  - Mouse down records start point (global pixels).
  - Mouse move updates a selection rectangle.
  - Mouse up emits `region` if the rectangle exceeds a small threshold; otherwise treat as click.
- Click window:
  - Emit `window` using the hovered window’s native id.
- Keyboard:
  - `Esc` emits `cancel`.

MVP intentionally avoids complex cycling/search; those can be added later without breaking the contract.

---

## Capture semantics (Tauri main)

### Window capture

- Enumerate windows via `xcap::Window::all()`.
- Find the window by id.
- Call `capture_image()` and save to `last_capture.png` in cache.

### Region capture

- Capture all monitors via `xcap::Monitor::all()` and `capture_image()` (parallelism optional).
- Composite monitor images into a single “virtual desktop” canvas using each monitor’s `(x, y)` origin.
- Crop using the selected `rect` (global pixels).
- Save to cache as `last_capture.png`.

### Failure handling

- Sidecar failure to start / timeout / invalid JSON:
  - Fallback to existing “primary display capture” behavior for now.
  - Log the error for diagnostics.
- Capture failure (permissions/protected content/etc.):
  - Return an error and keep the app running.

---

## Packaging and security (Tauri v2)

- Sidecar is bundled via `tauri.conf.json` `bundle.externalBin`.
- Execution uses Tauri shell plugin and should be constrained by capabilities once we introduce them.

---

## Milestones

1. Implement `rsnap-overlay` sidecar MVP: drag region + click window + cancel.
2. Integrate spawn + capture + open editor in the Tauri main process.
3. Add robust multi-monitor compositing + crop correctness.
4. Harden timeouts, error messages, and dev tooling.
5. Optional: window cycling/search, HUD details.

