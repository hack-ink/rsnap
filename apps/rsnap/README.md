# rsnap

Menubar-only app (tray icon + menu) that triggers `rsnap-overlay` capture and writes the result to the clipboard (Space) or saves to disk (Cmd+S / Ctrl+S).

## Capture platform support

- Live sampling: **macOS 12.3+** via ScreenCaptureKit (`SCStream`) stream samples.
- Live mode is stream-first and does not take full-frame captures on cursor movement.
- Menubar and Dock are excluded from live outline targeting.
- Frozen capture and scroll-capture imagery on macOS use the native capture stack described in `docs/spec/v0.md`.
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

For packaging, `scripts/bundle-macos.sh` now post-processes the bundled app with Xcode's asset catalog toolchain (`actool`) and compiles the Dock icon directly from `apps/rsnap/assets/app-icon/composer/AppIcon.icon` into `Assets.car`.

The current icon assets are organized as:

- `apps/rsnap/assets/app-icon/source/dock-icon-original.png`: original Dock icon sketch/input
- `apps/rsnap/assets/app-icon/composer/AppIcon.icon`: edited Icon Composer source-of-truth
- `apps/rsnap/assets/app-icon/generated/app-icon.icns`: generated static fallback used by raw `cargo bundle`
- `apps/rsnap/assets/tray-icon/source/tray-icon-original.png`: original tray icon sketch/input
- `apps/rsnap/assets/tray-icon/generated/tray-icon-template.png`: generated macOS template tray icon used at runtime

The one-click macOS bundling script replaces the raw `cargo bundle` fallback with the compiled Icon Composer output.

## macOS CI signing

The tag release workflow signs, notarizes, and staples the macOS `.app` before publishing the zip artifact. Configure these GitHub Actions secrets first:

- `APPLE_CERTIFICATE_P12_BASE64`: base64-encoded `Developer ID Application` certificate export (`.p12`)
- `APPLE_CERTIFICATE_PASSWORD`: password used when exporting the `.p12`
- `APPLE_SIGNING_IDENTITY`: codesign identity, for example `Developer ID Application: Your Name (TEAMID)`
- `APPLE_NOTARY_KEY_ID`: App Store Connect API key ID used by `notarytool`
- `APPLE_NOTARY_KEY_P8`: private key contents (`.p8`) for the App Store Connect API key
- `APPLE_NOTARY_ISSUER`: App Store Connect issuer UUID for team keys; leave unset for individual keys

The workflow builds `Rsnap.app`, signs it with hardened runtime, submits a zip to `notarytool`, staples the notarization ticket back onto the app, and then republishes the stapled `.app` as the macOS release artifact.

## Run

`cargo run -p rsnap`

## Smoke verification

On macOS, prefer the existing workspace smoke harnesses over ad-hoc manual launch checks:

- `cargo make smoke-self-check-macos`
- `cargo make smoke-macos`

These scripts automate tray-triggered capture, live/loupe performance checks, and
scroll-capture stitching assertions in a logged-in desktop session.
