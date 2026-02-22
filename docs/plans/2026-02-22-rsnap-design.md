# rsnap (Xnip-style) Design

**Goal:** Build a small, fast, Xnip-style screenshot utility for **macOS + Windows** with a tray/menubar presence and a global hotkey that opens a capture + edit flow.

**Positioning:** "Fancy, tiny, handy tool" focused on capture + annotate + copy/save/pin, with minimal platform-specific code and clean layering for future Linux support.

---

## Scope

### In scope (v1)

- Tray/menubar app
  - Tray menu: Capture, Open editor (last capture), Settings, Quit
  - Runs in background; no always-visible main window
- Global hotkey
  - Default shortcut (configurable)
  - Press hotkey to start capture flow
- Capture flow
  - Capture the current display (full-screen capture)
  - Open editor window with the captured image
  - Crop inside the editor (region capture via crop UI)
- Editor (initial feature set)
  - Crop rectangle with handles
  - Basic annotation tools: pen, rectangle, arrow, text (minimal)
  - Undo/redo (minimal)
  - Export actions: copy to clipboard, save to file (PNG), "Pin" (always-on-top)
- Settings
  - Hotkey configuration
  - Save location and file naming template (timestamp-based default)
  - Toggle: auto-copy / auto-save (defaults off)

### Not in scope (v1)

- Wayland/Linux support
- True "drag to select region" overlay capture (outside editor)
- Scrolling capture (stitching)
- OCR, screen ruler, color picker, GIF/video capture, cloud sync
- Full parity with Xnip’s toolset

---

## Architecture

### Key decision: Tauri v2 app shell

- Use **Tauri v2** for tray, global shortcuts, windows, and packaging.
- Use a web UI for the editor (Canvas-based) to accelerate "fancy" interactions and keep Rust UI code small.
- Keep platform-specific capture code behind a Rust trait; prefer a cross-platform crate when possible.

### Components

1. **`src-tauri` (Rust backend)**
   - App lifecycle: single instance, tray/menu, hotkey registration
   - Capture pipeline: capture screen -> encode PNG -> store to app cache
   - IPC commands: `capture_now`, `get_last_capture`, `save_capture`, `copy_capture`, `pin_capture`
   - Settings persistence
2. **Frontend editor (web UI)**
   - Loads last capture from backend (as bytes or file URL)
   - Implements crop + annotation tools
   - Produces an edited image (PNG bytes) for export via backend commands

### Data flow

1. User presses global hotkey.
2. Backend captures full screen and writes a PNG to app cache (and/or keeps bytes in memory).
3. Backend opens the editor window (or focuses it).
4. Frontend loads the screenshot and edits it.
5. Frontend exports edited PNG bytes to backend.
6. Backend performs copy/save/pin operations.

---

## Platform-specific considerations

### macOS

- Full screen capture requires Screen Recording permission on modern macOS when using capture APIs.
- Keep all macOS-specific behavior isolated to the capture backend and permission/error reporting.

### Windows

- Capture depends on a Windows capture API and/or a library that abstracts it.
- Runtime constraints (e.g., WebView2) are handled by Tauri packaging; still provide a clear error if not available.

---

## Persistence and file locations

- **Settings:** stored in the platform config directory.
- **Cache:** last capture stored in the platform cache/data directory.
- **Saved screenshots:** default to the user’s Pictures directory (configurable).

---

## Error handling and UX

- If capture fails, show a small error dialog and log the underlying error.
- If editor window fails to load, keep the app running and allow retry from tray menu.
- Never silently discard a capture: keep "last capture" available until replaced.

---

## Security / privacy

- Default posture: local-only; no network calls.
- No telemetry.
- Ensure logs do not include screenshot data.

---

## Milestones (high level)

1. Tauri skeleton: tray + global hotkey + settings persistence
2. Cross-platform full-screen capture -> open editor with image
3. Editor MVP: crop + basic tools + export (copy/save)
4. Pin window (always-on-top)

