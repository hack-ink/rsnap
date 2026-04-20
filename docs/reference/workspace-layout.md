# Workspace Layout Reference

Purpose: Explain the current rsnap workspace layout, which crate owns which behavior today, and
which directories are source versus generated or runtime-local.

Read this when: You are deciding where a change belongs in the current tree, checking whether the
directory structure still matches the implementation, or routing a docs/code question to the right
crate or folder.

Inputs: `Cargo.toml`; `README.md`; `docs/spec/capture-session.md`;
`docs/spec/platform-host-boundary.md`

Depends on: `docs/spec/capture-session.md`; `docs/reference/host-core-reset.md`

Covers: The checked-in workspace layout, crate ownership boundaries today, and the local
directories that should not be treated as repository source.

## Important posture

This document describes the checked-in repository structure today. It is not the architecture
target for the reset lane.

For the active target architecture and migration direction, read:

- `docs/reference/host-core-reset.md`
- `docs/spec/platform-host-boundary.md`
- `docs/decisions/native-host-rust-core-reset.md`

## Current top-level layout

| Path | Role |
| --- | --- |
| `apps/rsnap/` | Desktop native-host crate: tray/menubar startup, hotkeys, settings window, permissions, runtime entry points, macOS host facades, and session handoff into `rsnap-overlay::session` |
| `packages/rsnap-overlay/` | Rust-core session/rendering crate: capture-session logic, overlay rendering, capture backend integration, worker runtime, and scroll-capture stitching/replay semantics, with any remaining macOS host adapters quarantined behind explicit host modules |
| `docs/` | Agent-facing repository docs split into `spec`, `runbook`, `reference`, and `decisions` |
| `assets/` | Shared app-icon and tray-icon source plus generated bundle/runtime assets |
| `scripts/` | Packaging helpers plus structured smoke/perf entrypoints under `scripts/smoke/` and `scripts/perf/` |
| `.github/` | CI workflows and repository rules |

This top-level split reflects the codebase as checked in today. It remains useful for navigation,
but it should not be mistaken for the durable host/core target boundary.

## Crate ownership map

### `apps/rsnap/`

Treat `apps/rsnap/` as the current app shell.

It owns:

- tray and menubar wiring
- capture and settings hotkeys
- startup permission checks and permission-window routing
- settings window lifecycle
- app-level logging/bootstrap
- macOS native capture-host shell lifecycle for pointer, first-responder, keyboard, and IME
  routing into the overlay core
- macOS external scroll-input normalization and observer lifecycle before handing replayable input
  into the overlay session
- macOS scroll-capture screenshot capability acquisition and host-side capability error delivery
- deferred OCR generation tracking around overlay exits

Key paths:

- `apps/rsnap/src/lib.rs`: public runtime entry points plus the crate-level native-host façade
- `apps/rsnap/src/host_macos.rs`: public macOS host-owned capture/effect entry points
- `apps/rsnap/src/app.rs`: app-shell root and event routing
- `apps/rsnap/src/app/`: focused support modules for capture, hotkeys, runtime, and macOS scroll
  input
- `apps/rsnap/src/settings_window/`: settings-window UI, platform hooks, benchmark harness, and
  rendering helpers

### `packages/rsnap-overlay/`

Treat `packages/rsnap-overlay/` as the current transitional container for most capture-session and
overlay behavior.

Today it owns:

- capture-session lifecycle
- overlay, HUD, loupe, and toolbar rendering
- frozen-mode behavior and output flow
- text annotation semantics, text model, edit intent, caret and selection semantics, and rendered
  text state
- capture backend abstraction and worker coordination
- macOS live frame streaming and OCR support
- scroll-capture session logic, replay support, and benchmarks

Important:

- This is the checked-in ownership shape today.
- It is not the long-lived reset target.
- New work should avoid deepening `rsnap-overlay` as the authority for OS-facing window, focus,
  cursor, IME, or capture-capability semantics when that work belongs on the native-host side of
  the reset boundary.

Key paths:

- `packages/rsnap-overlay/src/lib.rs`: explicit `session` façade plus quarantined
  `host_macos` / `host_effects_macos` transition modules
- `packages/rsnap-overlay/src/overlay.rs`: current overlay root plus its focused
  runtime/rendering support modules
- `packages/rsnap-overlay/src/live_frame_stream_macos.rs`: current macOS live-stream support
- `packages/rsnap-overlay/src/scroll_capture.rs`: current scroll-capture session entry with
  focused support modules under `scroll_capture/`

## Documentation placement

- `README.md`: user-facing product and development overview for the whole workspace
- `docs/spec/`: normative behavior and architecture contracts
- `docs/runbook/`: procedural and maintenance runbooks
- `docs/reference/`: descriptive layout, ownership, and implementation references
- `docs/decisions/`: durable rationale records for accepted tradeoffs

## Local-only and generated directories

These paths are intentionally ignored and should not be treated as tracked repository structure:

- `target/`: Rust build products, benchmark outputs, and local analysis artifacts
- `.worktrees/`: local git worktree lanes
- `.workspaces/`: local clone-backed workspace lanes from older workflows
- `.codex/`: local agent/runtime state

If one of these directories appears in a filesystem listing, treat it as local environment noise
unless a task explicitly concerns runtime lane management or generated outputs.

## Structure assessment

The current directory structure is still navigable and serviceable for the checked-in codebase:

- the top-level split between `apps/` and `packages/` still reflects the actual tree
- `docs/`, `assets/`, and `scripts/` remain at the correct shared-workspace level
- several large files have already been split into focused submodules

But the active architecture direction has changed:

- the durable story is no longer "app shell + one overlay/runtime authority"
- the durable story is "native host owns OS semantics, Rust core owns product semantics"

Use this reference for current filesystem routing, not as the final architecture source of truth.
