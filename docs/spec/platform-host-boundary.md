---
title: "Platform Host Boundary"
description: "Platform Host Boundary documentation for Rsnap."
type: "Spec"
status: active
authority: normative
owner: acgxv/rsnap
last_verified: 2026-07-07
---
# Platform Host Boundary

Purpose: Define the normative ownership boundary between native platform hosts and the Rust core
for the Rsnap architecture reset.

Status: normative

Read this when: You are deciding where new capture, window, input, OCR, export, or platform code
belongs, or when you are reviewing whether a change deepens the wrong side of the architecture.

Not this document: The product-level capture contract, current repository layout, or historical
implementation notes. Use `docs/spec/capture-session.md` for product behavior,
`docs/reference/workspace-layout.md` for the checked-in tree, and the accepted reset decision for
architecture rationale.

Defines:
- which responsibilities belong to native platform hosts versus the Rust core
- the allowed protocol shape between host and core
- prohibited mixed-ownership patterns
- the portability rule for future platforms

## Architecture rule

Rsnap must be organized around:

- native platform hosts that own operating-system semantics
- a Rust core that owns cross-platform product semantics

No supported platform may depend on a mixed-ownership design where both the native host and the
Rust core simultaneously act as the authority for the same OS-facing concern.

## Native platform host ownership

The native platform host owns:

- top-level capture-window lifecycle
- z-order, visibility, hit-testing, and mouse passthrough
- activation, focus, key/main semantics, and first-responder behavior
- cursor ownership and platform cursor updates
- IME composition and native text-system integration
- global hotkey registration
- permissions and permission recovery flows
- native screenshot / live-stream / window-list / OCR capability acquisition
- OS resource discovery that has no portable API, such as the current desktop wallpaper path
- clipboard, save-panel, notification, sound, and similar host-side effects
- presenting rendered pixels returned by the core inside native windows and controls

Host-owned cursor and input acquisition may differ by capture entry path as long as the product
contract is preserved. For example, the ordinary macOS capture path may let a focused AppKit
overlay view own cursor rects and pointer delivery, while the quick screenshot path must keep its
overlay non-key, non-activating, and mouse-transparent so transient target UI remains visible. In
that quick path, the native host owns event interception through a session event tap and renders
pointer feedback through overlay layers rather than mutating the system cursor.

## Rust core ownership

The Rust core owns:

- capture-session state machines
- product modes such as Live, Frozen, and scroll capture
- geometry, targeting semantics, and selection rules
- annotation state, undo/redo, and export composition rules
- display-authority versus export-authority semantics
- scroll-capture overlap proof, stitching, and fail-closed rules
- final-byte image algorithms: crop mapping, lossless PNG export encoding, capture-frame planning
  and compositing, wallpaper thumbnail decoding/caching, mosaic patch generation, minimap planning,
  selection transforms, auto-centering analysis, and live-sample pixel extraction
- deterministic tests, fixtures, and product-level validation logic
- cross-platform product data models and behavior contracts

## Allowed host/core protocol

The host/core boundary must be expressed through explicit protocol messages.

Host to core messages include:

- user-intent events such as pointer, keyboard, and IME events
- capability results such as live-frame delivery, freeze snapshot delivery, and window snapshot
  updates
- source pixels and narrow OS resource references, such as a wallpaper file path, when Rust owns the
  portable planning, decode, cache, resize, composition, or export algorithm
- lifecycle and environment signals such as permission changes or host teardown

Core to host messages include:

- host commands such as show/hide/update capture UI
- capability requests such as start/stop live capture or request freeze snapshots
- side-effect requests such as copy, save, OCR, or other host-owned effects
- rendered pixels, PNG bytes, geometry plans, hit-test results, and other deterministic outputs from
  Rust-owned product algorithms

The boundary must avoid leaking platform-native event types or platform-native window handles into
the Rust product model except through narrow adapter types owned by the host layer.

## Prohibited patterns

The following are out of bounds for new architecture work:

- treating a generic cross-platform window toolkit as the authority for capture-session OS
  semantics
- encoding specific host implementation details such as passive shells, key-focus shells, or
  visible `winit` capture-window ownership into the product spec
- allowing Rust session code to directly own top-level platform window focus or activation policy
- coupling product correctness to a legacy fallback-heavy lifecycle when that concern belongs in a
  host capability adapter
- reimplementing Rust-owned image planning, export, geometry, or analysis algorithms in Swift after
  an FFI entrypoint exists for that responsibility

## Portability rule

Adding a new platform should mean adding a new native host implementation for the same product
contract, not redesigning the Rust product model around another platform's window toolkit.

Future platforms may differ in host implementation strategy, but they must still honor:

- the product-level contract in `docs/spec/capture-session.md`
- the ownership rules in this document

## Documentation rule

- Product behavior belongs in `docs/spec/capture-session.md`.
- Active migration and architecture posture belong in `docs/reference/host-core-reset.md`.
- Accepted rationale for this split belongs in `docs/decisions/native-host-rust-core-reset.md`.
- Superseded host designs must not remain in the active documentation set as planning inputs.
