---
title: "Settings and App Shell Contract"
description: "Settings and App Shell Contract documentation for Rsnap."
type: "Spec"
status: active
authority: normative
owner: acgxv/rsnap
last_verified: 2026-07-08
---
# Settings and App Shell Contract

Purpose: Define the normative Settings, status-menu, shortcut, permission, and macOS app-shell
behavior for Rsnap.

Status: normative

Read this when: You are implementing, reviewing, or validating Settings, status-menu commands,
shortcut presentation, launch-at-login configuration, permission recovery, Dock activation policy,
or Settings window behavior.

Not this document: Detailed visual styling, implementation-specific AppKit/SwiftUI structure, or
capture-session behavior once capture has started. Use `docs/spec/capture-session.md` for
capture-flow behavior and `docs/spec/platform-host-boundary.md` for host/core ownership.

Defines:
- Settings window and app-shell behavior
- status-menu command placement
- shortcut configuration and presentation rules
- launch-at-login configuration
- permission recovery placement
- Settings interaction and default-size usability invariants

## App Shell

- Rsnap is a background menubar app while Settings is closed: it must not keep a Dock icon visible
  in the idle menubar state.
- Opening Settings may temporarily promote Rsnap to an ordinary app-window activation state so the
  Settings window participates in normal macOS focus, keyboard, and window management.
- Closing Settings must return Rsnap to the background menubar state when no other ordinary app
  window is visible.
- Capture sessions must not require visible Dock activation artifacts to begin, complete, cancel,
  copy, save, or restore focus.
- Settings must expose an Open at Login control that registers or unregisters Rsnap with macOS
  Login Items.
- The Open at Login control must reflect system Login Items state, including pending approval or an
  unavailable packaged-app context, instead of only reflecting a stored user default.

## Status Menu

- The status menu must expose New Screenshot, Quick Screenshot, Open Screenshots Folder,
  Check for Updates, Settings, and Quit.
- Open Screenshots Folder must open the configured output directory, creating it first when needed.
- Check for Updates must invoke the same Sparkle-backed update check flow as the About section.
- The status menu must not expose Cancel Capture. Capture cancellation is handled in-session by
  `Esc` and secondary click in both live and Frozen modes.
- The status menu must not expose Permissions as a separate menu item. Permission status and
  recovery live in Settings.
- New Screenshot must use the same configured shortcut as the global capture hotkey.
- Quick Screenshot must use the same configured shortcut as the quick screenshot hotkey.

## Settings Window

- Settings must be an ordinary top-level platform window, not only an overlay surface.
- Settings must be selectable by system screenshot/window-picking tools and by Rsnap's own window
  selector so normal selection effects can apply to it.
- Settings must support standard macOS shortcuts: `Command-W` closes the Settings window and
  `Command-Q` quits Rsnap.
- The Settings window background must not globally hijack drag gestures. Draggable controls and
  component hit testing must receive their own pointer gestures.

## Shortcut Settings

- The capture shortcut and quick screenshot shortcut configuration must be present in Settings.
- Shortcut display strings must use canonical platform names such as `Option-X` and
  `Option-Shift-X`.
- Raw event spellings such as `alt+KeyX` must not appear in user-facing shortcut fields,
  summaries, or menu shortcut labels.
- Shortcut controls must use click-to-listen behavior instead of requiring users to type raw
  shortcut strings. Clicking a shortcut value arms capture for the next supported key press,
  commits the canonical platform display string, and persists the result immediately.
- While a shortcut control is listening, `Esc`, the next mouse-down in Settings, or losing
  Settings-window focus must cancel listening without changing the stored shortcut.
- The default capture shortcut is `Option-X`.
- The default quick screenshot shortcut is `Option-Shift-X`.
- In live capture, plain `Tab` toggles the loupe on and off. Hold-to-show Tab behavior is not a
  supported setting.

## Default Configuration

- Capture shortcut: `Option-X`.
- Quick screenshot shortcut: `Option-Shift-X`.
- Open at Login: off, until the user enables the macOS Login Items registration.
- Output directory: `~/Desktop`.
- Output filename prefix: `Rsnap`.
- Output naming: timestamp.
- Capture frame preset: Off.
- Capture frame apply-to: window.
- Frozen toolbar placement: bottom.
- Frozen resize handles: outward.
- Live HUD hint keycap: enabled.
- HUD glass: enabled, `liquid_glass`, `clear`.
- `liquid_glass` resolves to Classic Glass when the native host is running before macOS 26 or
  was built without Liquid Glass API support.
- HUD opacity: `0.4999747693194925`.
- HUD blur: `0.5032628676470589`.
- HUD tint: `0.3034524356617647`.
- HUD tint hue: `0.6399984993939073`.
- HUD tint saturation: `0.9915479812300032`.
- HUD tint brightness: `0.5749928951`.
- Loupe sample size: small.
- Sparkle update mode: install in release builds.

## Output Settings

- The Output section must expose save location, filename prefix, naming, frame preset, and frame
  applicability controls.
- Frame Preset must be a single control with Off and the available background presets. Off disables
  the capture frame effect without requiring a separate Export Frame toggle.
- Frame Preset must render as a compact horizontal swatch selector. Off is represented by an empty
  slash swatch, and background presets are represented by small background thumbnails without
  visible option labels. Preset growth must preserve swatch size instead of shrinking cards to fit;
  overflow must remain mouse-accessible through lightweight step controls, including click-and-hold
  repeated movement for long preset lists.
- Wallpaper swatches must not synchronously decode full wallpaper files in Swift. Swift may discover
  the current wallpaper path and present pixels, but bounded wallpaper thumbnail decoding and caching
  are Rust-owned.
- Apply To must be disabled while Frame Preset is Off.
- Capture frame effects may apply to drag-region captures, window captures, or both. Scroll capture
  follows drag-region applicability, and fullscreen captures are excluded from this setting.

## Permission Settings

- Settings must include a Permissions section.
- Screen Recording permission is required for the current native macOS capture host.
- Settings must present Screen Recording as required for native capture and Scroll Capture.
- Settings must not present Accessibility or Input Monitoring as required for Scroll Capture; the
  current product path uses overlay-local wheel forwarding rather than Accessibility target control
  or a CGEvent tap.
- Quick Screenshot may use a platform event tap for no-focus shortcut and selection input. If the
  platform refuses that event tap, Quick Screenshot must fail closed and emit diagnostic telemetry;
  Settings must not present event-tap access as a required capture permission unless a dedicated
  recovery flow exists.
- The Open at Login control must live at the bottom of the Permissions section so OS-owned app
  access controls remain first.
- When Screen Recording is missing at launch or at capture start, Rsnap must open the macOS Screen
  Recording privacy page and present a small Rsnap-owned floating drag guide near System Settings.
- Permission recovery should provide a visible drag-the-app affordance for adding Rsnap to System
  Settings where macOS allows that workflow, including a directional guide from the floating window
  toward System Settings and an Open System Settings fallback.
- Permission status refresh must be available without restarting Rsnap.

## About Settings

- Settings must include an About section.
- The About section must identify Yvette Cipher as the creator and describe Rsnap as an
  open-source macOS capture tool.
- The About section must include external links to `https://github.com/acgxv/rsnap` and
  `https://x.com/hackink`.
- The X link may encourage following for ongoing Rsnap updates and may state that follows
  help support future work.
- Release builds must use Sparkle's standard updater UI and appcast format for macOS self-updates.
  GitHub Releases remains the distribution surface, but the Sparkle appcast at
  `https://github.com/acgxv/rsnap/releases/latest/download/appcast.xml` is the update-version
  authority for in-app update checks.
- The appcast must compare against the running app bundle's `CFBundleVersion`. The user-visible
  version should remain `CFBundleShortVersionString`.
- The About section must expose a Check for Updates action backed by Sparkle's standard check
  flow. When an installable update is available, Sparkle owns the native update window, download
  progress, install authorization if needed, and final install-and-relaunch action.
- The Check for Updates action must remain user-available even while Sparkle reports that a
  session is active, downloading, or preparing an update. That state must not gray out the user's
  entry point back into the update flow.
- The About section must expose one Auto Update mode control rather than separate Automatic Checks
  and Automatic Updates rows. The visible modes are Off, Notify, and Install.
- Off must disable Sparkle automatic checks and automatic downloads. Notify must enable Sparkle
  automatic checks without automatic downloads. Install must enable automatic checks and Sparkle's
  `automaticallyDownloadsUpdates` setting when automatic updates are available.
- Sparkle must use a 24-hour scheduled check interval, and each fresh app launch should request one
  immediate background check after the updater starts when the selected mode is Notify or Install.
- Install mode must keep Sparkle's automatic download/install behavior enabled and must invoke
  Sparkle's immediate install-and-relaunch handler after an automatic update is prepared. If a
  capture, quick screenshot, Settings window, or permission recovery guide is active, Rsnap must
  defer that handler until Rsnap returns to its idle menubar state. Rsnap must not add a separate
  custom update prompt for this automatic path.
- The Auto Update secondary text must use sentence case, must not read like download or install
  progress, and should display Sparkle's last successful check time while Notify or Install is
  selected. When Sparkle is not configured in a development build, the secondary text may state
  that the signed appcast is not configured.
- The About section must not display last checked as a separate row.
- The About section must not expose capture defaults or a Restore Defaults action.

## Default-Size Usability

- At the default Settings window size, primary setting labels, selected option labels, and current
  value summaries must not be truncated.
- Segmented controls and pill-like options must make the full visible option area clickable, not
  only the text glyphs.
- Settings must remain usable and legible in both light and dark macOS appearances.
