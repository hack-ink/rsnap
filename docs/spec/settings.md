# Settings and App Shell Contract

Purpose: Define the normative Settings, status-menu, shortcut, permission, and macOS app-shell
behavior for Rsnap.

Status: normative

Read this when: You are implementing, reviewing, or validating Settings, status-menu commands,
shortcut presentation, permission recovery, Dock activation policy, or Settings window behavior.

Not this document: Detailed visual styling, implementation-specific AppKit/SwiftUI structure, or
capture-session behavior once capture has started. Use `docs/spec/capture-session.md` for
capture-flow behavior and `docs/spec/platform-host-boundary.md` for host/core ownership.

Defines:
- Settings window and app-shell behavior
- status-menu command placement
- shortcut configuration and presentation rules
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

## Status Menu

- The status menu must expose New Capture, Settings, and Quit.
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

## Permission Settings

- Settings must include a Permissions section.
- Screen Recording permission is required for the current native macOS capture host.
- Settings must present Screen Recording as the only permission needed by the current native macOS
  capture host.
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
- The About section must not expose capture defaults or a Restore Defaults action.

## Default-Size Usability

- At the default Settings window size, primary setting labels, selected option labels, and current
  value summaries must not be truncated.
- Segmented controls and pill-like options must make the full visible option area clickable, not
  only the text glyphs.
- Settings must remain usable and legible in both light and dark macOS appearances.
