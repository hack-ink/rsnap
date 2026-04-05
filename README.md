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
- In Frozen mode, a dragged-region capture can be dragged from inside the bright selection area to reposition it without resizing.
- In Frozen mode, `Space` copies the current frozen PNG to the clipboard and exits.
- In Frozen mode, Cmd+S (macOS) / Ctrl+S saves the current PNG to disk and exits.
- On macOS, Frozen mode can recognize text from the current capture and copy the result to the clipboard from the toolbar.
- After a dragged region freeze, press `s` or use the frozen toolbar `Scroll Capture ↓` action to enter scroll capture.
- Scroll capture is currently implemented on macOS for dragged-region freezes and uses image-first downward stitching with a live side preview.
- Upward scrolling may be observed for rewind/reacquire, but it never appends stitched rows.
- `Esc` cancels capture; during scroll capture, `Esc` / `Back` returns to normal Frozen mode.
- Glass HUD with configurable blur, tint, and hue controls.
- Tab-triggered loupe sample and frozen-mode toolbar for quick action access.

## Status

Prototype / in active development.

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
- macOS may phrase the Input Monitoring prompt as receiving keystrokes from any application even though rsnap only listens for scroll-wheel input in this path.
- macOS may describe Screen Recording as `Screen & System Audio Recording` or as direct screen/audio access when rsnap bypasses the system picker.
- On startup, `rsnap` checks Screen Recording, Accessibility, and Input Monitoring together and opens its own Settings window if any of them are missing.
- In the app, the Permissions section shows Screen Recording, Accessibility, and Input Monitoring status. Each permission button can still issue the matching macOS request when the system allows it, then open the relevant macOS settings pane if access is still missing.
- Normal capture does not issue a just-in-time permission request or reopen Settings when Screen Recording is missing.
- Scroll capture does not issue a just-in-time permission request or show a HUD permission message when Accessibility or Input Monitoring is missing.
- You can reopen `Permissions…` from the tray or menubar menu at any time.
- Base capture path: `System Settings` -> `Privacy & Security` -> `Screen Recording`.
- Scroll capture paths: `System Settings` -> `Privacy & Security` -> `Accessibility` and `Input Monitoring`.
- Enable `rsnap` (the built `.app`), then retry capture. If macOS still keeps capture blocked after changing a permission, relaunch the app.

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
- On macOS, use the frozen toolbar `Recognize Text` action to copy recognized text from the current frozen capture and exit.
- After entering scroll capture from a dragged region on macOS, downward scrolling may append newly proven rows into the side preview.
  Upward scrolling never appends. Returning to already-stitched content should not grow the export; only newly proven content may be added.
  The scroll-capture commit path uses discrete region screenshots plus pairwise image registration; clipboard and save must match the committed preview the user sees.
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

Scroll-capture verification now starts with deterministic replay instead of the old GUI smoke:

```sh
cargo make replay-scroll-capture
cargo make replay-scroll-capture-self-check
```

For semantic trace analysis (first bad frame, under-consumption, overshoot), use:

```sh
cargo make analyze-scroll-capture-trace
```

The remaining macOS GUI smoke harnesses are still available for live-loupe and
desktop-session checks:

```sh
cargo make smoke-self-check-macos
cargo make smoke-macos
```

`cargo make replay-scroll-capture` and `cargo make analyze-scroll-capture-trace`
now force the latest recorded live trace through the same worker-pairwise
commit path that current macOS production scroll capture uses. They are
trace-driven rather than scenario-driven, so they expect at least one recorded
trace under `~/Library/Application Support/ink.hack.rsnap/scroll-capture-traces/`
unless you pass `--trace <manifest-path>` directly to the example. Use the
direct example without `--force-worker-pairwise` only when you intentionally
want to compare the legacy recorded-source replay mode. `cargo make
replay-scroll-capture-self-check` is the repo-local fallback when you want to
verify the replay harness itself without relying on a user-recorded trace.
`cargo make smoke-self-check-macos` and `cargo make smoke-macos` still drive
the logged-in macOS live-loupe smoke path and require the expected Screen
Recording / automation permissions.

For `XY-185` style downward scroll-capture work, treat the verification order as:

1. deterministic tests and `cargo make check`
2. `cargo make replay-scroll-capture`
3. `cargo make analyze-scroll-capture-trace`
4. one fresh release live touchpad run with a newly recorded trace

Repo-native performance entrypoints are available for deterministic benches and
dedicated smoke:

```sh
cargo make perf-local
cargo make perf-self-check-macos
cargo make perf-macos
```

Use `cargo make perf-local` for component-render and scroll-capture regressions
that should stay comparable on a normal development machine. Use
`cargo make perf-self-check-macos` to validate the dedicated macOS smoke
environment, and `cargo make perf-macos` only on a logged-in desktop session
when you need end-to-end GUI performance evidence. The durable runbook for
command selection and baseline comparison lives at
`docs/runbook/performance-validation.md`.

The capture-session contract lives at `docs/spec/capture-session.md`.

## Workspace Layout

The tracked workspace is intentionally small:

- `apps/rsnap/`: desktop app shell, tray/menubar lifecycle, hotkeys, settings window, logging,
  permissions, and session handoff into `rsnap-overlay`
- `packages/rsnap-overlay/`: overlay session crate, capture/runtime internals, rendering, worker
  flow, OCR, and scroll capture
- `docs/`: repository docs split into `spec`, `runbook`, `reference`, and `decisions`
- `assets/`: shared app and tray icon sources plus generated runtime/bundle assets
- `scripts/`: packaging and dedicated macOS smoke helpers

Generated or local-only directories such as `target/`, `.worktrees/`, and `.workspaces/` are not
part of the tracked repository structure.

For code-ownership and directory-routing details, read `docs/reference/workspace-layout.md`.

## Documentation

- Product and development overview: this `README.md`
- Workspace layout and crate boundaries: `docs/reference/workspace-layout.md`
- Runtime behavior contract: `docs/spec/capture-session.md`
- Performance contract: `docs/spec/performance.md`
- Procedural runbooks: `docs/runbook/`
- Current implementation references: `docs/reference/`
- Durable design rationale: `docs/decisions/`
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

<div align="right">

### License

<sup>Licensed under [GPL-3.0](LICENSE).</sup>

</div>
