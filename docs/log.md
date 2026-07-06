# Documentation Log

## 2026-07-06

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
