# macOS Native Capture Window Layer

Purpose: Describe the current macOS-native capture window layer that owns the single AppKit root
window topology plus passive pointer input and explicit keyboard-focus routing for rsnap capture
sessions.

Read this when: You are changing macOS overlay-window behavior, focus/activation handling, IME
support, or the app-shell boundary that wakes overlay input.

Inputs: `docs/spec/capture-session.md`, `packages/rsnap-overlay/src/overlay.rs`,
`packages/rsnap-overlay/src/overlay/macos_native_capture_shell_runtime.rs`,
`apps/rsnap/src/app/capture.rs`

Depends on: `docs/spec/capture-session.md`, `docs/reference/workspace-layout.md`

Covers: The current single-root AppKit capture topology, the passive pointer shells, the explicit
key-focus shell, and the app-shell event boundary that bridges native input back into Rust.

Spec boundary: `docs/spec/capture-session.md`

## Current model

The Rust overlay session still owns capture state, display/export authority, worker coordination,
rendering, and output flow. On macOS, window activation and pointer/IME routing are no longer
owned exclusively by `winit` windows.

Instead, rsnap now uses a single native AppKit root-owner topology:

- A single transparent AppKit root-owner window tracks the union frame of the active capture
  surfaces and owns the native child-window topology.
- The mirrored `winit` overlay, HUD, loupe, toolbar, and scroll-preview windows attach under that
  root owner as child windows and remain the render surfaces.
- Passive AppKit shells mirror live overlay windows and the frozen toolbar for pointer input.
- A dedicated key-focus AppKit shell appears only when Frozen text editing or scroll capture
  needs keyboard ownership.
- Rust remains the source of truth for session state and input handling; the native shells only
  gather platform events and wake the overlay session.

## Single native root owner

The root owner exists so macOS capture windows no longer behave like unrelated top-level surfaces.

Current behavior:

- The root owner is a transparent non-activating AppKit window created from the first live capture
  surface and kept in sync with the union of the active overlay/HUD/loupe/toolbar/scroll-preview
  frames.
- Overlay render windows, auxiliary HUD windows, passive pointer shells, and the key-focus shell
  attach under the same root owner through AppKit child-window ownership.
- Window creation, deferred startup auxiliary-window creation, prewarm discard, and session exit
  all rebuild or tear down that topology from `OverlaySession` instead of leaving orphan native
  windows behind.

## Passive pointer shells

The passive shell layer exists to keep live capture, frozen entry, and toolbar pointer
interaction from activating rsnap or stealing `key/main` window status from the target app.

Current behavior:

- Live overlay pointer movement and left-click selection come from passive AppKit shells.
- Frozen toolbar pointer move, drag, hover, and scroll-wheel input also come from passive shells.
- The mirrored `winit` windows remain the render surfaces under the root owner, but they are not
  the macOS pointer interaction authority for these flows.

This is the layer that replaced the older per-window `winit` focus-policy patching path.

## Explicit key-focus shell

Keyboard ownership is now explicit and scoped:

- Frozen text editing uses a dedicated key-focus shell so ASCII input, command keys, and IME
  composition can flow without turning the pointer shells into key windows.
- Scroll capture uses the same key-focus shell pattern for `Esc`, `Space`, save shortcuts, and
  pause/undo controls.
- During scroll capture that key-focus shell stays nonactivating while the full-screen frozen
  overlay windows switch into mouse passthrough, so the target app keeps receiving the real
  scroll gesture.
- When neither text editing nor scroll capture needs keyboard ownership, no capture window should
  remain key-capable.

The key-focus shell is AppKit-owned and implements the minimal responder and `NSTextInputClient`
surface needed for rsnap's current text/IME needs. It also lives under the same native root owner
instead of floating as an unrelated top-level capture window.

## App-shell wake boundary

Native AppKit input does not mutate session state directly. The app shell owns the wake path:

- `rsnap-overlay` enqueues passive-shell events in its native capture input queue.
- `apps/rsnap` installs a native-capture-input waker on session creation.
- The app shell coalesces wakeups into `UserEvent::OverlayNativeCaptureInput`.
- The overlay session drains the queue on the main event loop and applies the events through the
  same Rust input handlers used by the rest of the session.

This keeps the platform boundary at the event-delivery layer instead of pushing session mutations
into AppKit callbacks.

## What still stays in Rust

The native window layer does not own:

- capture-session state machines
- display/export authority
- frozen editing behavior
- scroll-stitching logic
- PNG/save/OCR/export decisions
- toolbar layout rules

Those remain in `rsnap-overlay` and are shared with the non-macOS paths.
