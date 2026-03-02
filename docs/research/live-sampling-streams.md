# Live RGB/Loupe Sampling: Stream-First Architecture

Date: 2026-03-02

## Why this doc exists

rsnap v0 UX requires instant updates on cursor movement:

- RGB under cursor
- Loupe patch under cursor (Alt-held)
- Hovered-window outline switching

In practice, mature tools stay smooth because they avoid taking a screenshot on every cursor move. They keep a continuous frame stream and sample pixels from the latest frame.

This doc records the current stream-first implementation and why it is used.

## Symptom recap (macOS)

Even after moving hover outline + sampling to overlay-local caches, fast cursor movement could still stall the HUD/Loupe UI (for example, circling quickly across window corners).

Key observation: system cursor tracking remains smooth while rsnap updates lag, indicating app-side stalls in the live path.

## Root causes that were observed

1) full-display capture during live movement

Previous live updates depended on full-display reads. Even with throttling, this created periodic CPU and memory spikes.

2) expensive window lookups during movement

Window queries that cross process boundaries or call the window server per event can cause jitter under rapid movement.

3) unnecessary work on every move

Window refresh and sampling work in the same high-frequency path increases jitter pressure.

## Implemented stream path

rsnap now uses the following live model on macOS:

- Keep a per-monitor `SCStream` alive while in live mode.
- Store the latest frame in shared state.
- On cursor move, sample:
  - a single RGB pixel, and
  - a loupe patch
  from that latest frame, without requesting another full capture.
- Cache and reuse window geometry from periodic snapshots, then do point-in-rect test locally for outline switching.
- Keep CPU work per mouse move to a strict minimum.

Fallback behavior:

- Live sampling is strict stream-only.
- If stream sampling is unavailable (e.g. unavailable permission), live RGB/Loupe samples remain empty rather than triggering xcap-style full-frame capture.
- Freeze/export still uses the existing still capture plane.

## macOS implementation details

- Stream: `SCStream` (`SCStreamConfiguration`, `CMSampleBuffer`) via `objc2-screen-capture-kit`.
- Minimum: macOS 12.3+.
- Pixel sampling is requested through a combined cursor-sample operation to avoid repeated buffer locks.
- Stream queue depth is tuned for latest-frame behavior and low-latency live response.
- HUD/Loupe movement remains throttled in the render scheduling path.

## Window hit-testing architecture (implemented)

- Window rectangles are collected into a window list snapshot.
- Snapshot is refreshed on a short cadence while live.
- Hover outline is computed from local z-order/geometry lookup, not by repeatedly hitting system APIs.

## Support constraints

- Live path intentionally excludes these UI layers from outline targeting:
  - Menu bar
  - Dock
  - Desktop layer
- This keeps behavior stable and avoids false window outlines.

## Proposed future architecture direction

Keep capture split by quality profile:

- **Live plane**: stream-first, low-latency RGB/Loupe and live outline updates.
- **Freeze/export plane**: higher-cost still capture for full screenshot quality.

Linux/Windows details are tracked for future work and remain out of scope for this live-implementation milestone.

## Known status

- [x] Implemented macOS `SCStream` live path for cursor samples (RGB/Loupe).
- [x] Removed live full-display refresh dependency from cursor path.
- [x] Kept freeze/export on the existing still-capture flow.
- [ ] Add opt-in diagnostics for frame age and sample latency.
