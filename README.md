# rsnap

Pure-Rust menubar screenshot prototype (macOS-first).

## Status

Prototype / in active development.

## v0 behavior (prototype)

- Menubar-only app (no Dock icon) on macOS
- Global hotkey: `Alt+X` (macOS: Option+X)
- Transparent capture-session overlay (no dim mask) that blocks interaction with the desktop
- HUD near the cursor showing global `x,y` and `rgb(r,g,b)`
- Left click freezes the active monitor as a fullscreen screenshot
- Space copies the frozen screenshot PNG to the clipboard and exits
- Esc cancels and exits

The app crate (`apps/rsnap`) runs the menubar and hotkey controller that drives the
overlay crate (`packages/rsnap-overlay`).

See `SPEC.md` for the normative v0 contract.

### macOS permissions

`rsnap` requires **Screen Recording** permission to capture other apps/windows.

- Go to `System Settings` -> `Privacy & Security` -> `Screen Recording`.
- Enable `rsnap` (the built `.app`), then relaunch the app.

## Development

This repository uses `cargo make` tasks in `Makefile.toml` and workspace-level
build commands for the Rust-only flow.

Common commands:

```sh
cargo build --workspace
cargo make fmt
cargo make clippy
cargo make test
cargo make checks
```

## Notes

This repo intentionally omits Tauri/WebView in favor of a Rust-only v0 architecture.

## License

GPL-3.0 (see `LICENSE`).
