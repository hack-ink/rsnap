# macOS Overlay: No-Dim Mode + Fancy Hover Border (Design)

**Goal:** On macOS, the capture overlay MUST NOT dim the screen. Entering capture mode MUST be
indicated by a cursor style change plus a "fancy" animated hover border. While the overlay is
active, it MUST intercept all clicks (including the menubar) so the user cannot interact with
underlying UI.

**Context:** Current overlay implementation lives in `crates/rsnap-overlay`. It uses `softbuffer`,
whose pixel format does not support alpha (highest 8 bits are required to be `0`). The current
implementation achieves translucency via per-window alpha (`setAlphaValue` on macOS), which
necessarily dims the entire screen.

---

## Requirements

### R1: No screen dim (macOS)

- The overlay MUST NOT apply a uniform dim/gray mask.
- The overlay SHOULD keep the underlying desktop visually unchanged.

### R2: Capture mode affordance

- While overlay is active, cursor MUST change (e.g. crosshair).
- Hovering a window MUST show an animated border around that window bounds.
- Dragging a region MUST show a region rectangle border.

### R3: Intercept all clicks (including menubar)

- While overlay is active, clicks MUST NOT reach underlying apps or the menubar.
- Clicking on the menubar area MUST NOT trigger menubar buttons.
- Overlay exit MUST be driven by overlay input only (`Esc`, click-select window, drag-select region).

---

## Options considered

### Option A: Keep `softbuffer` + reduce window alpha

Keep per-window alpha but make it extremely small.

- Pros: Minimal code churn.
- Cons: Border visibility scales with the same alpha (hard to make border "fancy" while keeping
  dim imperceptible). This does not satisfy the "0% dim" requirement reliably.

### Option B (Chosen): macOS native border rendering via CoreAnimation layers

Keep winit for window creation + input, but on macOS:

- Do not rely on per-window alpha to see through the overlay.
- Render hover/drag borders using `CALayer` / `CAShapeLayer` on the underlying `NSView`.
- Animate using CoreAnimation (e.g. `lineDashPhase`, shadow/glow) without a continuous redraw loop.

- Pros: True no-dim background; high-quality border animation; low CPU.
- Cons: macOS-specific code path; more careful coordinate mapping needed.

### Option C: Replace renderer with wgpu/pixels cross-platform

- Pros: Unified alpha-capable rendering.
- Cons: Higher dependency + complexity; larger migration.

---

## Proposed design (Option B)

### Architecture changes

- Keep the current process model: sidecar `rsnap-overlay` is spawned on-demand and exits after
  emitting one selection JSON line.
- Split rendering by platform:
  - macOS: `RendererMacosLayers` that installs and updates CoreAnimation layers on the `NSView`.
  - non-macOS: keep existing `softbuffer` renderer (can remain dimmed for now).

### macOS window configuration

When each overlay `winit` window is created:

- Ensure the underlying `NSWindow`:
  - is non-opaque and uses a clear background.
  - does NOT use reduced alpha (`alphaValue` remains at 1.0).
  - is in a level above the menubar so it intercepts clicks in the menubar area.
  - joins all spaces and can appear above full-screen apps (retain existing collection behavior).

### Input + state

Keep the existing hit-testing logic via `xcap::Window::all()` and selection state machine:

- Hover state: best z-ordered window under the cursor (excluding self).
- Drag state: mouse down + threshold -> region rectangle.
- Commit:
  - click -> window selection
  - drag release -> region selection
  - `Esc` -> cancel

Cursor:

- On macOS, set cursor icon to `Crosshair` for the overlay windows while the overlay is active.

### Fancy border animation (hover)

Use a `CAShapeLayer`:

- stroke color: a bright accent (e.g. green/cyan) with a subtle outer glow (shadow).
- line width: 2px–3px (in points), configurable.
- dashed border: `lineDashPattern` with an animated `lineDashPhase` to create motion.
- optional: add an additional blurred outer layer to simulate a glow "aura".

The layer path MUST be updated on each hover rect change, mapped to the overlay window's
coordinate system.

### Region border (drag)

Use a separate `CAShapeLayer` for drag selection:

- solid stroke (white) or a distinct accent.
- no dash (or a slower dash) to keep region selection clear.

### Coordinate mapping

The current logic already translates global rects into per-monitor overlay coordinates. For macOS
layer rendering, map that rect into the `NSView` coordinate system. If the view is not flipped,
convert top-left origin into bottom-left origin by:

- `y_layer = view_height - (rect.y + rect.height)`

### Verification (manual)

On macOS:

1. Trigger capture.
2. Confirm no dim/gray mask.
3. Confirm cursor becomes crosshair.
4. Hover over windows: see animated border.
5. Hover/click in menubar area: underlying menubar MUST NOT respond.
6. Select window and region; confirm correct capture output.

Automated tests are not expected for AppKit UI behavior; rely on existing Rust unit tests plus
manual verification.

