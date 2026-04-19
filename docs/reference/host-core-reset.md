# Host/Core Reset Reference

Purpose: Describe the active target architecture for the rsnap reset lane and how new work should
behave while the checked-in codebase is still transitional.

Read this when: You are planning architecture work, deciding whether a change deepens the right
boundary, or trying to understand the intended end state of the reset project.

Inputs: `docs/spec/platform-host-boundary.md`; `docs/spec/capture-session.md`;
`docs/decisions/native-host-rust-core-reset.md`; `docs/reference/workspace-layout.md`

Depends on: `docs/spec/platform-host-boundary.md`;
`docs/decisions/native-host-rust-core-reset.md`

Covers: The target architecture, migration posture, and the relationship between the target design
and the current checked-in repository layout.

## Target architecture

The active reset target is:

- native platform hosts own operating-system semantics
- Rust owns cross-platform product semantics

In practical terms:

- native hosts own capture-window lifecycle, focus/activation, cursor, IME, permissions, and
  native capture capabilities
- Rust owns capture-session state, geometry, annotations, export composition, scroll stitching,
  replay, and deterministic product logic
- host and core communicate through an explicit protocol instead of sharing ownership of OS-facing
  behavior

## What this means for the current tree

The checked-in repository does not yet fully match the target design.

Today:

- `apps/rsnap/` is still the app shell
- `packages/rsnap-overlay/` is still a large transitional runtime container
- final-byte export effects now cross the boundary as explicit host-effect requests: the overlay
  prepares authoritative export/OCR payloads, and `apps/rsnap/` owns clipboard, save, and
  deferred OCR publication

During the reset, treat these as implementation containers rather than the final architecture
story.

## Migration posture

New work in the reset lane should prefer changes that:

- clarify host versus core ownership
- pull OS-facing semantics toward native hosts
- make product semantics more explicit and portable inside Rust
- replace legacy mixed-ownership paths with protocol boundaries

New work should avoid spending the reset lane on changes that only make the old architecture more
comfortable, such as:

- preserving a split-window or split-shell ownership model as the active target
- filing or landing cleanup that only splits files without clarifying the future boundary
- hard-coding product behavior around a specific platform shell implementation

Current reset posture for the scroll-capture slice:

- the native app host owns scroll-capture permission checks, external scroll-input observer
  lifecycle, native scroll-input normalization, and screenshot capability acquisition
- the Rust overlay core owns scroll-capture session state, overlap proof, stitching, and
  fail-closed product semantics
- capability start/stop, frame delivery, and host-side failures must cross the boundary as explicit
  host/core protocol calls instead of implicit worker ownership inside the overlay runtime

## Vertical-slice model

The reset is intended to land as vertical slices rather than as one giant rewrite.

Expected slice order:

1. docs and architecture boundary
2. native host ownership of window/input/focus on macOS
3. live targeting plus display-first Frozen entry on the new boundary
4. export-authority effects on the new boundary
5. text and IME on the new boundary
6. scroll capture on the new boundary
7. validation and performance hardening across the new boundary

## Historical material

Superseded shell-era history is intentionally excluded from the active reset corpus.

Plan new work from the current specs, runbooks, references, and accepted decision record instead.
