# rsnap

Menubar-only app (tray icon + menu) that triggers `rsnap-overlay` capture and writes the result to the clipboard (Space) or saves to disk (Cmd+S / Ctrl+S).

## Capture platform support

- Live sampling: **macOS 12.3+** via ScreenCaptureKit (`SCStream`) stream samples.
- Live mode is stream-first and does not take full-frame captures on cursor movement.
- Menubar and Dock are excluded from live outline targeting.
- Freeze/export remains on `xcap` for now.
- Windows is planned (minimum Windows 10) and is not implemented yet.

## Logs

- Runtime logs are written to `ProjectDirs` data directory under `logs/` (on macOS this maps to `~/Library/Application Support/ink.hack.rsnap/logs`).
- Log files rotate daily and keep up to 15 files.
- If file logging cannot start (for example directory permission issues), rsnap falls back to console logging.
- Set `RUST_LOG` or set `log_filter` in `settings.toml` to increase verbosity, for example `rsnap=debug,rsnap_overlay=debug`.

## Hotkey

- Global hotkey: `Alt+X`

## macOS Dock icon

This crate attempts to avoid showing a Dock icon at runtime by setting the app activation policy to `Accessory` and hiding Dock visibility.

For the most reliable “no Dock icon” behavior when distributing a bundled `.app`, also set `LSUIElement=1` in the app `Info.plist`.

## Run

`cargo run -p rsnap`
