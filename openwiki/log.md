# Documentation Log

## 2026-07-28

- Initialized the repository OpenWiki with Open Knowledge Format v0.1.
- Migrated all 30 body-bearing documents and all six lane indexes from `docs/` into
  `openwiki/`, preserving specifications, runbooks, references, accepted decisions, research
  provenance, drift evidence, metadata, and the reserved log.
- Replaced the former documentation router with `openwiki/quickstart.md` and the generated
  `openwiki/index.md` bundle inventory.
- Added repository-specific generation policy in `openwiki/INSTRUCTIONS.md` and OpenWiki routing
  blocks in root `AGENTS.md` and `CLAUDE.md`.
- Retired the tracked `docs/` tree and updated README and CI path routing to `openwiki/`.
- Kept OpenWiki maintenance manual and generator-first. No recurring OpenWiki workflow is
  authorized.
- Preserved all 233 source headings across the 30 body-bearing pages; generated relationship
  sections and four source-grounded Mermaid diagrams increased the migrated body corpus from
  4,470 to 4,578 lines before final curation.

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
  Contract so `openwiki/` remains Markdown-only.
