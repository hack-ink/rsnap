# macOS No-Dim Overlay + Fancy Hover Border Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make the macOS capture overlay fully non-dimming (no gray mask) while providing clear
capture-mode affordance via a crosshair cursor and a fancy animated hover border, and ensure the
overlay intercepts all clicks including the menubar.

**Architecture:** Keep `rsnap-overlay` as the native sidecar driven by winit input + xcap hit-test.
On macOS, stop using `softbuffer` to render a full-screen mask (no alpha support). Instead, install
and update CoreAnimation layers (`CAShapeLayer`) on the winit window's underlying `NSView` to render
hover/drag borders over a truly transparent window background.

**Tech Stack:** Rust, winit, xcap, objc (AppKit/CoreAnimation), existing overlay protocol.

**Commit cadence:** This repository requires running full pre-commit gates before committing. To
keep iteration fast, prefer **one commit per batch (default: 3 tasks)** rather than committing
after every 2–5 minute step.

---

### Task 1: Add macOS layer renderer scaffolding

**Files:**
- Modify: `crates/rsnap-overlay/src/main.rs`

**Step 1: Add a macOS-only renderer struct**
- Add a `#[cfg(target_os = "macos")]` struct that owns the underlying `NSView`/`NSWindow` pointers
  needed to manage layers.
- Keep non-macOS code paths unchanged.

**Step 2: Compile**
- Run: `cargo make test`
- Expected: PASS

**Step 3: Stage (commit deferred to batch end)**
- Run: `git add crates/rsnap-overlay/src/main.rs`

---

### Task 2: Configure macOS overlay window for “no dim” + click interception

**Files:**
- Modify: `crates/rsnap-overlay/src/main.rs`

**Step 1: Stop using per-window alpha dimming on macOS**
- Replace the current `setAlphaValue` usage with:
  - `setOpaque: false`
  - clear background color
  - keep `alphaValue` at `1.0`

**Step 2: Ensure overlay is above the menubar**
- Set `NSWindow` level high enough that menubar clicks are intercepted while the overlay is active.
- Keep the existing collection behavior (join all spaces, full-screen auxiliary).

**Step 3: Manual verification (macOS)**
- Trigger capture.
- Try clicking menubar items: they MUST NOT activate.
- Expected: overlay stays active until selection/cancel.

**Step 4: Commit**
- (deferred to batch end)

---

### Task 3: Set crosshair cursor during overlay lifetime (macOS)

**Files:**
- Modify: `crates/rsnap-overlay/src/main.rs`

**Step 1: Set cursor icon**
- On window creation, call `window.set_cursor_icon(CursorIcon::Crosshair)`.

**Step 2: Manual verification**
- Trigger capture: cursor MUST become crosshair when over the overlay.

**Step 3: Commit**
- (deferred to batch end)

---

### Task 4: Implement fancy hover border (animated)

**Files:**
- Modify: `crates/rsnap-overlay/src/main.rs`

**Step 1: Add hover `CAShapeLayer`**
- Stroke-only rect path, with glow/shadow.
- Dashed line pattern.
- Add a repeating animation (e.g. animate `lineDashPhase`) for motion.

**Step 2: Update layer path on hover changes**
- Use existing `xcap` hit-test rect (global pixels) translated into per-monitor window coordinates.
- Convert from overlay’s top-left coordinate system to CoreAnimation’s coordinate system if needed.

**Step 3: Manual verification**
- Hover different windows: border follows the topmost window under cursor.
- Animation is visible and smooth.

**Step 4: Commit**
- (deferred to batch end)

---

### Task 5: Implement drag region border

**Files:**
- Modify: `crates/rsnap-overlay/src/main.rs`

**Step 1: Add drag `CAShapeLayer`**
- Distinct stroke style (e.g. solid white).
- Show only while dragging; hide hover border while dragging.

**Step 2: Manual verification**
- Drag-select region: selection rectangle updates live and is clear.

**Step 3: Commit**
- (deferred to batch end)

---

### Task 6: Keep non-macOS behavior stable

**Files:**
- Modify: `crates/rsnap-overlay/src/main.rs`

**Step 1: Verify non-macOS build still compiles**
- Run: `cargo make test`
- Expected: PASS

**Step 2: Commit**
- (deferred to batch end)

---

### Task 7: Update current system spec (behavior change)

**Files:**
- Modify: `docs/spec/system_rsnap_current.md`

**Step 1: Update overlay UX description**
- Note: macOS overlay does not dim the screen; uses cursor + animated hover border.
- Note: overlay intercepts clicks including menubar while active.

**Step 2: Commit**
- `cargo make lint-fix`, `cargo make fmt`, `cargo make test`
- `git commit -m '<cmsg/1 JSON>'`

---

### Batch commit checklist (run at the end of each batch)

1. Ensure working tree is staged as intended: `git status --porcelain`
2. Run repo gates:
   - `cargo make lint-fix`
   - `cargo make fmt`
   - `cargo make test`
3. Commit with a `cmsg/1` JSON message.
