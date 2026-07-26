# Decisions Index

Purpose: Route agents to durable rationale documents that explain why an accepted technical or
product tradeoff was chosen and what consequences follow from that choice.

Question this index answers: "why was this tradeoff accepted?"

## Use this index when

- You need the reasoning behind an accepted architecture or product decision.
- You need to understand which alternatives were considered and rejected.
- You need long-lived context before revisiting or superseding an earlier choice.

## Do not use this index when

- You need the current authoritative behavior contract.
- You need the current implementation shape or ownership map.
- You need a step-by-step operational sequence.

## What belongs in `docs/decisions/`

- Durable architecture decisions.
- Accepted product or platform tradeoffs with long-lived consequences.
- Records that explain alternatives considered and follow-on constraints.

## Decision document contract

Start each decision record with:

- `Status`
- `Date`
- `Context`
- `Decision`
- `Alternatives considered`
- `Consequences`

Then keep the body decision-oriented:

- Record only accepted or superseded decisions worth revisiting later.
- Link to specs, runbooks, and references instead of duplicating them.
- State the practical impact of the choice so future changes can judge whether it should be kept or
  replaced.

## Current decision records

- `docs/decisions/native-host-rust-core-reset.md` for the accepted architecture reset that makes
  native hosts own OS semantics and Rust own cross-platform product semantics
- `docs/decisions/annotation-pen-style.md` for the pen-tool tradeoff that prioritizes polished
  screenshot annotation over faithful pointer-path reproduction
- `docs/decisions/frozen-toolbar-anchor.md` for the stable-anchor layout choice that prevents
  style-capsule expansion from moving the primary Frozen toolbar
- `docs/decisions/scroll-capture-architecture.md` for the accepted layered scroll-capture target
  architecture based on CleanShot/Xnip/Snagit/Shottr/ScrollSnap prior art and Rsnap live failures
- `docs/decisions/organization-release-secret-visibility.md` for the accepted organization-wide
  GitHub Actions secret scope and the separate application update-trust boundaries
