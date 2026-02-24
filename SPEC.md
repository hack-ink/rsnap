# rsnap v0 Spec (Prototype)

This repository contains a pure-Rust screenshot prototype targeting macOS first, with a
cross-platform architecture.

## v0 goals (must-have)

1. Menubar-only app (no Dock icon) on macOS.
2. Global hotkey starts capture session: `Alt+X` (macOS: Option+X).
3. When the capture session overlay is visible, underlying desktop content MUST NOT be
   interactive.
4. The overlay MUST be fully transparent (no dim/black mask).
5. In live mode, the overlay MUST show a HUD near the cursor with:
   - global cursor coordinates `x,y`
   - pixel color `rgb(r,g,b)` under the cursor
6. The first prototype capture flow is:
   - Hotkey -> live transparent overlay
   - Left click -> freeze the active monitor (the monitor under the cursor) as a fullscreen
     screenshot
   - Space -> copy the frozen fullscreen screenshot PNG to the system clipboard, then exit
   - Esc -> cancel and exit without copying

## Non-goals (v0)

- Region selection (drag to select).
- Window selection (click to select a window).
- Editor UI, annotations, mosaic, saving to disk, pinning.
