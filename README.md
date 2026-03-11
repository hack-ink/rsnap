<div align="center">

# rsnap

Pure-Rust menubar screenshot prototype (macOS-first).

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
- In Frozen mode, `Space` copies the current frozen PNG to the clipboard and exits.
- In Frozen mode, Cmd+S (macOS) / Ctrl+S saves the current PNG to disk and exits.
- After a dragged region freeze, press `s` or use the frozen toolbar `Scroll Capture ↓` action to enter scroll capture.
- Scroll capture is currently implemented on macOS for dragged-region freezes and uses image-first downward stitching with a live side preview.
- Upward scrolling may be observed for rewind/reacquire, but it never appends stitched rows.
- `Esc` cancels capture; during scroll capture, `Esc` / `Back` returns to normal Frozen mode.
- Glass HUD with configurable blur, tint, and hue controls.
- Alt-triggered loupe sample and frozen-mode toolbar for quick action access.

## Status

Prototype / in active development.

## Capture platform support

- Live sampling path: **macOS 12.3+** via ScreenCaptureKit (`SCStream`) stream samples.
- Live mode is stream-first and does not capture full display on cursor movement.
- Frozen capture and scroll-capture imagery on macOS use the native capture stack; `docs/spec/v0.md` is the current contract source of truth.
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
- Scroll capture availability does not depend on an Accessibility-based scroll binding.
- Default cursor tracking and Option key detection on macOS do not require Accessibility or Input Monitoring permissions.

- Go to `System Settings` -> `Privacy & Security` -> `Screen Recording`.
- Enable `rsnap` (the built `.app`), then relaunch the app.

### HUD settings behavior

- HUD controls are in Settings → Overlay:
  - Opacity (`0..100`, default `75`)
  - Blur (`0..100`, default `25`)
  - Tint (`0..100`, default `0`)
  - Hue (`0..360`, system-blue default)
  - Toolbar placement (`bottom` / `top`, default `bottom`)
- Tint is applied as hue-shift intensity (0 = no tint, 100 = full tint), while Hue sets
  target color.
- Numeric entry accepts plain integers for percent/degree fields and updates immediately.
- Same HUD style settings are used by main HUD, loupe, and frozen toolbar.

### Output (save-to-disk)

- In Frozen mode, use Cmd+S (macOS) / Ctrl+S to save a PNG to disk and exit.
- After entering scroll capture from a dragged region on macOS, downward scrolling may append newly proven rows into the side preview.
  Upward scrolling never appends. Returning to already-stitched content should not grow the export; only newly proven content may be added.
  `Space` copies the stitched image, Cmd+S (macOS) / Ctrl+S saves it, and `Esc` / `Back`
  returns to the original Frozen capture without exiting.
- Output is configured in `settings.toml`:
  - `output_dir` (default: Desktop)
  - `output_filename_prefix` (default: `rsnap`, sanitized to `[A-Za-z0-9_-]`)
  - `output_naming` (`timestamp` (unix ms) or `sequence` (0001))

## Development

```sh
cargo make fmt
cargo make lint
cargo make test
```

macOS GUI smoke harnesses are also available:

```sh
cargo make smoke-self-check-macos
cargo make smoke-macos
```

These scripts drive a logged-in macOS desktop session, require the expected
Screen Recording / automation permissions, and are intended for dedicated smoke
verification runs rather than background CI on a shared desktop session.

The v0 contract lives at `docs/spec/v0.md`.

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
