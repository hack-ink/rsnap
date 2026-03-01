# Live RGB/Loupe Sampling: Streaming Capture Research

Date: 2026-03-01

## Why this doc exists

rsnap’s v0 UX requires **instant** updates on cursor movement:

- RGB under cursor
- Loupe patch under cursor (Alt-held)
- Hovered-window outline switching

In practice, “mature” tools (and Apple’s Digital Color Meter) stay smooth because they **do not** do a full screenshot on every cursor move. They keep a **continuous frame stream** and read pixels from the *latest frame*.

This doc records:

- What caused stutter/jank in rsnap’s prototype.
- What mitigations were applied.
- The industry-standard approach to fully eliminate this class of jank.
- A cross-platform architecture outline for future Windows support (minimum Win10).

## Symptom recap (macOS)

Even after moving hover outline + sampling onto overlay-local caches, users could still trigger “widget not following hand” during fast cursor movement (e.g. circling around window corners to force rapid window switching).

Key observation: **the OS cursor remains smooth** while rsnap’s HUD/Loupe becomes laggy. This points to **app-side stalls** (CPU/bandwidth spikes, blocking calls, or event-loop starvation), not system-wide input lag.

## Root causes we identified

### 1) Full-display screenshot capture during movement

Our live sampling path used `xcap_monitor.capture_image()` (full display readback) to refresh a monitor snapshot.
Even when throttled, this can produce **periodic spikes** (CPU + memory bandwidth) and cause the UI thread to miss frames.

### 2) Doing expensive window-server queries too frequently

macOS window hit-testing/geometry was previously relying on APIs that can be expensive if called repeatedly. The direction that worked best was:

- Do **one-pass** window list collection.
- Cache it.
- Compute “hovered rect contains point” locally.

### 3) Native HUD window movement in the hot path

Calling `set_outer_position` for HUD/Loupe windows on every mouse move can also amplify jank. Even if each call is “fast”, the aggregate cost can be high at high event rates.

## Mitigations applied in rsnap (current state)

These mitigations reduced jank but do not guarantee elimination under all loads:

1) **Overlay computes instantly from caches**
   - Hover outline + RGB/Loupe computed synchronously from local snapshot(s).
   - Worker only refreshes snapshots asynchronously.

2) **Throttle native HUD/Loupe moves**
   - Cap native window `set_outer_position` to <= 60Hz.
   - Apply pending positions in `about_to_wait()` so the window catches up when events stop.

3) **Never refresh full-display snapshot while cursor is actively moving**
   - CursorMoved path uses existing snapshots only.
   - Snapshot refresh requests happen on “idle tick” (no recent cursor events) or first-frame seeding.

These changes improve perceived smoothness but still rely on full-display capture for updates, which can spike.

## What fully fixes it: “Frame stream” architecture

To eliminate this class of stutter, the proven approach is:

- Maintain a continuous **frame stream** (frames arrive at display cadence, via system compositor APIs).
- Keep a reference to the **latest frame** (ideally GPU-backed, or at least a shared surface/buffer).
- On cursor movement:
  - Sample a single pixel for RGB.
  - Sample a small patch for Loupe.
  - Update hover outline purely from cached window geometry.
  - Render via GPU (Metal / D3D).

The important property is: **cursor move does not trigger “take screenshot”.**

### macOS: ScreenCaptureKit (recommended)

- API family: ScreenCaptureKit (`SCStream`, `SCStreamConfiguration`, `CMSampleBuffer`)
- Availability: macOS 12.3+
- Output frames come as sample buffers; typical implementations keep frames in an `IOSurface`/`CVPixelBuffer` path and avoid full CPU copies until needed.
- Rust (binding): `objc2-screen-capture-kit` (Objective-C bindings; no Swift runtime dependency): `https://crates.io/crates/objc2-screen-capture-kit`
- Rust: `objc2-screen-capture-kit` API docs: `https://docs.rs/objc2-screen-capture-kit`

Implementation note (rsnap): we intentionally avoid Swift-based bindings here, since they can pull in Swift Concurrency runtime dependencies (e.g. `libswift_Concurrency.dylib`) that may not be available in unbundled CLI builds.

WWDC references (useful for deeper details and tuning):

- WWDC 2022 Session 10155, ScreenCaptureKit overview (`https://developer.apple.com/videos/play/wwdc2022/10155`)
- WWDC 2022 Session 10156, stream-based sampling patterns (`https://developer.apple.com/videos/play/wwdc2022/10156`)

- Apple API reference (ScreenCaptureKit): `https://developer.apple.com/documentation/screencapturekit`

### Windows (future): minimum Win10

We are not implementing Windows yet, but the cross-platform plan should assume:

- Preferred: **Windows.Graphics.Capture** (Win10 1803+ in general; Win32 interop patterns often require 1903+ for `IGraphicsCaptureItemInterop` style usage).
- Microsoft Learn overview of Windows Graphics Capture APIs (`https://learn.microsoft.com/windows/apps/develop/graphics-capture`).
- Sample API docs for `GraphicsCapture` and related Win32 interop (`https://learn.microsoft.com/windows/win32/api/windows.graphics.capture/`, `https://learn.microsoft.com/windows/win32/api/windows.graphics.capture.interop/`).
- WGC interop version guidance (`https://learn.microsoft.com/windows/win32/api/windows.graphics.capture.interop/`).

- Compatibility fallback: **DXGI Desktop Duplication** (Windows 8+), monitor-based capture, DXGI surfaces, dirty rects, etc. (`https://learn.microsoft.com/windows/win32/direct3ddxgi/desktop-duplication`)

Practical implication for “minimum Win10”:

- If we truly need “all Win10 builds”, we likely need either:
  - WGC with feature-gating + picker-based flow on older builds, or
  - a DXGI Desktop Duplication fallback path.

### Linux (future)

For Wayland:

- Preferred: `xdg-desktop-portal` screen-cast -> PipeWire frame stream (compositor approved).

For X11:

- Various options exist (XShm/XComposite/etc), but expect fragmented behavior vs Wayland portal.

## Proposed rsnap architecture (cross-platform)

Split “capture” into two planes:

1) **Live plane (low-latency frame stream)**
   - Purpose: RGB/Loupe sampling + live UI.
   - Requirements:
     - Frame stream per monitor (or per selected capture target).
     - Latest-frame access without blocking UI thread.
     - Prefer GPU-backed frames (Metal texture, D3D texture).

2) **Freeze/export plane (high quality still)**
   - Purpose: freeze selection and export PNG.
   - Requirements:
     - High-quality still capture of selected region/window/monitor.
     - May tolerate more latency than live plane (but should remain snappy).

Suggested traits/modules:

- `FrameStreamBackend`:
  - start/stop per monitor
  - publish `LatestFrameHandle` (reference-counted)
  - expose pixel format + scale factor metadata

- `PixelSampler`:
  - `sample_rgb(frame, point) -> Option<Rgb>`
  - `sample_patch(frame, point, w, h) -> Option<RgbaImage>` (prefer GPU path for loupe)

- `WindowGeometryBackend`:
  - periodic window list snapshot (z-order ordered)
  - fast hover hit-test locally

## Implementation notes (macOS-first)

1) Start with ScreenCaptureKit stream per monitor in live mode.
2) Keep `LatestFrame` in an `Arc` and update it on the stream callback.
3) In overlay’s cursor move path:
   - Read RGB/patch from latest frame (no worker roundtrip).
4) Render loupe with GPU sampling if possible; only fall back to CPU patch extraction when needed.
5) Keep the current window-geometry cache approach; it is independent and already “cheap enough” when cached.

## Risks / pitfalls to plan for

- Permission / TCC: ScreenRecording permission is required for capture.
- HDR / wide color: captured pixel formats can differ; sampling must handle format conversion or choose a consistent pixel format in config.
- Multi-monitor: frame stream per monitor or a unified stream; map global coords correctly.
- Backpressure: keep only “latest frame”, drop older frames.
- Threading: never block winit/egui render loop on capture callbacks.

## Status / TODOs

- [ ] Implement macOS `SCStream` live frame stream backend for sampling (replace xcap full-display refresh in live mode).
- [ ] Keep current xcap still-capture for freeze/export until SCStream still path is proven stable.
- [ ] Add an opt-in debug overlay for “frame age” (now - captured_at) and UI update cadence.
