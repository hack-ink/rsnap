---
title: "Documentation Policy"
description: "Repository documentation authority, placement, and maintenance policy for Documentation Policy."
type: "Policy"
status: active
authority: normative
owner: acg-box/rsnap
last_verified: 2026-07-28
---
# Documentation Policy

Purpose: Define how agent-facing documentation is organized, updated, and kept consistent
across this repository.

Audience: All documentation under `openwiki/` is written for AI agents and LLM workflows.
The active split between `spec`, `runbook`, `reference`, `decisions`, and supporting `research`
artifacts is by task shape, not by reader type.

## Principles

- Optimize for retrieval, routing, and execution.
- Keep one authoritative document per topic.
- Separate normative truth, execution steps, descriptive current-state reference, and durable
  design rationale.
- Prefer explicit section labels and stable links over prose-heavy narrative.
- Remove superseded historical material from the active OpenWiki tree.
- Let structure emerge from real topics. Avoid premature folder taxonomies.

## Document classes

| Class | Location | Answers | Source of truth for | Update trigger |
| --- | --- | --- | --- | --- |
| Spec | `openwiki/spec/` | What must be true? | Contracts, schemas, invariants, required behavior | Any behavior or schema change |
| Runbook | `openwiki/runbook/` | Which sequence should I execute? | Runbooks, migrations, validation, troubleshooting | Any procedure or operational change |
| Reference | `openwiki/reference/` | How is it currently organized or implemented? | Ownership maps, implementation-model notes, non-normative technical context | Any layout, ownership, or current-implementation explanation change |
| Decisions | `openwiki/decisions/` | Why was this tradeoff accepted? | Durable rationale for accepted technical or product choices | Any accepted decision with long-lived consequences |
| Research | `openwiki/research/` | What evidence supported a bounded investigation? | Supporting research contracts, not primary behavior authority | Any evidence-backed research run that must remain replayable |
| Evidence | `openwiki/evidence/` | What was observed or verified? | Dated, public-safe drift and validation evidence within its stated scope | Any watched claim or evidence anchor change |

## Placement rules

- If a document defines correctness, it belongs in `openwiki/spec/`.
- If a document defines an execution sequence, it belongs in `openwiki/runbook/`.
- If a document explains current layout, ownership, defaults, or implementation shape without
  defining correctness, it belongs in `openwiki/reference/`.
- If a document records why a durable tradeoff was accepted, which alternatives were considered,
  and what consequences follow from that choice, it belongs in `openwiki/decisions/`.
- If a document records a bounded research method, evidence inventory, challenge, or decision
  finalization, it belongs in `openwiki/research/` and must link to the authoritative spec, runbook,
  reference, or decision it supports.
- If a document records a dated drift audit or validation observation, it belongs in
  `openwiki/evidence/` and must state the watched claim, evidence anchors, verification date,
  scope, and gaps.
- If a document becomes historical-only and no longer helps execute current work, remove it from
  `openwiki/` instead of keeping it in the active routing surface.
- Do not duplicate the same authoritative content across documents. Link to the source
  of truth instead.
- A runbook may summarize why a step exists, but normative statements still live in the
  governing spec.

## Document contracts

Every document should start with a short routing header.

Spec header:

- `Purpose`
- `Status: normative`
- `Read this when`
- `Not this document`
- `Defines`

Runbook header:

- `Goal`
- `Read this when`
- `Inputs` or `Preconditions`
- `Depends on`
- `Outputs` or `Verification`

Reference header:

- `Purpose`
- `Read this when`
- `Inputs` or `Sources`
- `Depends on`
- `Covers`

Decision header:

- `Status`
- `Date`
- `Context`
- `Decision`
- `Alternatives considered`
- `Consequences`

Evidence header:

- `Watched Claims`
- `Evidence Anchors`
- `Reverse Checks`
- `Verdict`
- `Required Updates`

## Structure rules

- Prefer shallow paths by default.
- Add subfolders only when they mirror stable system boundaries or improve retrieval.
- Prefer descriptive kebab-case file names for tracked Markdown documents.
- Prefer stable subject names over phase labels, version labels, or document-status labels.
- Let the parent directory carry the document class; filenames should carry the topic.
- Keep existing file paths stable unless a rename materially improves retrieval or removes
  ambiguity.
- Do not require fixed filename prefixes unless a real ambiguity appears.
- Do not create empty folders, empty indexes, or placeholder documents to satisfy a
  taxonomy.

## Canonical entry points

- Canonical documentation router: [`openwiki/quickstart.md`](./quickstart.md)
- Normative router: `openwiki/spec/`
- Procedural router: `openwiki/runbook/`
- Descriptive router: `openwiki/reference/`
- Decision router: `openwiki/decisions/`
- Generic repo gates: `Makefile.toml`
- Smoke/perf validation entrypoints: `scripts/smoke/` and `scripts/perf/`

## LLM reading guidance

When answering a repository question:

1. Read `openwiki/quickstart.md` for routing. Use `openwiki/index.md` as the generated OKF
   inventory.
2. Route by question type:
   - "What must be true?" -> `openwiki/spec/`
   - "Which sequence should I execute?" -> `openwiki/runbook/`
   - "How is it currently organized or implemented?" -> `openwiki/reference/`
   - "Why was this tradeoff accepted?" -> `openwiki/decisions/`
3. Read `Makefile.toml` when the task depends on generic repo gates such as `fmt`, `lint`, `test`, or `checks`.
4. Read `scripts/smoke/`, `scripts/perf/`, and the relevant runbook when the task depends on smoke or performance validation entrypoints.

## Update workflow

- Classify the knowledge impact as `none`, `update_required`, or `research_required`.
- Behavior or schema change: update the relevant spec.
- Procedure change: update the relevant runbook.
- Layout, ownership, or current-implementation explanation change: update the relevant reference.
- Durable accepted tradeoff change: add or update the relevant decision record.
- Historical or supporting context that should not remain authoritative: remove it from `openwiki/`
  instead of leaving it available as a default planning input.
- If a change touches both truth and procedure, update both documents and keep their
  boundary explicit.
- If a change touches truth plus descriptive context, update both the spec and the reference.
- When a runbook starts carrying normative content, move that content into spec and link
  to it.
- When a runbook starts carrying mostly descriptive context instead of executable steps, move that
  content into reference and keep only the runbook in `openwiki/runbook/`.
- For `update_required`, update the owning source first, run
  `openwiki code --update --print`, and review the generated `AGENTS.md`, `CLAUDE.md`, and
  `openwiki/` diff against source authority.
- Direct page edits are reserved for explicit curation or correction. Recurring OpenWiki
  automation requires a separate checked-in decision and correctly routed credentials.
- OpenWiki v0.2.3 can restore generic scheduled-workflow wording in its root routing blocks.
  Review and correct those blocks after each run when no recurring workflow is authorized.

## OpenWiki migration note

The canonical router is the [Rsnap OpenWiki Quickstart](./quickstart.md). The retained [documentation log](./log.md) preserves prior change provenance, while [openwiki/INSTRUCTIONS.md](./INSTRUCTIONS.md) remains user-authored control metadata outside the generated concept set.
