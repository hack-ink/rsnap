# Native Overlay Sidecar Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a native capture overlay as a sidecar process (`rsnap-overlay`) and integrate it into the existing Tauri app so the global hotkey/tray triggers an Xnip-style flow: **click window** or **drag region** → capture → open editor.

**Architecture:** Keep the Tauri app for tray/hotkey/editor. Add a Rust sidecar binary using `winit` that outputs a single JSON selection message to stdout. The Tauri main process reads the selection, performs capture via `xcap`, stores `last_capture.png`, and reveals the editor.

**Tech Stack:** Rust, Tauri v2, `xcap`, `winit`, `serde`, `image`.

---

### Task 1: Define the IPC contract types

**Files:**
- Create: `crates/rsnap-overlay-protocol/Cargo.toml`
- Create: `crates/rsnap-overlay-protocol/src/lib.rs`
- Test: `crates/rsnap-overlay-protocol/src/lib.rs`

**Step 1: Write a serde roundtrip unit test**

- Add a unit test that roundtrips `CaptureSelection::Window`, `::Region`, `::Cancel`.

**Step 2: Implement the protocol**

- Define `CaptureSelection` enum and `RectI32` struct.
- Use `serde` with `#[serde(tag = "type")]` to match the JSON in the design doc.

**Step 3: Run unit tests**

- Run: `cargo test -p rsnap-overlay-protocol`
- Expected: PASS

---

### Task 2: Add the sidecar binary crate skeleton

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/rsnap-overlay/Cargo.toml`
- Create: `crates/rsnap-overlay/src/main.rs`

**Step 1: Add workspace members**

- Add `crates/rsnap-overlay-protocol` and `crates/rsnap-overlay` to the workspace.

**Step 2: Implement a minimal overlay app**

- Create a transparent, borderless, always-on-top window on the primary monitor.
- Handle `Esc` to emit `cancel` JSON to stdout and exit.

**Step 3: Manual run**

- Run: `cargo run -p rsnap-overlay`
- Expected: A full-screen overlay appears; `Esc` closes it and prints JSON to stdout.

---

### Task 3: Implement region selection (drag) + JSON output

**Files:**
- Modify: `crates/rsnap-overlay/src/main.rs`

**Step 1: Track mouse down/move/up**

- Store start/end points in global pixel coordinates.
- When drag exceeds a small threshold, emit `region`.

**Step 2: Draw selection rectangle**

- Render a dim mask and a rectangle border.
- Repaint on mouse events.

**Step 3: Manual verification**

- Run: `cargo run -p rsnap-overlay`
- Expected: Dragging draws a rectangle; releasing prints `region` JSON and exits.

---

### Task 4: Implement window hover highlight + click selection

**Files:**
- Modify: `crates/rsnap-overlay/src/main.rs`

**Step 1: Enumerate windows**

- Use `xcap::Window::all()` to get an ordered list.
- Hit-test by cursor point against window bounds; pick the first match.

**Step 2: Click selection**

- On mouse click (without drag), emit `window` with `window_id`.

**Step 3: Manual verification**

- Expected: Hover shows a highlight rectangle; click prints `window` JSON and exits.

---

### Task 5: Integrate spawning the sidecar from Tauri

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/commands.rs`

**Step 1: Add a backend function `start_capture_flow_with_app`**

- Replace the hotkey handler and tray “Capture Now” to call this.

**Step 2: Spawn sidecar**

- Spawn `rsnap-overlay` in dev via a resolved path (initially: workspace `target/debug`).
- Read one line JSON (with timeout).
- Parse into `CaptureSelection`.

**Step 3: Fallback behavior**

- If sidecar fails, fallback to the existing `capture_primary_display_to_cache` path.

---

### Task 6: Implement capture for window + region in `src-tauri`

**Files:**
- Modify: `src-tauri/src/capture.rs`
- Modify: `src-tauri/src/commands.rs`

**Step 1: Window capture**

- Implement `capture_window_to_cache(app, window_id)`.

**Step 2: Region capture**

- Implement `capture_region_to_cache(app, rect)` using multi-monitor compositing + crop.

**Step 3: Manual flow**

- Run the app (dev).
- Expected: hotkey → overlay → selection → editor shows selected result.

---

### Task 7: Wire up Tauri v2 sidecar bundling (packaging)

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Create: `src-tauri/bin/.gitkeep`
- Create/Modify: `src-tauri/build.rs`

**Step 1: Add `bundle.externalBin`**

- Configure `externalBin` for `rsnap-overlay` (per target triple naming).

**Step 2: Build-time copy**

- Update `src-tauri/build.rs` to copy the built sidecar into `src-tauri/bin/` with the expected name.

**Step 3: Smoke build**

- Run: `cargo tauri build`
- Expected: Bundle contains sidecar binary.

---

### Task 8: Verification

**Files:**
- (none)

**Step 1: Run repo checks**

- Run: `cargo make checks`
- Expected: PASS

**Step 2: UI build**

- Run: `cd ui && npm run build`
- Expected: PASS

