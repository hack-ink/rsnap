# Live Sampling Reference

Purpose: Describe the current stream-first live RGB/loupe sampling path and why rsnap uses it.

Read this when: You are working on live HUD updates, loupe behavior, cursor-path performance, or
hovered-window outline responsiveness.

Inputs: `docs/spec/capture-session.md`; current focus is the macOS live path rather than
freeze/export.

Depends on: `docs/spec/capture-session.md`

Covers: The stream-only sampling model, cache usage, fallback behavior, and the current live
versus freeze/export capture split.

Spec boundary: `docs/spec/capture-session.md`

Governing performance contract: `docs/spec/performance.md`

Date: 2026-05-01

## Active-lane note

This document describes the checked-in live-sampling implementation, not the full target
architecture. For the reset architecture route, start with `docs/reference/host-core-reset.md` and
`docs/spec/platform-host-boundary.md`.

## Why this doc exists

rsnap current UX requires instant updates on cursor movement:

- RGB under cursor
- Loupe patch under cursor
- Hovered-window outline switching

In practice, mature tools stay smooth because they avoid taking a screenshot on every cursor
move. They keep a continuous frame stream and sample pixels from the latest frame.

This reference records the current stream-first implementation and why it is used.

## Symptom recap (macOS)

Even after moving hover outline + sampling to overlay-local caches, fast cursor movement could
still stall the HUD/Loupe UI (for example, circling quickly across window corners).

Key observation: system cursor tracking remains smooth while rsnap updates lag, indicating
app-side stalls in the live path.

## Root causes that were observed

1. Full-display capture during live movement.
2. Expensive window lookups during movement.
3. Unnecessary work on every move.

Previous live updates depended on full-display reads. Even with throttling, this created periodic
CPU and memory spikes.

Window queries that cross process boundaries or call the window server per event can cause jitter
under rapid movement.

Window refresh and sampling work in the same high-frequency path increases jitter pressure.

## Implemented stream path

rsnap now uses the following live model on macOS:

- Keep a per-monitor `SCStream` alive while in live mode.
- Store the latest frame in shared state.
- On cursor move, sample:
  - a single RGB pixel, and
  - a loupe patch
  from that latest frame, without requesting another full capture.
- Cache and reuse window geometry from periodic snapshots, then do point-in-rect test locally for
  outline switching.
- Keep CPU work per mouse move to a strict minimum.

Fallback behavior:

- Live sampling is strict stream-only.
- If stream sampling is unavailable (for example unavailable permission), live RGB/Loupe samples
  remain empty rather than triggering xcap-style full-frame capture.
- Freeze commit uses the frozen-frame authority stream, not the live-sampler latest-monitor cache.

## macOS implementation details

- Stream: `SCStream` (`SCStreamConfiguration`, `CMSampleBuffer`) via
  `objc2-screen-capture-kit`.
- Minimum: macOS 12.3+.
- Pixel sampling is requested through a combined cursor-sample operation to avoid repeated buffer
  locks.
- Stream queue depth is tuned for latest-frame behavior and low-latency live response.
- HUD/Loupe movement remains throttled in the render scheduling path.

## Window hit-testing architecture (implemented)

- Window rectangles are collected into a window list snapshot.
- Snapshot is refreshed on a short cadence while live.
- Hover outline is computed from local z-order/geometry lookup, not by repeatedly hitting system
  APIs.

## Support constraints

- Live path intentionally excludes these UI layers from outline targeting:
  - Menu bar
  - Dock
  - Desktop layer
- This keeps behavior stable and avoids false window outlines.

## Frozen-frame freshness boundary

The live sampler's latest-monitor APIs are cache-oriented. They can return the last warm stream
frame without proving when that frame was captured, and the Swift FFI bridge does not currently
carry the source frame age or sequence. That is acceptable for live RGB/Loupe sampling, where the
next stream tick can correct the HUD, but it is not acceptable for the frozen screenshot first
frame.

Frozen commit must use `FrozenFrameAuthority` or another source with equivalent provenance:

- The frame must carry a real capture timestamp and stream sequence.
- `post_token` is preferred: the frame sequence advanced after the frozen latch.
- `latest_unchanged` is allowed only for a fresh same-sequence frame on an unchanged/static
  desktop.
- Cache-only `peekLatestMonitorImage`, `take_latest_monitor_rgba`, or wrappers that synthesize
  `capturedAt` at call time must not feed the frozen first frame.
- A frozen-authority stream warmed before overlay windows became visible must be replaced after
  those windows are on screen, but replacement must be a hot handoff: keep the previous
  self-capture-excluding stream alive until the replacement stream is configured so fast click
  selection cannot fail with `no_fresh_frame` solely because content-filter lookup is still
  running. Once the replacement stream is ready, the first Frozen display frame must come from a
  self-capture-excluding filter, not from a pre-overlay filter that can see rsnap's own live mask,
  border, badge, or toolbar.
- If no freshness-proven frame is available, fail the freeze with `no_fresh_frame` instead of
  showing a screenshot from seconds earlier.

This is the guard against the old regression where a quick drag could freeze a frame that came from
seconds-old live-stream cache data while telemetry incorrectly reported it as current.

## Current capture-plane split

The current implementation already keeps capture responsibilities split by quality profile:

- `Live` plane: stream-first, low-latency RGB/Loupe and live outline updates.
- `Freeze first-frame` plane: freshness-proven frozen authority frames for the immediate handoff.
- `Export` plane: higher-cost output capture after frozen mode is established.

Linux/Windows details remain out of scope for the current macOS-first contract.

## Known status

- [x] Implemented macOS `SCStream` live path for cursor samples (RGB/Loupe).
- [x] Removed live full-display refresh dependency from cursor path.
- [x] Kept frozen first-frame commit off cache-only live-sampler full-monitor snapshots.
- [ ] Add opt-in diagnostics for live sample latency beyond frozen `frameAgeMs`.
