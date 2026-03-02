# rsnap

Menubar-only app (tray icon + menu) that triggers `rsnap-overlay` capture and writes the result to the clipboard.

## Capture platform support

- Live sampling: **macOS 12.3+** via ScreenCaptureKit (`SCStream`) stream samples.
- Live mode is stream-first and does not take full-frame captures on cursor movement.
- Menubar and Dock are excluded from live outline targeting.
- Freeze/export remains on `xcap` for now.
- Windows is planned (minimum Windows 10) and is not implemented yet.

## Hotkey

- Global hotkey: `Alt+X`

## macOS Dock icon

This crate attempts to avoid showing a Dock icon at runtime by setting the app activation policy to `Accessory` and hiding Dock visibility.

For the most reliable “no Dock icon” behavior when distributing a bundled `.app`, also set `LSUIElement=1` in the app `Info.plist`.

## Run

`cargo run -p rsnap`
