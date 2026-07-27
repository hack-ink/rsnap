---
title: "Native Host / Rust Core Reset"
description: "Native Host / Rust Core Reset documentation for Rsnap."
type: "Decision"
status: active
authority: normative
owner: acg-box/rsnap
last_verified: 2026-07-06
---
# Native Host / Rust Core Reset

Status: accepted
Date: 2026-04-18

Context:

- The existing Rsnap implementation accumulated repeated ownership conflicts across platform
  windows, focus/activation, cursor handling, IME, wheel routing, and fallback-heavy capture
  lifecycle behavior.
- Earlier work attempted to repair those problems with mixed ownership, including passive shells,
  key-focus shells, and other platform-specific patches while Rust remained the primary authority
  for more of the capture-window lifecycle than it should have.
- That path did not produce a durable architecture for macOS, and it would make future platforms
  harder rather than easier.
- At the same time, Rsnap still benefits from keeping cross-platform product semantics, replay,
  geometry, annotation logic, stitching, and export composition in Rust.

Decision:

- Rsnap will be reset around native platform hosts plus a Rust core.
- Native platform hosts own operating-system semantics:
  - capture-window lifecycle
  - focus/activation
  - cursor ownership
  - IME and native text integration
  - permissions
  - native capture capabilities
  - clipboard, save, OCR, and related host-side effects
- Rust owns cross-platform product semantics:
  - capture-session state
  - geometry and targeting rules
  - display-authority versus export-authority semantics
  - annotation state and export composition
  - scroll-capture overlap proof and stitching
  - deterministic product-level verification logic
- Host and core must communicate through an explicit protocol rather than by sharing authority over
  OS-facing behavior.
- Historical shell-specific documents remain as historical context only and are no longer the
  default route for new work.

Alternatives considered:

- Continue repairing the mixed-ownership macOS shell design.
  - Rejected because repeated iterations still left ownership split across too many layers and did
    not produce a durable path for future platforms.
- Move the entire product into platform-native code.
  - Rejected because it would throw away the cross-platform product core, replayability, and
    deterministic logic that still belong in Rust.
- Keep a generic cross-platform window toolkit as the authority for capture-session OS semantics.
  - Rejected because capture tools are too tightly coupled to native window/input/capture systems
    for that to remain the durable authority boundary.

Consequences:

- New architecture work routes through the native-host / Rust-core boundary instead of deepening
  the old mixed-ownership model.
- Product specs must stop encoding shell-specific implementation details as normative truth.
- The documentation router must point to the reset architecture as authoritative.
- The first concrete platform host will be macOS, but the architecture is intended to scale by
  adding more native hosts rather than redesigning the Rust product model per platform.
