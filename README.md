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
- Left click freezes the active monitor as a fullscreen screenshot.
- Space copies the frozen screenshot PNG to the clipboard and exits.
- Esc cancels and exits.
- Glass HUD with configurable blur, tint, and hue controls.
- Alt-triggered loupe sample and frozen-mode toolbar for quick action access.

## Status

Prototype / in active development.

## Capture platform support

- Live sampling path: **macOS 12.3+** via ScreenCaptureKit (`SCStream`) stream samples.
- Live mode is stream-first and does not capture full display on cursor movement.
- Freeze/export still uses `xcap` capture.
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

- Go to `System Settings` -> `Privacy & Security` -> `Screen Recording`.
- Enable `rsnap` (the built `.app`), then relaunch the app.

### HUD settings behavior

- HUD controls are in Settings → Overlay:
  - Opacity (`0..100`, default `75`)
  - Blur (`0..100`, default `25`)
  - Tint (`0..100`, default `0`)
  - Hue (`0..360`, system-blue default)
- Tint is applied as hue-shift intensity (0 = no tint, 100 = full tint), while Hue sets
  target color.
- Numeric entry accepts plain integers for percent/degree fields and updates immediately.
- Same HUD style settings are used by main HUD, loupe, and frozen toolbar.

## Development

```sh
cargo make fmt
cargo make lint
cargo make test
```

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
