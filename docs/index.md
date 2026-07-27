# Documentation Index

Purpose: Route agents to the smallest correct document set for the current task.

Audience: All documentation in this repository is written for AI agents and LLM workflows.
The active split below is by question type, not by human-versus-agent audience.

## Read order

- Read `docs/policy.md` for document contracts and placement rules.
- Read `Makefile.toml` when the task depends on generic repo gates such as `fmt`, `lint`, `test`, or `checks`.
- Read `scripts/smoke/` and `scripts/perf/` when the task depends on smoke or performance validation entrypoints.
- Then choose one primary lane:
  - `docs/spec/index.md` when the question is "what must be true?"
  - `docs/runbook/index.md` when the question is "which sequence should I execute?"
  - `docs/reference/index.md` when the question is "how is it currently organized, or what is the
    current target architecture?"
  - `docs/decisions/index.md` when the question is "why was this tradeoff accepted?"

## Routing matrix

- Need contracts, invariants, schemas, enums, state machines, or required behavior ->
  `docs/spec/`
- Need the normative host/core ownership boundary ->
  `docs/spec/platform-host-boundary.md`
- Need the canonical reset crates and protocol types for native-host integration ->
  `docs/spec/host-core-protocol.md`
- Need the product-level capture contract independent from implementation history ->
  `docs/spec/capture-session.md`
- Need product display name, macOS app bundle identity, or lower-case identifier exceptions ->
  `docs/spec/app-identity.md`
- Need tag source, macOS signing, release assets, Sparkle, or publication invariants ->
  `docs/spec/release-distribution.md`
- Need Settings, status-menu shortcut display, permission placement, Dock behavior, or Settings
  window semantics -> `docs/spec/settings.md`
- Need telemetry fields, event names, metric names, or log correlation identifiers ->
  `docs/spec/telemetry.md`
- Need runbooks, migrations, validation steps, troubleshooting, or operational sequences ->
  `docs/runbook/`
- Need native-host or Rust telemetry collection and summary steps ->
  `docs/runbook/telemetry-debugging.md`
- Need to recover macOS scroll capture after live tearing, sparse stitching, or rollback failures
  -> `docs/runbook/scroll-capture-recovery-plan.md`
- Need the step-by-step execution sequence for a host/core reset slice ->
  `docs/runbook/architecture-reset-implementation.md`
- Need the active architecture-reset target and migration posture ->
  `docs/reference/host-core-reset.md`
- Need the current smoke/perf ownership map before pruning validation assets ->
  `docs/reference/smoke-perf-validation-surface.md`
- Need current repository layout or crate ownership notes ->
  `docs/reference/workspace-layout.md`
- Need durable rationale for the architecture reset ->
  `docs/decisions/native-host-rust-core-reset.md`
- Need the accepted layered scroll-capture architecture and prior-art analysis ->
  `docs/decisions/scroll-capture-architecture.md`
- Need the supporting research contract for scroll-capture prior-art analysis ->
  `docs/research/scroll-capture-prior-art-2026-05-10.md`
- Need generic repo gate names -> `Makefile.toml`
- Need smoke or perf validation entrypoints -> `scripts/smoke/` and `scripts/perf/`
- Need documentation placement or authoring rules -> `docs/policy.md`

## Retrieval rules

- Optimize for agent routing and execution, not narrative flow.
- Keep one authoritative document per topic. Link instead of copying.
- Start each document with a short routing header that says what the document is for,
  when to read it, and what it does not cover.
- Keep links explicit and stable.
- Let structure emerge from real topics. Do not create empty folders, empty indexes, or
  naming schemes that are stricter than the current corpus needs.
- Historical documents must say so clearly and must stay outside the default route for new work.
