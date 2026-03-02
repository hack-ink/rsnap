# Window Hit-Testing Strategy (Live Outline)

## Scope

This note defines the window outline strategy used by rsnap live mode as of March 2026.

## Strategy A (current, default)

- Capture geometry via a window-list snapshot from the windowing API.
- Keep snapshot in memory and sorted by stacking order.
- On cursor move, test `point ∈ rect` against cached windows to choose the hovered window outline.

Why this is the default:

- One expensive query per snapshot interval, not per move.
- Predictable latency under fast cursor movement.
- Keeps live path independent of heavyweight synchronous hit-test calls.
- Works naturally with the stream-first cursor sampling flow.

Implementation notes:

- Snapshot interval is tuned for responsiveness without saturating the window server.
- If a snapshot is missing, no outline is drawn until fresh geometry is available.
- Excluded from target set: menu bar, dock, and desktop-layer windows.

## Strategy B (alternative, researched)

- Query the active window under point directly on each cursor event (for example through platform-specific APIs that report topmost window at point).

Observed trade-offs:

- Potentially lower code complexity when supported by a single API.
- Higher jitter risk at high event rates.
- Can produce inconsistent results across platform boundaries and special windows.
- Less aligned with a strict stream-first low-latency pipeline.

## Planned rule

Use Strategy A by default on macOS live mode. Evaluate Strategy B only if we need explicit behavior changes in a later release.
