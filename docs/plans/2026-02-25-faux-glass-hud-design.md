# Faux Glass HUD (A) — Design

Date: 2026-02-25

## Context

`rsnap` renders a small HUD "pill" in a transparent, always-on-top overlay window during capture
mode. The HUD currently uses a left-side accent (sampled color) implemented as a separate
shape/layer. On high-contrast backgrounds (especially white), the left edge shows visible
artifacts (a stray bright bar / misaligned block).

The user also wants a more "Apple liquid glass" feel. True liquid glass relies heavily on
backdrop blur; however, this phase (A) is explicitly a blur-free approximation.

## Goals

- Make the HUD pill feel like "glass" (tint + highlight + crisp strokes), without backdrop blur.
- Remove the visible left-edge artifact on white/bright backgrounds.
- Keep performance excellent (no heavy GPU textures, no large offscreen rendering).
- Keep cross-platform compatibility (no macOS-only APIs in this phase).

## Non-Goals

- Real-time backdrop blur / vibrancy (macOS `NSVisualEffectView`), which requires a separate
  native view/window and platform glue (deferred to phase C).
- Reworking capture flow / hotkeys / freeze logic.
- Introducing a full theme system.

## Constraints / Invariants

- Overlay window remains transparent and always-on-top.
- HUD is rendered via egui/wgpu.
- `cargo make checks` must remain green (clippy `-D warnings`, vstyle curate, tests, fmt checks).

## Proposed Approach (A: Faux Glass)

### 1) Remove the left accent strip

Remove the left accent strip entirely from the pill background. The sampled color remains visible
via the small swatch in the HUD row (and optionally future loupe/recent chips).

Rationale:
- The accent strip provides little additional information beyond the swatch.
- It is the primary source of edge artifacts due to antialiasing + rounded corners + translucent
  overlays.

### 2) Render a glass-like pill background

Render the pill as a single rounded rect background with:

- **Tint**: a neutral semi-transparent fill (fixed, not dependent on sampled RGB).
- **Specular highlight**: a subtle top overlay (approximate gradient using layered translucent
  rounded rect(s) clipped to the top portion of the pill).
- **Bottom shade**: a subtle bottom overlay to add depth.
- **Double stroke**: a thin outer light stroke and an inner dark stroke to improve definition on
  both light and dark backgrounds.
- **Shadow**: small, tight drop shadow (avoid "blob" artifacts below the pill).

This is intended to approximate "liquid glass" without blur.

### 3) Keep color conveyance practical

- Swatch uses sampled color (already implemented).
- Text remains monospace for scanning and stability.

## Acceptance Criteria

- On a pure white background, the pill has no stray bar / misaligned bright block on its left edge.
- The pill feels more "glass-like" (highlight + crisp stroke visible but subtle).
- No new UI jitter/regressions.
- `cargo make checks` passes.

## Follow-ups (Phase C: macOS native glass)

If faux glass is insufficient, phase C will introduce a separate small HUD window on macOS using
native vibrancy/blur (e.g. `NSVisualEffectView`). The fullscreen overlay window remains responsible
for input capture and crosshair behavior; the HUD window is just the glass pill.

