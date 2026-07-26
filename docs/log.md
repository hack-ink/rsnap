# Documentation Log

## 2026-07-26

- Added the release distribution contract for source provenance, Developer ID signing,
  mandatory notarization, Sparkle metadata, checksums, and draft publication.
- Changed the release runbook so incomplete notarization credentials and unnotarized packages
  fail closed.
- Recorded `release` as the single GitHub environment for release jobs and named the required
  Rsnap-specific Sparkle secret.
- Updated the canonical repository owner and public URLs to `acg-box/rsnap`.
- Split release build and signing across separate `macos-26` runners so tests and dependencies never
  share a runner with Apple or Sparkle release secrets.
- Required the release tag commit to equal the current `origin/main` tip and prevented stable
  release-version or latest-pointer regression.
- Replaced dependency-provided Sparkle `sign_update` execution with a checked-in CryptoKit Ed25519
  signer.
- Recorded organization-wide release-secret visibility as an accepted single-operator tradeoff and
  added a value-free Infisical topology contract and evidence record.
- Added GitHub rules that limit `v*` tag creation to the operator and make each created release tag
  immutable.
- Recorded that public `v0.3.0` predates the hardened contract, preserves the Rsnap Sparkle key,
  and must not be treated as Developer ID, notarization, staple, or checksum evidence.

## 2026-07-07

- Documented the quick screenshot no-focus contract: quick screenshot must preserve transient target
  UI such as context menus by avoiding overlay activation, key-window promotion, and
  first-responder ownership while keeping acquisition overlays mouse-transparent.
- Recorded the host-side split between ordinary capture cursor ownership through focused AppKit
  overlay views and quick screenshot cursor/input ownership through non-activating acquisition.
- Recorded that ordinary and quick screenshot paths keep the native cursor visible and use a
  small Rsnap-owned cursor-hotspot accent overlay as screenshot-mode feedback.
- Recorded `CapturePointerAccentLayer.swift` as the shared visual owner for ordinary and quick
  screenshot pointer hotspot accent chrome.
- Added `CaptureHostCursorOwner.swift` and `QuickScreenshotController.swift` ownership notes to the
  workspace layout reference.
- Recorded frozen selection-transform performance ownership: drag updates are coalesced through
  the capture-host pointer dispatch queue and active transform redraws are narrowed to dirty
  regions.
- Added user-facing and release-validation documentation for the default quick screenshot shortcut:
  `Alt+Shift+X` / macOS `Option-Shift-X`.
- Recorded quick screenshot Settings ownership, status-menu validation, and event-tap failure
  expectations.
- Recorded the follow-up frozen selection-transform performance constraints: drag delivery keeps
  hover and drag queues separate, drag cancels queued hover work, unchanged transform samples do not
  refresh the overlay, active toolbar translation reuses existing Liquid Glass/content views without
  synchronous redraw, and transform invalidation remains window-local without adding cross-screen
  selection movement.

## 2026-07-06

- Removed the final active `rsnap-overlay` ownership references after macOS ordered live sampling
  moved into the native Swift frame authority.
- Moved frozen-overlay edit state references from `rsnap-overlay` to `rsnap-capture-core` after the
  edit session migrated into the durable core crate.
- Moved frozen-overlay export composition, text rendering, and font fallback references from
  `rsnap-overlay` to `rsnap-capture-core` after the compositor migrated into the durable core
  crate.
- Moved scroll-capture ownership references from `rsnap-overlay` to `rsnap-capture-core` after the
  stitching engine, tests, and Criterion benchmark target migrated into the durable core crate.
- Removed stale documentation routes for the retired Rust overlay UI runtime, recorded replay
  scripts, and trace recorder.
- Added OKF evidence and research indexes required by `decodex docs check`.
- Converted the scroll-capture prior-art research artifact from JSON into a Markdown Research
  Contract so `docs/` remains Markdown-only.
