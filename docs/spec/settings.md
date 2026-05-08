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

- The status menu must expose New Capture, Open Screenshots Folder, Check for Updates, Settings,
  and Quit.
- Open Screenshots Folder must open the configured output directory, creating it first when needed.
- Check for Updates must invoke the same Sparkle-backed update check flow as the About section.
- The status menu must not expose Cancel Capture. Capture cancellation is handled in-session by
  `Esc` and secondary click in both live and Frozen modes.
- The status menu must not expose Permissions as a separate menu item. Permission status and
  recovery live in Settings.
- New Capture must use the same configured shortcut as the global capture hotkey.

## Settings Window

- Settings must be an ordinary top-level platform window, not only an overlay surface.
- Settings must be selectable by system screenshot/window-picking tools and by Rsnap's own window
  selector so normal selection effects can apply to it.
- Settings must support standard macOS shortcuts: `Command-W` closes the Settings window and
  `Command-Q` quits Rsnap.
- The Settings window background must not globally hijack drag gestures. Draggable controls and
  component hit testing must receive their own pointer gestures.

## Shortcut Settings

- The capture shortcut configuration must be present in Settings.
- Shortcut display strings must use canonical platform names such as `Option-X`.
- Raw event spellings such as `alt+KeyX` must not appear in user-facing shortcut fields,
  summaries, or menu shortcut labels.
- The default capture shortcut is `Option-X`.
- In live capture, plain `Tab` toggles the loupe on and off. Hold-to-show Tab behavior is not a
  supported setting.

## Default Configuration

- Capture shortcut: `Option-X`.
- Open at Login: off, until the user enables the macOS Login Items registration.
- Output directory: `~/Desktop`.
- Output filename prefix: `Rsnap`.
- Output naming: timestamp.
- Frozen toolbar placement: bottom.
- Frozen resize handles: outward.
- Live HUD hint keycap: enabled.
- HUD glass: enabled, `liquid_glass`, `clear`.
- `liquid_glass` resolves to Classic Glass when the native host is running before macOS 26 or
  was built without Liquid Glass API support.
- HUD opacity: `0.4999747693194925`.
- HUD blur: `0.5032628676470589`.
- HUD tint: `0.4990234375`.
- HUD tint hue: `0.6074879184861536`.
- Loupe sample size: small.
- Sparkle update mode: install in release builds.

## Permission Settings

- Settings must include a Permissions section.
- Screen Recording permission is required for the current native macOS capture host.
- Settings must present Screen Recording as the only permission needed by the current native macOS
  capture host.
- The Open at Login control must live at the bottom of the Permissions section so OS-owned app
  access controls remain first.
- When Screen Recording is missing at launch or at capture start, Rsnap must open the macOS Screen
  Recording privacy page and present a small Rsnap-owned floating drag guide near System Settings.
- Accessibility and Input Monitoring must not be displayed in Settings while the current native host
  does not need them.
- Permission recovery should provide a visible drag-the-app affordance for adding Rsnap to System
  Settings where macOS allows that workflow, including a directional guide from the floating window
  toward System Settings and an Open System Settings fallback.
- Permission status refresh must be available without restarting Rsnap.

## About Settings

- Settings must include an About section.
- The About section must identify Yvette Cipher as the creator and describe Rsnap as an
  open-source macOS capture tool.
- The About section must include external links to `https://github.com/hack-ink/rsnap` and
  `https://x.com/YvetteCipher`.
- The creator link may encourage following for ongoing Rsnap updates and may state that follows
  help support future work through X creator rewards.
- Release builds must use Sparkle's standard updater UI and appcast format for macOS self-updates.
  GitHub Releases remains the distribution surface, but the Sparkle appcast at
  `https://github.com/hack-ink/rsnap/releases/latest/download/appcast.xml` is the update-version
  authority for in-app update checks.
- The appcast must compare against the running app bundle's `CFBundleVersion`. The user-visible
  version should remain `CFBundleShortVersionString`.
- The About section must expose a Check for Updates action backed by Sparkle's standard check
  flow. When an installable update is available, Sparkle owns the native update window, download
  progress, install authorization if needed, and final install-and-relaunch action.
- The About section must expose one Auto Update mode control rather than separate Automatic Checks
  and Automatic Updates rows. The visible modes are Off, Notify, and Install.
- Off must disable Sparkle automatic checks and automatic downloads. Notify must enable Sparkle
  automatic checks without automatic downloads. Install must enable automatic checks and Sparkle's
  `automaticallyDownloadsUpdates` setting when automatic updates are available.
- Sparkle must use a 24-hour scheduled check interval, and each fresh app launch should request one
  immediate background check after the updater starts when the selected mode is Notify or Install.
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
