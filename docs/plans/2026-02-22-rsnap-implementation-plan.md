# rsnap Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Tauri v2 tray/menubar screenshot utility (macOS + Windows) with a global hotkey to capture, then crop/annotate, then copy/save/pin.

**Architecture:** Tauri v2 app shell (tray + hotkey + windows) with a Canvas-based editor frontend. Rust backend owns capture, persistence, and export actions. Platform-specific code is isolated; prefer cross-platform crates.

**Tech Stack:** Rust (Tauri v2), TypeScript (Vite), cross-platform capture crate (candidate: `xcap`), clipboard crate (candidate: `arboard`).

---

## Task 1: Rename and restructure repo into a Tauri layout

**Files:**
- Modify: `Cargo.toml`
- Move: `src/main.rs` -> `src-tauri/src/main.rs`
- Move: `src/cli.rs` -> `src-tauri/src/cli.rs` (temporary; later delete if unused)
- Move: `build.rs` -> `src-tauri/build.rs`
- Modify: `Makefile.toml`
- Create: `src-tauri/Cargo.toml`

**Step 1: Write a smoke check**

Run: `cargo build`
Expected: PASS (baseline)

**Step 2: Create root workspace**

- Convert root `Cargo.toml` to a `[workspace]` with `members = ["src-tauri"]`.
- Create `src-tauri/Cargo.toml` for the Tauri app crate.

**Step 3: Run build**

Run: `cargo build`
Expected: PASS

---

## Task 2: Add Tauri v2 config and minimal frontend

**Files:**
- Create: `src-tauri/tauri.conf.json`
- Create: `ui/package.json`
- Create: `ui/vite.config.ts`
- Create: `ui/index.html`
- Create: `ui/src/main.ts`

**Step 1: Minimal frontend**

- Use Vite + TypeScript (no framework) for a small dependency surface.

**Step 2: Wire Tauri build config**

- `devUrl` -> Vite dev server URL
- `frontendDist` -> `../ui/dist`

**Step 3: Verify dev boot**

Run (two terminals):
- `cd ui && npm install`
- `cd ui && npm run dev`
- `cd src-tauri && cargo tauri dev`

Expected: App launches with a blank editor page.

---

## Task 3: Implement tray/menubar + quit/settings menu

**Files:**
- Modify: `src-tauri/src/main.rs`
- Create: `src-tauri/src/tray.rs`

**Step 1: Implement tray menu**

- Items: Capture Now, Settings, Quit
- Clicking Quit exits the process.

**Step 2: Manual verification**

- Launch app
- Tray icon appears
- Quit works

---

## Task 4: Implement global hotkey to trigger capture

**Files:**
- Modify: `src-tauri/src/main.rs`
- Create: `src-tauri/src/hotkeys.rs`
- Create: `src-tauri/src/settings.rs`

**Step 1: Persist settings**

- Store hotkey in a config file; default to a safe shortcut.

**Step 2: Hotkey registration**

- Register global hotkey on startup
- Re-register on settings change

**Step 3: Manual verification**

- Hotkey triggers the same handler as tray "Capture Now"

---

## Task 5: Implement capture pipeline (full screen -> cached PNG)

**Files:**
- Create: `src-tauri/src/capture/mod.rs`
- Create: `src-tauri/src/capture/xcap.rs`
- Create: `src-tauri/src/state.rs`

**Step 1: Add capture backend**

- Implement `capture_fullscreen_png() -> Vec<u8>` via a cross-platform crate (candidate: `xcap`).
- Store `last_capture.png` in app cache dir.

**Step 2: Add a small unit test (non-GUI)**

- Test file naming / cache path logic (do not test OS capture).

**Step 3: Manual verification**

- Trigger capture
- Verify cache file exists and is a valid PNG

---

## Task 6: Open/focus editor window with the last capture

**Files:**
- Modify: `src-tauri/src/main.rs`
- Create: `src-tauri/src/windows.rs`
- Create: `src-tauri/src/commands.rs`
- Modify: `ui/src/main.ts`

**Step 1: Backend command**

- `get_last_capture()` returns bytes (base64) or a file URL.

**Step 2: Frontend display**

- Render the image on a canvas.

**Step 3: Manual verification**

- Capture opens editor with screenshot visible.

---

## Task 7: Crop UI in editor and export cropped PNG back to backend

**Files:**
- Modify: `ui/src/main.ts`
- Create: `ui/src/crop.ts`
- Modify: `src-tauri/src/commands.rs`

**Step 1: Implement crop rectangle**

- Mouse drag to select
- Handles for resize (minimal)

**Step 2: Export**

- Produce cropped PNG bytes in the browser (Canvas) and send to backend.

**Step 3: Manual verification**

- Crop and save produces the cropped image.

---

## Task 8: Save/copy actions

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Create: `src-tauri/src/export.rs`
- Modify: `ui/src/main.ts`

**Step 1: Save to file**

- Timestamp-based filename default
- Use OS file picker later; for v1 save to configured folder

**Step 2: Copy to clipboard**

- Use a cross-platform clipboard crate (candidate: `arboard`).

**Step 3: Manual verification**

- Copy -> paste into an image editor works
- Save -> file exists and opens

---

## Task 9: Pin window (always-on-top image)

**Files:**
- Create: `ui/src/pin.ts`
- Modify: `src-tauri/src/windows.rs`
- Modify: `src-tauri/src/commands.rs`

**Step 1: Create pin window**

- New window with image, always-on-top, minimal chrome

**Step 2: Manual verification**

- Pin stays on top and can be closed independently

---

## Task 10: Verification and documentation polish

**Files:**
- Modify: `README.md`
- Modify: `docs/index.md` (optional link to plans)

**Step 1: Run repo checks**

Run: `cargo make checks`
Expected: PASS

**Step 2: Basic usage docs**

- Hotkey default
- How to quit (tray menu)
- Where files are saved

---

## Notes / risks

- macOS screen capture may require Screen Recording permission; ensure errors guide the user to enable it.
- Windows capture + WebView2 runtime must be handled gracefully; prefer clear error dialogs.

