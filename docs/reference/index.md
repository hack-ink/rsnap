# Reference Index

Purpose: Route agents to descriptive documents that explain the current repository layout,
implementation shape, and chosen technical approach without defining normative truth or an
execution sequence.

Question this index answers: "how is it currently organized or implemented?"

## Use this index when

- You need the current crate or directory ownership map.
- You need a descriptive explanation of the current implementation model or default strategy.
- You need change-planning context before editing code, but not a normative contract or a runbook.

## Do not use this index when

- You need the authoritative contract, schema, invariant, or required behavior.
- You need a step-by-step runbook, validation sequence, or troubleshooting flow.
- You need durable rationale for why an accepted tradeoff was chosen.

## What belongs in `docs/reference/`

- Repository maps and ownership notes.
- Current implementation-model explanations.
- Strategy and default-choice references that help route or scope a change.
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

- `docs/reference/workspace-layout.md` for workspace layout, crate boundaries, and local-only
  directories
- `docs/reference/live-sampling.md` for the stream-first live RGB and loupe sampling path
- `docs/reference/window-hit-testing.md` for live-mode hovered-window targeting strategy
- `docs/reference/macos-native-capture-window-layer.md` for the single AppKit root-owner capture
  topology, passive pointer shells, and explicit key-focus shell boundary on macOS
