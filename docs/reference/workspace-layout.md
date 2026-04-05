# Workspace Layout Reference

Purpose: Explain the current rsnap workspace layout, which crate owns which behavior, and which
directories are source versus generated or runtime-local.

Read this when: You are deciding where a change belongs, checking whether the current directory
structure still matches the implementation, or routing a docs/code question to the right crate or
folder.

Inputs: `Cargo.toml`; `README.md`; `apps/rsnap/README.md`; `docs/spec/capture-session.md`

Depends on: `docs/spec/capture-session.md`

Covers: The tracked workspace layout, crate ownership boundaries, and the local directories that
should not be treated as repository source.

## Current top-level layout

| Path | Role |
| --- | --- |
| `apps/rsnap/` | Desktop app-shell crate: tray/menubar startup, hotkeys, settings window, permissions, logging, and session handoff into `rsnap-overlay` |
| `packages/rsnap-overlay/` | Overlay/runtime crate: overlay windows, HUD/loupe/toolbar rendering, capture backend integration, OCR handoff, worker runtime, and scroll capture |
| `docs/` | Agent-facing repository docs split into `spec`, `runbook`, `reference`, and `decisions` |
| `assets/` | Shared app-icon and tray-icon source plus generated bundle/runtime assets |
| `scripts/` | Packaging and dedicated macOS smoke helpers |
| `.github/` | CI workflows and repository rules |

This top-level split is reasonable for the current implementation because the workspace has one
shipping app crate and one reusable overlay/runtime crate, with docs and assets shared at the root.

## Crate ownership map

### `apps/rsnap/`

Treat `apps/rsnap/` as the app shell.

It owns:

- tray and menubar wiring
- capture and settings hotkeys
- startup permission checks and permission-window routing
- settings window lifecycle
- app-level logging/bootstrap
- macOS external scroll-input normalization before handing events to the overlay session
- deferred OCR generation tracking around overlay exits

Key paths:

- `apps/rsnap/src/app.rs`: app-shell root and event routing
- `apps/rsnap/src/app/`: focused support modules for capture, hotkeys, runtime, and macOS scroll
  input
- `apps/rsnap/src/settings_window/`: settings-window UI, platform hooks, benchmark harness, and
  rendering helpers

### `packages/rsnap-overlay/`

Treat `packages/rsnap-overlay/` as the capture-session and overlay engine.

It owns:

- overlay session lifecycle
- overlay, HUD, loupe, and toolbar windows
- frozen-mode behavior and output flow
- capture backend abstraction and worker coordination
- macOS live frame streaming and OCR support
- scroll-capture session logic, replay support, and benchmarks

Key paths:

- `packages/rsnap-overlay/src/lib.rs`: public session-level surface exported to the app shell
- `packages/rsnap-overlay/src/overlay.rs`: overlay root plus its focused runtime/rendering support
  modules
- `packages/rsnap-overlay/src/scroll_capture.rs`: scroll-capture session entry with focused
  support modules under `scroll_capture/`

## Documentation placement

- `README.md`: user-facing product and development overview for the whole workspace
- `apps/rsnap/README.md`: crate-local ownership and file-map note for the app shell
- `docs/spec/`: normative behavior and performance contracts
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

The current directory structure is mostly sound:

- the top-level split between `apps/` and `packages/` matches the actual crate boundary
- `docs/`, `assets/`, and `scripts/` live at the right shared-workspace level
- the overlay and settings-window internals have already been split into focused submodules rather
  than staying in single hotspot files

The main source of confusion was documentation, not code layout:

- root and crate-local docs did not clearly separate product overview from crate ownership
- the docs router did not answer basic "where does this live?" questions
- old terminology blurred runbooks, references, and durable rationale

Use this reference as the default answer for repository-layout questions instead of inferring
meaning from local runtime directories such as `.worktrees/` or `.workspaces/`.
