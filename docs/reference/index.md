# Reference Index

Purpose: Route agents to descriptive documents that explain the current repository layout,
active target architecture, and technical context without defining normative truth or an
execution sequence.

Question this index answers: "how is it currently organized or implemented?"

## Use this index when

- You need the current crate or directory ownership map.
- You need the active target architecture for the reset lane.
- You need change-planning context before editing code, but not a normative contract or a runbook.

## Do not use this index when

- You need the authoritative contract, schema, invariant, or required behavior.
- You need a step-by-step runbook, validation sequence, or troubleshooting flow.
- You need durable rationale for why an accepted tradeoff was chosen.

## What belongs in `docs/reference/`

- Repository maps and ownership notes.
- Current implementation-model explanations.
- Active migration-target notes that are descriptive rather than normative.
- Non-normative technical context that should stay separate from runbooks.

## Reference document contract

Start each reference with a compact routing header:

- `Purpose`
- `Read this when`
- `Inputs` or `Sources`
- `Depends on`
- `Covers`

Then keep the body descriptive:

- Explain the current shape, boundaries, defaults, and tradeoffs.
- Link to specs for required behavior and to runbooks for execution steps.
- Avoid turning references into normative contracts or procedure checklists.

## Current references

- `docs/reference/host-core-reset.md` for the active target architecture and migration posture of
  the host/core reset
- `docs/reference/workspace-layout.md` for workspace layout, crate boundaries, and local-only
  directories
- `docs/reference/live-sampling.md` for the stream-first live RGB and loupe sampling path
- `docs/reference/window-hit-testing.md` for live-mode hovered-window targeting strategy

Historical and superseded material is intentionally omitted from the active reference lane and
should not drive change planning.
