<div align="center">

# Rsnap

macOS-first screenshot app built with a native host and Rust core.

[![License](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Language Checks](https://github.com/hack-ink/rsnap/actions/workflows/language.yml/badge.svg?branch=main)](https://github.com/hack-ink/rsnap/actions/workflows/language.yml)
[![Release](https://github.com/hack-ink/rsnap/actions/workflows/release.yml/badge.svg)](https://github.com/hack-ink/rsnap/actions/workflows/release.yml)
[![GitHub tag (latest by date)](https://img.shields.io/github/v/tag/hack-ink/rsnap)](https://github.com/hack-ink/rsnap/tags)
[![GitHub last commit](https://img.shields.io/github/last-commit/hack-ink/rsnap?color=red&style=plastic)](https://github.com/hack-ink/rsnap)
[![GitHub code lines](https://tokei.rs/b1/github/hack-ink/rsnap)](https://github.com/hack-ink/rsnap)

</div>

https://github.com/user-attachments/assets/ff2fe84f-f551-40e8-919c-66ae8a61f8e7

## Feature Highlights

- Menubar-only app (no Dock icon) on macOS.
- Global hotkey: `Alt+X` (macOS: Option+X).
- Transparent capture-session overlay that blocks desktop interaction.
- HUD near the cursor showing global `x,y` and `rgb(r,g,b)`.
- Left click + drag freezes a selected region; a single left click freezes the hovered window or falls back to the active monitor fullscreen.
- On macOS, entering Frozen mode restores the pre-capture target after selection completes instead of leaving Rsnap focused; exiting capture restores the original frontmost app.
- In Frozen mode, a dragged-region capture can be dragged from inside the bright selection area to reposition it without resizing.
- In Frozen mode, `Space` copies the current frozen PNG to the clipboard and exits.
- In Frozen mode, Cmd+S (macOS) / Ctrl+S saves the current PNG to disk and exits.
- On macOS, Frozen mode can recognize text from the current capture and copy the result to the clipboard from the toolbar.
- Frozen toolbar tools include pointer, pen, arrow, text, mosaic, spotlight, undo, redo, auto-center,
  OCR, copy, and save.
- `Esc` cancels capture.
- Glass HUD with Classic Glass fallback and Liquid Glass in release builds on supported macOS.
- Tab-triggered loupe sample and frozen-mode toolbar for quick action access.

## Status

Prototype / in active development.

## App identity

- Product display name: **Rsnap**.
- macOS app bundle name: **Rsnap.app**.
- Lower-case `rsnap` remains only for stable technical identifiers such as the repository slug,
  Cargo package names, crate paths, environment variables, bundle identifiers, telemetry schemas,
  and C ABI symbols. See `docs/spec/app-identity.md`.

## Reset posture

- Shipping behavior on macOS now enters through the native host lane:
  - `native/macos-host/` is the SwiftPM AppKit host shell and the default local Run path.
  - `apps/rsnap/` is now a thin launcher/bootstrap crate that stages or opens the native host
    bundle and records startup logging.
  - `packages/rsnap-overlay/` remains a transitional Rust implementation container for reset
    slices that have not yet moved into `rsnap-capture-core` / the native host.
- The active reset target is no longer a pure-Rust UI stack. New boundary crates now live in:
  - `packages/rsnap-capture-core/` for platform-neutral session semantics and host/core protocol
    models, plus Rust-owned export, capture-frame rendering, wallpaper thumbnail, minimap,
    selection-transform, and image-analysis algorithms
  - `packages/rsnap-host-ffi/` for the thin C ABI that the native macOS host calls
    through `packages/rsnap-host-ffi/include/rsnap_host_ffi.h`
- Current version support remains **macOS only**. Windows and Linux stay out of scope for this
  version beyond protocol and abstraction design.

## Capture platform support

- Live sampling path: **macOS 12.3+** via ScreenCaptureKit. Live loupe/window
  sampling uses `SCStream`.
- Live mode is stream-first and does not capture full display on cursor movement.
- Frozen capture imagery on macOS uses the native capture stack;
  `docs/spec/capture-session.md` is the current contract source of truth.
- Menubar and Dock are not included in live window-outline targeting.
- Windows support is planned (minimum Windows 10), but not implemented yet.
- The scroll-capture engine, deterministic replay, and benchmark surfaces remain in the repository,
  but the v0.1.7 native-host release does not expose scroll capture in the toolbar.

## Usage

### Installation

#### Download macOS Build

Download the latest macOS zip:

<https://github.com/hack-ink/rsnap/releases/latest/download/rsnap-aarch64-apple-darwin.zip>

Unzip it and move `Rsnap.app` to `/Applications`.

Release builds include Sparkle-based updates. Use `Settings...` -> `About` -> `Check` for the
standard macOS update flow; the About Auto Update mode defaults to `Install` for signed release builds
with the Sparkle appcast configured.

#### Build from Source

```sh
git clone https://github.com/hack-ink/rsnap
cd rsnap

cargo build --workspace
cargo run -p rsnap
```

#### macOS Gatekeeper approval for signed but unnotarized builds

Current preview release builds are signed, but may not be notarized by Apple. If macOS blocks
`Rsnap.app` after you unzip a downloaded build, use the quarantine override only for a bundle you
built yourself or downloaded from this repository's GitHub Releases page.

Move the app to `/Applications`, then run:

```sh
xattr -rd com.apple.quarantine /Applications/Rsnap.app
```

If Terminal reports a permission error, grant Terminal Full Disk Access in `System Settings` ->
`Privacy & Security` -> `Full Disk Access`, then rerun the command. If you keep the app in a
different location, replace `/Applications/Rsnap.app` with that bundle path.

After Gatekeeper allows the app to open, continue with Screen Recording permission below.

### macOS permissions

Rsnap currently relies on **Screen Recording** permission to capture other apps/windows.
- ScreenCaptureKit live sampling on macOS requires macOS 12.3+ and Screen Recording permission.
- Normal region/window/monitor capture does not require Accessibility or Input Monitoring.
- The retained scroll-capture path uses Screen Recording-backed screenshots plus forwarded wheel
  input, but the v0.1.7 native-host release does not expose scroll capture in the toolbar.
- macOS may describe Screen Recording as `Screen & System Audio Recording` or as direct screen/audio access when Rsnap bypasses the system picker.
- Settings -> Permissions shows Screen Recording as the only required permission.
- Normal native capture depends on Screen Recording; if access is missing, Rsnap opens the Screen Recording page in System Settings and shows a floating drag-to-grant guide.
- You can reopen the Permissions section from `Settings…` in the tray or menubar menu at any time.
- Base capture path: `System Settings` -> `Privacy & Security` -> `Screen Recording`.
- Enable `Rsnap.app`, then retry capture. If macOS still keeps capture blocked after changing a permission, relaunch the app.

### HUD settings behavior

- The native `Settings…` window currently owns:
  - HUD glass enable/disable
  - HUD glass style (`Classic Glass` / `Liquid Glass`)
  - Liquid Glass style (`Regular` / `Clear`) when supported by macOS and the current build
  - shared HUD tint / color, plus Classic Glass opacity / blur
  - HUD `Tab` hint visibility
  - loupe sample size (`small` / `medium` / `large`)
  - output directory
  - filename prefix
  - output naming (`timestamp` / `sequence`)
  - frozen toolbar placement (`bottom` / `top`)

### Output (save-to-disk)

- In Frozen mode, use Cmd+S (macOS) / Ctrl+S to save a PNG to disk and exit.
- On macOS, use the frozen toolbar `Recognize Text` action to copy recognized text from the current frozen capture and exit.
- Output is configured in the native `Settings…` window:
  - `Save Location` (default: Desktop)
  - `Filename Prefix` (default: `Rsnap`, sanitized to `[A-Za-z0-9_-]`)
  - `Naming` (`Timestamp` or `Sequence`)
  - `Frame Preset` (`Off`, wallpaper, or gradient backgrounds)
  - `Apply To` for drag-region and window captures; fullscreen captures are excluded

### Current scroll-capture status

Scroll capture is temporarily hidden in the v0.1.7 native-host release. The retained Rust
scroll-capture session, deterministic replay, and benchmark surfaces remain for validation and
future re-enablement, but users should not expect a `Scroll Capture` toolbar item in this release.

## Development

```sh
cargo make checks
cargo make test-host-reset
cargo make test-macos-native-host-stage
./scripts/build_and_run.sh --verify
```

Native-host local loop:

- `scripts/build_and_run.sh` builds `rsnap-host-ffi`, builds `native/macos-host/`, stages
  `target/rsnap-native-host/Rsnap.app`, and launches it as a real `.app` bundle.
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
- The native host keeps OS-facing ownership. It captures or discovers platform resources, passes
  source pixels and resource paths through `RsnapHostBridge`, displays returned images, and performs
  clipboard, save-panel, OCR, and update side effects. Rust owns the final-byte and image-planning
  algorithms exposed through `rsnap-host-ffi`.

Smoke/perf entrypoints:

```sh
scripts/smoke/replay-scroll-capture.sh
scripts/smoke/replay-scroll-capture-self-check.sh
scripts/smoke/analyze-scroll-capture-trace.sh
scripts/smoke/native-hud-follow-macos.sh
scripts/smoke/self-check-macos.sh
scripts/smoke/macos.sh
scripts/smoke/sparkle-update-local.sh
scripts/perf/local.sh
scripts/perf/self-check-macos.sh
scripts/perf/macos.sh
```

`scripts/smoke/macos.sh` and `scripts/perf/macos.sh` run the native-host HUD-follow smoke
plus recorded scroll-capture replay.

For durable command selection, verification order, baseline workflow, and asset ownership:

- `docs/runbook/performance-validation.md`
- `docs/reference/smoke-perf-validation-surface.md`
- `docs/runbook/validate-release.md`

The capture-session contract lives at `docs/spec/capture-session.md`.

## Workspace Layout

The tracked workspace currently keeps:

- `native/macos-host/` as the new AppKit-first macOS host shell and local run target
- `apps/rsnap/` as the thin launcher/bootstrap crate for the native host bundle
- `packages/rsnap-overlay/` as the large transitional overlay/runtime container
- `packages/rsnap-capture-core/` as the durable product-semantics and image-algorithm layer
- `packages/rsnap-host-ffi/` as the thin C ABI bridge used by the native macOS host

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
