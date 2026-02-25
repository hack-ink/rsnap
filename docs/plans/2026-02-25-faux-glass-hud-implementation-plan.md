# Faux Glass HUD (A) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Restyle the capture HUD pill to a blur-free "faux glass" look and remove the left-edge accent artifact.

**Architecture:** Keep the existing egui HUD layout but replace the pill background rendering with a single fixed glass tint plus subtle highlight/shade overlays and a double stroke. Remove the left accent strip; sampled color is conveyed via the swatch only.

**Tech Stack:** Rust, egui, wgpu, winit, cargo-make, vstyle.

---

### Task 1: Remove left accent strip + adopt a single glass base

**Files:**
- Modify: `packages/rsnap-overlay/src/overlay.rs`

**Step 1: Implement the faux glass base**

- Update `WindowRenderer::render_hud_frame` to:
  - Render a single pill frame with a fixed semi-transparent tint (does not depend on sampled RGB).
  - Remove the left accent strip / outer accent layer.
  - Keep existing padding, min width, and content rendering.

**Step 2: Add glass overlays (subtle, readability-first)**

- Add a top "specular" overlay and bottom shade using additional translucent rounded rect(s) painted over the pill with low alpha.
- Add a double stroke: keep an outer light stroke in the frame, then paint an inner darker stroke on top.

**Step 3: Run verification**

Run: `cargo make checks`
Expected: success (clippy `-D warnings`, vstyle curate, tests, fmt-check all pass).

**Step 4: Manual check**

Run: `cargo run -p rsnap`
Expected:
- On white and black backgrounds, the left edge shows no stray bar / misaligned bright block.
- The pill reads as "glass" (tint + highlight + crisp edge) without obscuring text.

**Step 5: Commit + push**

- Use `cmsg/1` JSON commit message.
- Push to `origin/main`.

### Task 2: Tuning pass (constants only)

**Files:**
- Modify: `packages/rsnap-overlay/src/overlay.rs`

**Step 1: Adjust constants**

- Tune tint alpha, highlight alpha, border alphas, and shadow parameters based on quick manual review.
- Keep changes minimal and localized.

**Step 2: Run verification**

Run: `cargo make checks`
Expected: success.

**Step 3: Commit + push**

- Use `cmsg/1` JSON commit message.
- Push to `origin/main`.

