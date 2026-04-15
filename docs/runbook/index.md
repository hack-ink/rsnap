# Runbook Index

Purpose: Route agents to procedural documents that tell them which execution sequence to run
safely and repeatably.

Question this index answers: "which sequence should I execute?"

## Use this index when

- You need a runbook, how-to, migration sequence, validation flow, troubleshooting
  path, or maintenance procedure.
- You already know the relevant spec and need the operational steps.
- You need a bounded sequence with prerequisites and verification.

## Do not use this index when

- You need the authoritative contract, schema, or invariant.
- You need descriptive current-state context such as repository layout or implementation strategy;
  read `docs/reference/index.md`.
- You need durable rationale for why an accepted tradeoff exists; read `docs/decisions/index.md`.
- You need broad documentation policy or repo task-entrypoint rules; read
  `docs/policy.md` or `Makefile.toml` instead.

## What belongs in `docs/runbook/`

- Task-oriented runbooks.
- Validation and test procedures.
- Migration, rollout, rollback, and recovery sequences.
- Troubleshooting flows and operator checklists.
- Short operational recipes that depend on a governing spec and end in explicit verification.

## Runbook document contract

Start each runbook with a compact routing header:

- `Goal`
- `Read this when`
- `Inputs` or `Preconditions`
- `Depends on`
- `Outputs` or `Verification`

Then structure the body for execution:

- Write steps in the order an agent should perform them.
- Keep commands, checks, and rollback points explicit.
- Link to specs for normative truth instead of restating contracts.
- Include failure branches only when they change the next action.
- End with verification so an agent can tell whether the runbook succeeded.

## Structure policy

- Group runbooks by workflow or subsystem only when multiple runbooks exist and the grouping
  improves retrieval.
- Do not create empty category folders or placeholder section headings.
- Prefer titles that encode the task or outcome, such as `validate-release.md` or
  `rerun-ingest-job.md`.
- Keep the runbook index as a router, not a dumping ground for long explanations.

## Current runbooks

- `docs/runbook/performance-validation.md` for repo-native performance and smoke command routing
- `docs/runbook/scroll-capture-benchmarks.md` for deterministic scroll-capture benchmark usage
- `docs/runbook/macos-native-capture-window-validation.md` for manual validation of passive
  pointer shells and explicit key-focus shell behavior on macOS
