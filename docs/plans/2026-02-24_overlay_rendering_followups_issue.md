# Issue: Cross-Platform Alpha-Capable Overlay Rendering (Follow-up)

**Summary:** The current overlay renderer (`softbuffer`) does not support per-pixel alpha. This
forces the UX to use per-window alpha, which dims the entire screen. The short-term macOS solution
is to render borders via CoreAnimation layers, but long-term we may want a unified cross-platform
renderer that supports true transparent backgrounds with opaque borders and richer effects.

---

## Problem

- `softbuffer` pixel format is `00000000RRRRRRRRGGGGGGGGBBBBBBBB`; alpha is not available.
- Any translucency must be achieved via whole-window opacity, which scales all content equally.
- This prevents a true "0% dim" full-screen overlay while still rendering opaque UI elements.

---

## Proposed follow-ups

### Option 1: Adopt `pixels`/wgpu for the overlay renderer

- Replace the `softbuffer` renderer with a GPU-backed surface that supports alpha blending.
- Use per-pixel alpha to keep the background transparent and render hover/drag affordances with
  full opacity.
- Enables richer effects (gradients, blur/glow) consistently on macOS + Windows.

**Complexity:** medium-high (new dependencies, renderer integration, platform quirks).

### Option 2: Keep platform-native renderers

- macOS: CoreAnimation layers (as in the chosen near-term design).
- Windows: DWM/DirectComposition or `UpdateLayeredWindow` for per-pixel alpha.

**Complexity:** high (two native implementations).

---

## Acceptance criteria (when pursued)

- Cross-platform overlay can render:
  - fully transparent background (no dim),
  - fully opaque hover/drag borders,
  - smooth animations at 60fps without high CPU.
- Overlay intercepts clicks reliably (including menubar/taskbar equivalents as applicable).

