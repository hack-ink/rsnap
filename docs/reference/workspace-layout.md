# Workspace Layout Reference

Purpose: Explain the current Rsnap workspace layout, which crate owns which behavior today, and
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
| `native/macos-host/` | SwiftPM AppKit-first macOS host shell: menu bar entry, full-screen capture windows, in-window HUD/toolbar, and native bridging into `rsnap-host-ffi` |
| `apps/rsnap/` | Thin launcher/bootstrap crate: startup logging, build metadata, stable-bundle resolution, and `cargo run -p rsnap` handoff into the staged native macOS host |
| `packages/rsnap-overlay/` | Rust-core session/rendering crate: capture-session logic, overlay rendering, capture backend integration, worker runtime, and scroll-capture stitching/replay semantics, with any remaining macOS host adapters quarantined behind explicit host modules |
| `packages/rsnap-capture-core/` | New durable product-semantics crate: shared geometry, semantic scene model, host/core protocol enums, and the first reset-native session core |
| `packages/rsnap-host-ffi/` | New thin C ABI bridge crate for future native hosts that call the Rust product core |
| `docs/` | Agent-facing repository docs split into `spec`, `runbook`, `reference`, and `decisions` |
| `assets/` | Shared app-icon source plus generated bundle/runtime assets |
| `scripts/` | Packaging helpers plus structured smoke/perf entrypoints under `scripts/smoke/` and `scripts/perf/` |
| `.github/` | CI workflows and repository rules |

This top-level split reflects the codebase as checked in today. It remains useful for navigation,
but it should not be mistaken for the durable host/core target boundary.

## Crate ownership map

### `apps/rsnap/`

Treat `apps/rsnap/` as the launcher/bootstrap layer for the native host.

It owns:

- app-level logging/bootstrap
- build metadata emission
- stable native-bundle lookup from the current worktree
- fallback handoff into `scripts/build_and_run.sh` when the staged native bundle is missing
- explicit unsupported-platform failure for non-macOS builds

Key paths:

- `apps/rsnap/src/lib.rs`: public runtime entry points for the launcher crate
- `apps/rsnap/src/native_launcher_macos.rs`: staged native-bundle lookup and launch handoff
- `apps/rsnap/src/startup.rs`: startup logging/bootstrap helpers
- `apps/rsnap/src/unsupported_platform.rs`: explicit non-macOS error path

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

### `packages/rsnap-capture-core/`

Treat `packages/rsnap-capture-core/` as the new durable product-semantics landing zone.

It owns:

- portable geometry types
- semantic scene snapshots
- explicit host/core protocol enums and structs
- the first reset-native reference session core

This crate must stay free of:

- top-level window ownership
- `winit` or `egui` runtime authority
- AppKit ownership
- host-side permission or capture capability code

### `packages/rsnap-host-ffi/`

Treat `packages/rsnap-host-ffi/` as the thin ABI companion to `rsnap-capture-core/`.

It owns:

- opaque session handles for foreign hosts
- FFI-safe config, event, report, scene, and request types
- exported `extern "C"` functions that forward into `rsnap-capture-core`
- the checked-in C header consumed by future native hosts:
  `packages/rsnap-host-ffi/include/rsnap_host_ffi.h`

It does not own product behavior beyond ABI adaptation.

### `native/macos-host/`

Treat `native/macos-host/` as the new native macOS landing zone for the reset.

It owns:

- the SwiftPM-built `.app` host shell
- the AppKit window/view tree used for live and frozen capture UI
- native cursor, focus, event routing, menu bar entry, and host-side effects
- the checked-in bridge probe used by `cargo make test-host-reset`

It depends on:

- `packages/rsnap-host-ffi/` for the C ABI contract
- `packages/rsnap-capture-core/` indirectly through that ABI

It must not grow a second product-semantic model. Scene state and host requests still come from the
Rust core.

## Documentation placement

- `README.md`: user-facing product and development overview for the whole workspace
- `docs/spec/`: normative behavior and architecture contracts
- `docs/runbook/`: procedural and maintenance runbooks
- `docs/reference/`: descriptive layout, ownership, and implementation references
- `docs/decisions/`: durable rationale records for accepted tradeoffs

## Local-only and generated directories

These paths are intentionally ignored and should not be treated as tracked repository structure:

- `target/`: Rust build products, benchmark outputs, and local analysis artifacts
- `target/rsnap-native-host/`: locally staged native-host `Rsnap.app` bundles from
  `scripts/build_and_run.sh`
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
