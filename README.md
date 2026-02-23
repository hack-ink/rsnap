<div align="center">

# rsnap

Xnip-style screenshot utility for macOS + Windows: tray hotkey, native capture overlay, and a
lightweight editor for crop + export.

[![License](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Language Checks](https://github.com/hack-ink/rsnap/actions/workflows/language.yml/badge.svg?branch=main)](https://github.com/hack-ink/rsnap/actions/workflows/language.yml)
[![Release](https://github.com/hack-ink/rsnap/actions/workflows/release.yml/badge.svg)](https://github.com/hack-ink/rsnap/actions/workflows/release.yml)
[![GitHub tag (latest by date)](https://img.shields.io/github/v/tag/hack-ink/rsnap)](https://github.com/hack-ink/rsnap/tags)
[![GitHub last commit](https://img.shields.io/github/last-commit/hack-ink/rsnap?color=red&style=plastic)](https://github.com/hack-ink/rsnap)
[![GitHub code lines](https://tokei.rs/b1/github/hack-ink/rsnap)](https://github.com/hack-ink/rsnap)

</div>

## Status

MVP / in active development.

## Feature highlights (current)

- Tray/menubar app with a global hotkey (default: `Ctrl+Shift+S`)
- Native overlay sidecar:
  - Drag to select a region
  - Click to select a window
  - `Esc` cancels
- Editor actions:
  - Crop inside the editor
  - Copy (PNG) to clipboard
  - Save (PNG) to disk
  - Pin (always-on-top)

## Documentation

- Start here: `docs/index.md`
- Specs:
  - Current behavior (as-is): `docs/spec/system_rsnap_current.md`
  - v1 target behavior (to-be): `docs/spec/system_rsnap_v1.md`

## Usage (high level)

- Use the tray menu or press the global hotkey to start capture.
- Drag for region capture, or click a window for window capture.
- The editor opens for export actions (copy/save/pin).

### macOS permissions

rsnap requires **Screen Recording** permission to capture other apps/windows.

- Go to `System Settings` -> `Privacy & Security` -> `Screen Recording`.
- Enable `rsnap` (the built `.app`), then relaunch the app.

## Development

This repository uses `cargo make` tasks defined in `Makefile.toml`.

Common commands:

```sh
cargo make lint-fix
cargo make fmt
cargo make test
```

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
