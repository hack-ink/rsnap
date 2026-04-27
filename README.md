<div align="center">

# rsnap

macOS-first screenshot prototype in native-host / Rust-core reset.

[![License](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Language Checks](https://github.com/hack-ink/rsnap/actions/workflows/language.yml/badge.svg?branch=main)](https://github.com/hack-ink/rsnap/actions/workflows/language.yml)
[![Release](https://github.com/hack-ink/rsnap/actions/workflows/release.yml/badge.svg)](https://github.com/hack-ink/rsnap/actions/workflows/release.yml)
[![GitHub tag (latest by date)](https://img.shields.io/github/v/tag/hack-ink/rsnap)](https://github.com/hack-ink/rsnap/tags)
[![GitHub last commit](https://img.shields.io/github/last-commit/hack-ink/rsnap?color=red&style=plastic)](https://github.com/hack-ink/rsnap)
[![GitHub code lines](https://tokei.rs/b1/github/hack-ink/rsnap)](https://github.com/hack-ink/rsnap)

</div>

## Feature Highlights

- Menubar-only app (no Dock icon) on macOS.
- Global hotkey: `Alt+X` (macOS: Option+X).
- Transparent capture-session overlay that blocks desktop interaction.
- HUD near the cursor showing global `x,y` and `rgb(r,g,b)`.
- Left click + drag freezes a selected region; a single left click freezes the hovered window or falls back to the active monitor fullscreen.
- On macOS, entering Frozen mode restores the pre-capture target after selection completes instead of leaving rsnap focused; exiting capture restores the original frontmost app.
- In Frozen mode, a dragged-region capture can be dragged from inside the bright selection area to reposition it without resizing.
- In Frozen mode, `Space` copies the current frozen PNG to the clipboard and exits.
- In Frozen mode, Cmd+S (macOS) / Ctrl+S saves the current PNG to disk and exits.
- On macOS, Frozen mode can recognize text from the current capture and copy the result to the clipboard from the toolbar.
- After a dragged region freeze, press `s` or use the frozen toolbar `Scroll Capture ↓` action to enter scroll capture.
- Scroll capture is currently implemented on macOS for dragged-region freezes and uses image-first downward stitching with a live side preview.
- Upward scrolling may be observed for rewind/reacquire, but it never appends stitched rows.
- `Esc` cancels capture; during scroll capture, `Esc` / `Back` returns to normal Frozen mode.
- Glass HUD with Liquid Glass on supported macOS, plus classic blur/tint controls.
- Tab-triggered loupe sample and frozen-mode toolbar for quick action access.

## Status

Prototype / in active development.

## Reset posture

- Shipping behavior on macOS now enters through the native host lane:
  - `native/macos-host/` is the SwiftPM AppKit host shell and the default local Run path.
  - `apps/rsnap/` is now a thin launcher/bootstrap crate that stages or opens the native host
    bundle and records startup logging.
  - `packages/rsnap-overlay/` remains a transitional Rust implementation container for reset
    slices that have not yet moved into `rsnap-capture-core` / the native host.
- The active reset target is no longer a pure-Rust UI stack. New boundary crates now live in:
  - `packages/rsnap-capture-core/` for platform-neutral session semantics and host/core protocol
    models
  - `packages/rsnap-host-ffi/` for the thin C ABI that a future native macOS host will call
    through `packages/rsnap-host-ffi/include/rsnap_host_ffi.h`
- Current version support remains **macOS only**. Windows and Linux stay out of scope for this
  version beyond protocol and abstraction design.

## Capture platform support

- Live sampling path: **macOS 12.3+** via ScreenCaptureKit. Live loupe/window
  sampling uses `SCStream`; downward scroll capture uses discrete
  `SCScreenshotManager` region screenshots plus pairwise registration.
- Live mode is stream-first and does not capture full display on cursor movement.
- Frozen capture and scroll-capture imagery on macOS use the native capture stack;
  `docs/spec/capture-session.md` is the current contract source of truth.
- Menubar and Dock are not included in live window-outline targeting.
- Windows support is planned (minimum Windows 10), but not implemented yet.

## Usage

### Installation

#### Build from Source

```sh
git clone https://github.com/hack-ink/rsnap
cd rsnap

cargo build --workspace
cargo run -p rsnap
```

### macOS permissions

`rsnap` currently relies on **Screen Recording** permission to capture other apps/windows.
- ScreenCaptureKit live sampling on macOS requires macOS 12.3+ and Screen Recording permission.
- Normal region/window/monitor capture does not require Accessibility or Input Monitoring.
- Scroll capture currently requires **Accessibility** because rsnap forwards scroll into the target app.
- Scroll capture currently requires **Input Monitoring** because rsnap listens for global scroll-wheel input via a native macOS listen-event tap.
- The native host currently disables scroll capture, so the active native feature set only requests Screen Recording.
- macOS may phrase the Input Monitoring prompt as receiving keystrokes from any application even though rsnap only listens for scroll-wheel input in this path.
- macOS may describe Screen Recording as `Screen & System Audio Recording` or as direct screen/audio access when rsnap bypasses the system picker.
- The native menubar host exposes `Permissions…`, which shows Screen Recording, Accessibility, and Input Monitoring status. It marks Accessibility and Input Monitoring as not needed until native scroll automation is enabled.
- Normal native capture depends on Screen Recording; if access is missing, rsnap reports it in the host status message and the Permissions window is the canonical repair surface.
- You can reopen `Permissions…` from the tray or menubar menu at any time.
- Base capture path: `System Settings` -> `Privacy & Security` -> `Screen Recording`.
- Scroll capture paths: `System Settings` -> `Privacy & Security` -> `Accessibility` and `Input Monitoring`.
- Enable `rsnap` (the built `.app`), then retry capture. If macOS still keeps capture blocked after changing a permission, relaunch the app.

### HUD settings behavior

- The native `Settings…` window currently owns:
  - HUD glass enable/disable
  - HUD glass style (`Liquid Glass` / `Classic Blur`)
  - Liquid Glass style (`Regular` / `Clear`) when supported by macOS
  - Classic HUD opacity / blur / tint / color
  - HUD `Tab` hint visibility
  - loupe sample size (`small` / `medium` / `large`)
  - output directory
  - filename prefix
  - output naming (`timestamp` / `sequence`)
  - frozen toolbar placement (`bottom` / `top`)

### Output (save-to-disk)

- In Frozen mode, use Cmd+S (macOS) / Ctrl+S to save a PNG to disk and exit.
- On macOS, use the frozen toolbar `Recognize Text` action to copy recognized text from the current frozen capture and exit.
- After entering scroll capture from a dragged region on macOS, downward scrolling may append newly proven rows into the side preview.
  Upward scrolling never appends. Returning to already-stitched content should not grow the export; only newly proven content may be added.
  The scroll-capture commit path uses discrete region screenshots plus pairwise image registration; clipboard and save must match the committed preview the user sees.
  `Space` copies the stitched image, Cmd+S (macOS) / Ctrl+S saves it, and `Esc` / `Back`
  returns to the original Frozen capture without exiting.
- Output is configured in the native `Settings…` window:
  - `output directory` (default: Desktop)
  - `filename prefix` (default: `rsnap`, sanitized to `[A-Za-z0-9_-]`)
  - `output naming` (`timestamp` or `sequence`)

## Development

```sh
cargo make fmt
cargo make lint
cargo make test
cargo make test-host-reset
cargo make test-macos-native-host-stage
./scripts/build_and_run.sh --verify
```

Native-host local loop:

- `scripts/build_and_run.sh` builds `rsnap-host-ffi`, builds `native/macos-host/`, stages
  `target/rsnap-native-host/rsnap.app`, and launches it as a real `.app` bundle.
- On macOS, `cargo run -p rsnap` now delegates to that staged native host bundle instead of
  starting a legacy Rust-owned capture runtime.
- `.codex/environments/environment.toml` points the Codex app Run button at that script.
- `cargo make test-host-reset` now includes the native-host bridge probe in addition to the Rust
  core and header checks.
- `scripts/build_and_run.sh stage` stages the native host bundle without launching it; this is the
  macOS packaging entrypoint used by release automation.
- The live native-host path is now driven by `primary interaction -> scene.liveSelectionPreview ->
  requestFreezeSnapshot`, so the host no longer keeps its own pending freeze-selection shadow
  state.

Smoke/perf entrypoints:

```sh
scripts/smoke/replay-scroll-capture.sh
scripts/smoke/replay-scroll-capture-self-check.sh
scripts/smoke/analyze-scroll-capture-trace.sh
scripts/smoke/live-loupe-perf-macos.sh
scripts/smoke/live-loupe-perf-self-check-macos.sh
scripts/smoke/self-check-macos.sh
scripts/smoke/macos.sh
scripts/perf/local.sh
scripts/perf/self-check-macos.sh
scripts/perf/macos.sh
```

For durable command selection, verification order, baseline workflow, and asset ownership:

- `docs/runbook/performance-validation.md`
- `docs/reference/smoke-perf-validation-surface.md`

The capture-session contract lives at `docs/spec/capture-session.md`.

## Workspace Layout

The tracked workspace currently keeps:

- `native/macos-host/` as the new AppKit-first macOS host shell and local run target
- `apps/rsnap/` as the thin launcher/bootstrap crate for the native host bundle
- `packages/rsnap-overlay/` as the large transitional overlay/runtime container
- `packages/rsnap-capture-core/` as the new durable product-semantics layer
- `packages/rsnap-host-ffi/` as the new thin C ABI bridge for future native hosts

Generated or local-only directories such as `target/`, `.worktrees/`, and `.workspaces/` are not
part of the tracked repository structure. For the authoritative layout and ownership map, read
`docs/reference/workspace-layout.md`.

## Documentation

- Product and development overview: this `README.md`
- Unified documentation router: `docs/index.md`
- Normative specs: `docs/spec/index.md`
- Procedural runbooks: `docs/runbook/index.md`
- Current implementation references: `docs/reference/index.md`
- Durable design rationale: `docs/decisions/index.md`
- Documentation policy and placement rules: `docs/policy.md`

## Support Me

If you find this project helpful and would like to support its development, you can buy me a coffee!

Your support is greatly appreciated and motivates me to keep improving this project.

- **Fiat**
    - [Ko-fi](https://ko-fi.com/hack_ink)
    - [Afdian](https://afdian.com/a/hack_ink)
- **Crypto**
    - **Bitcoin**
        - `bc1pedlrf67ss52md29qqkzr2avma6ghyrt4jx9ecp9457qsl75x247sqcp43c`
    - **Ethereum**
        - `0x3e25247CfF03F99a7D83b28F207112234feE73a6`
    - **Polkadot**
        - `156HGo9setPcU2qhFMVWLkcmtCEGySLwNqa3DaEiYSWtte4Y`

Thank you for your support!

## Appreciation

We would like to extend our heartfelt gratitude to the following projects and contributors:

- The Rust community for their continuous support and development of the Rust ecosystem.

## Additional Acknowledgements

- TODO

<div align="right">

### License

<sup>Licensed under [GPL-3.0](LICENSE).</sup>

</div>
