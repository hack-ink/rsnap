# Window Hit-Testing Guide

Goal: Describe the current live-mode window hit-testing strategy and the default choice used by
rsnap.

Read this when: You are changing hovered-window outline behavior, evaluating live hit-test
latency, or comparing candidate hit-testing strategies.

Inputs: `docs/spec/v0.md`; current focus is the macOS live path and its current default strategy.

Depends on: `docs/spec/v0.md`

Outputs: Understanding of the snapshot-based default strategy, the researched alternative, and the
planned rule for current live mode.

Spec boundary: `docs/spec/v0.md`

## Current Strategy Scope

This guide defines the current window-outline strategy used by rsnap live mode as of March 2026.

## Strategy A (current, default)

- Capture geometry via a window-list snapshot from the windowing API.
- Keep the snapshot in memory and sorted by stacking order.
- On cursor move, test `point ∈ rect` against cached windows to choose the hovered window
  outline.

Why this is the default:

- One expensive query per snapshot interval, not per move.
- Predictable latency under fast cursor movement.
- Keeps the live path independent of heavyweight synchronous hit-test calls.
- Works naturally with the stream-first cursor sampling flow.

Implementation notes:

- Snapshot interval is tuned for responsiveness without saturating the window server.
- If a snapshot is missing, no outline is drawn until fresh geometry is available.
- Excluded from the target set: menu bar, dock, and desktop-layer windows.

## Strategy B (alternative, researched)

- Query the active window under point directly on each cursor event, for example through
  platform-specific APIs that report the topmost window at a point.

Observed trade-offs:

- Potentially lower code complexity when supported by a single API.
- Higher jitter risk at high event rates.
- Can produce inconsistent results across platform boundaries and special windows.
- Less aligned with a strict stream-first low-latency pipeline.

## Planned rule

Use Strategy A by default on macOS live mode. Evaluate Strategy B only if a later release needs
explicit behavior changes.
