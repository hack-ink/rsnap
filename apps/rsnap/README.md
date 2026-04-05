# rsnap App Shell

This file is the crate-local directory guide for `apps/rsnap/`.
Cargo package metadata for `rsnap` still points at the workspace-root `README.md`, so this file
exists only as a local maintenance and ownership note for this crate directory.

Read the workspace `README.md` for product usage and end-user feature overview. Read
`docs/reference/workspace-layout.md` when you need the wider workspace map.

## What this crate owns

`apps/rsnap/` is the desktop app shell that wraps the overlay engine exported by
`rsnap-overlay`.

It owns:

- tray and menubar lifecycle
- capture and settings hotkeys
- startup logging/bootstrap
- settings-window lifecycle and UI entry points
- macOS permission checks and permission-window routing
- app-level handling for overlay exits such as deferred OCR follow-up
- macOS external scroll-input normalization before those events are handed to the overlay session

It does not own the capture-session runtime itself. Overlay windows, capture backends, worker
flow, OCR request processing, and scroll-capture stitching live in `packages/rsnap-overlay/`.

## Key source paths

- `src/main.rs`: binary entrypoint
- `src/lib.rs`: shared library surface for benches and tests
- `src/app.rs`: app-shell root and event routing
- `src/app/`: focused support modules for capture, hotkeys, runtime, shell/menu wiring, and
  macOS scroll input
- `src/settings_window/`: settings-window chrome, render path, platform glue, sections, and
  benchmark helpers
- `src/startup.rs`: startup/build metadata and logging bootstrap
- `src/settings.rs`: app settings model and parsing

## Runtime notes

- Runtime logs are written to the app `ProjectDirs` data directory under `logs/`
  (on macOS: `~/Library/Application Support/ink.hack.rsnap/logs`).
- If file logging cannot start, rsnap falls back to console logging.
- The default global capture hotkey is `Alt+X` (`Option+X` on macOS) and can be customized from
  Settings.

## Packaging notes

- The bundled macOS app uses shared assets from the workspace-root `assets/` tree.
- `scripts/bundle-macos.sh` post-processes the bundled app and compiles the Dock icon from the
  shared Icon Composer source.
- The release workflow signs, notarizes, and staples the macOS `.app` before publishing the
  release artifact.

## Verification entrypoints

- Run the app locally: `cargo run -p rsnap`
- Repo-native smoke on macOS:
  - `cargo make smoke-self-check-macos`
  - `cargo make smoke-macos`

## Related docs

- Workspace overview: `README.md`
- Workspace layout and crate boundaries: `docs/reference/workspace-layout.md`
- Runtime behavior contract: `docs/spec/capture-session.md`
- Performance contract and smoke routing: `docs/spec/performance.md`
