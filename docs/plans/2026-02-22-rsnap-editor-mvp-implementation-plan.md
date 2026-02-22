# rsnap Editor MVP Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** After tray/hotkey capture, show an editor window that displays the captured screenshot, supports crop selection, and can copy/save the result.

**Architecture:** Keep capture in Rust (Tauri backend) and keep editor interactions in the web UI (Canvas). Backend exposes small commands (capture/read/save/copy/pin) and window-management helpers. Frontend stays framework-free (Vite + TS).

**Tech Stack:** Tauri v2 (Rust), Vite + TypeScript, `xcap` (capture), `base64` + `arboard` + `image` (export/copy).

---

## Task 1: Backend commands + window management (Rust)

**Files:**
- Modify: `src-tauri/src/main.rs:1`
- Create: `src-tauri/src/commands.rs`
- Create: `src-tauri/src/export.rs`
- Modify: `src-tauri/Cargo.toml:1`
- Modify: `src-tauri/tauri.conf.json:1`

**Step 1: Add Tauri commands**

- `capture_now() -> Result<(), String>`: capture primary display to cache (`last_capture.png`).
- `get_last_capture_base64() -> Result<String, String>`: read cache PNG and return base64.
- `save_png_base64(png_base64: String) -> Result<String, String>`: write to Downloads (timestamp-based filename), return path.
- `copy_png_base64(png_base64: String) -> Result<(), String>`: decode PNG and copy image to clipboard.
- `open_pin_window() -> Result<(), String>`: open an always-on-top window in “pin mode”.

**Step 2: Show editor window on capture**

- On tray/hotkey capture: ensure the main window becomes visible and focused.
- Keep it simple: call `eval("window.location.reload()")` after capture so the UI re-reads the latest capture.

**Step 3: Config**

- Ensure main window has a stable label (e.g., `main`) and starts hidden (`visible: false`) to behave like a tray app.

**Step 4: Verification**

Run: `cargo make checks`
Expected: PASS

---

## Task 2: Editor UI MVP (Frontend)

**Files:**
- Modify: `ui/package.json:1`
- Modify: `ui/src/main.ts:1`
- Create: `ui/src/api.ts`
- Create: `ui/src/editor.ts`

**Step 1: Add Tauri JS API**

- Add `@tauri-apps/api` v2 dependency.
- `api.ts` wraps `invoke` calls for the backend commands.

**Step 2: Display last capture**

- On load: call `get_last_capture_base64()`.
- Render to `<img>` or `<canvas>` as `data:image/png;base64,...`.

**Step 3: Crop selection**

- Implement a simple drag-to-select rectangle overlay on a canvas.
- “Export Cropped” builds a cropped PNG (Canvas) and produces base64.

**Step 4: Buttons**

- `Capture Now` (calls backend `capture_now`, then refresh image)
- `Save` (calls backend `save_png_base64`)
- `Copy` (calls backend `copy_png_base64`)
- `Pin` (calls backend `open_pin_window`)

**Step 5: Verification**

Run:
- `cd ui && npm install`
- `cd ui && npm run build`
Expected: PASS

---

## Task 3: Integration smoke test (manual)

Run:
- Terminal A: `cd ui && npm run dev`
- Terminal B: `cd src-tauri && cargo tauri dev`

Manual expected:
- App runs as a tray app.
- Tray “Capture Now” shows the editor window.
- Editor displays the new capture.
- Save writes a PNG file; Copy puts an image on clipboard.

