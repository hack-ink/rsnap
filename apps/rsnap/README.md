# rsnap

Menubar-only app (tray icon + menu) that triggers `rsnap-overlay` capture and writes the result to the clipboard.

## Hotkey

- Global hotkey: `Alt+X`

## macOS Dock icon

This crate attempts to avoid showing a Dock icon at runtime by setting the app activation policy to `Accessory` and hiding Dock visibility.

For the most reliable “no Dock icon” behavior when distributing a bundled `.app`, also set `LSUIElement=1` in the app `Info.plist`.

## Run

`cargo run -p rsnap`
