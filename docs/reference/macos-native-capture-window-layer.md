# macOS Native Capture Window Layer

Purpose: Describe the current macOS-native window shell that now owns passive pointer input and
explicit keyboard-focus routing for rsnap capture sessions.

Read this when: You are changing macOS overlay-window behavior, focus/activation handling, IME
support, or the app-shell boundary that wakes overlay input.

Inputs: `docs/spec/capture-session.md`, `packages/rsnap-overlay/src/overlay.rs`,
`packages/rsnap-overlay/src/overlay/macos_native_capture_shell_runtime.rs`,
`apps/rsnap/src/app/capture.rs`

Depends on: `docs/spec/capture-session.md`, `docs/reference/workspace-layout.md`

Covers: The current passive AppKit shell model, the explicit key-focus shell, and the app-shell
event boundary that bridges native input back into Rust.

Spec boundary: `docs/spec/capture-session.md`

## Current model

The Rust overlay session still owns capture state, display/export authority, worker coordination,
rendering, and output flow. On macOS, window activation and pointer/IME routing are no longer
owned exclusively by `winit` windows.

Instead, rsnap now uses a split shell model:

- Passive AppKit shells mirror live overlay windows and the frozen toolbar for pointer input.
- A dedicated key-focus AppKit shell appears only when Frozen text editing or scroll capture
  needs keyboard ownership.
- Rust remains the source of truth for session state and input handling; the native shells only
  gather platform events and wake the overlay session.

## Passive pointer shells

The passive shell layer exists to keep live capture, frozen entry, and toolbar pointer
interaction from activating rsnap or stealing `key/main` window status from the target app.

Current behavior:

- Live overlay pointer movement and left-click selection come from passive AppKit shells.
- Frozen toolbar pointer move, drag, hover, and scroll-wheel input also come from passive shells.
- The mirrored `winit` windows remain the render surfaces, but they are not the macOS pointer
  interaction authority for these flows.

This is the layer that replaced the older per-window `winit` focus-policy patching path.

## Explicit key-focus shell

Keyboard ownership is now explicit and scoped:

- Frozen text editing uses a dedicated key-focus shell so ASCII input, command keys, and IME
  composition can flow without turning the pointer shells into key windows.
- Scroll capture uses the same key-focus shell pattern for `Esc`, `Space`, save shortcuts, and
  pause/undo controls.
- When neither text editing nor scroll capture needs keyboard ownership, no capture window should
  remain key-capable.

The key-focus shell is AppKit-owned and implements the minimal responder and `NSTextInputClient`
surface needed for rsnap's current text/IME needs.

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
