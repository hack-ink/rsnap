---
type: Knowledge Base Router
title: Rsnap OpenWiki Quickstart
description: Canonical entry point for Rsnap repository knowledge, routing readers by authority class to specifications, runbooks, references, decisions, research provenance, and drift evidence.
tags: [rsnap, routing, openwiki]
---
# Rsnap OpenWiki Quickstart

Rsnap is a macOS-first screenshot application whose shipping runtime combines a native Swift/AppKit host with a portable Rust capture core. This wiki is the repository's maintained knowledge and agent-routing surface. It preserves the established documentation corpus without changing the authority of source code, tests, configuration, workflows, specifications, decisions, or historical evidence.

## Authority order

1. User instructions and checked-in project policy govern the work.
2. Source code, tests, configuration, manifests, workflows, and observed behavior own current implementation facts.
3. [Specifications](spec/) own declared contracts and required behavior.
4. [Runbooks](runbook/) own executable procedures and validation sequences.
5. [References](reference/) describe current organization and implementation; they do not create correctness requirements.
6. [Accepted decisions](decisions/) preserve durable rationale.
7. [Research](research/) and [drift evidence](evidence/) preserve provenance and observations without overriding active authority.
8. The reserved [documentation log](log.md) records historical documentation changes and is not a complete source history.

Use [Documentation Policy](policy.md) for placement, ownership, and maintenance rules. When wiki prose conflicts with executable evidence, prefer the higher-authority source and update the canonical owner rather than duplicating a correction elsewhere.

## Route by question

| Question | Canonical lane | Start with |
| --- | --- | --- |
| What must remain true? | [Specifications](spec/) | [Capture Session](spec/capture-session.md), [Platform Host Boundary](spec/platform-host-boundary.md), or the relevant subject spec |
| Which sequence should I execute? | [Runbooks](runbook/) | [Architecture Reset Validation](runbook/architecture-reset-validation.md), [Performance Validation](runbook/performance-validation.md), or [Release](runbook/release.md) |
| How is the repository currently organized? | [References](reference/) | [Workspace Layout](reference/workspace-layout.md) and [Host/Core Reset](reference/host-core-reset.md) |
| Why was a tradeoff accepted? | [Decisions](decisions/) | [Native Host and Rust Core Reset](decisions/native-host-rust-core-reset.md) or another subject decision |
| What evidence supported scroll capture? | [Research](research/) | [Scroll Capture Prior Art](research/scroll-capture-prior-art-2026-05-10.md) |
| What drift was observed during cleanup? | [Evidence](evidence/) | [Legacy Overlay Cleanup Drift](evidence/legacy-overlay-cleanup-drift-2026-07-06.md) |

## Product and architecture orientation

- The product identity and stable naming rules are defined by [App Identity](spec/app-identity.md).
- The native-host/Rust-core split is required by the [Platform Host Boundary](spec/platform-host-boundary.md), represented by the [Host/Core Protocol](spec/host-core-protocol.md), and explained historically by the [architecture reset decision](decisions/native-host-rust-core-reset.md).
- Live, quick, frozen, export, and scroll behavior is governed by the [Capture Session specification](spec/capture-session.md); current sampling implementation is described by [Live Sampling](reference/live-sampling.md).
- Settings and operational telemetry are governed by [Settings](spec/settings.md) and [Telemetry](spec/telemetry.md); collection and diagnosis are performed through the [Telemetry Debugging runbook](runbook/telemetry-debugging.md).
- Release trust and artifact requirements are governed by [Release Distribution](spec/release-distribution.md) and executed through the [Release runbook](runbook/release.md).

## First development checks

Read `README.md`, `Cargo.toml`, and `Makefile.toml` before changing build or workspace behavior. The common repository gates are:

```sh
cargo make checks
cargo make test-host-reset
cargo make test-macos-native-host-stage
./scripts/build_and_run.sh --verify
```

Choose additional smoke and performance checks through [Smoke and Performance Validation Surfaces](reference/smoke-perf-validation-surface.md), not by assuming one aggregate command covers every native path.

## Maintaining this wiki

Update the source that owns a claim, then run `openwiki code --update --print` and review the generated diff against source authority. Direct concept-page edits are reserved for explicit curation or correction. Do not create recurring OpenWiki automation without a separate checked-in decision and correctly routed credentials. Keep secrets, private user data, local environment contents, and machine-specific secret routing out of the wiki.

## Known migration cautions

- Some migrated current-state references contain filenames that predate the July 2026 Swift file-boundary rename; verify exact source paths before acting.
- Existing metadata historically marked decisions, references, research, and evidence as `authority: normative` even where their bodies define a weaker authority. The migration preserves provenance but uses lane meaning and page prose when interpreting authority.
- Some scroll-capture pages still describe v0.2.5 as current while the workspace manifest reports version 0.3.4. Preserve those dated claims as history and use current source for release status.
- The pen-beautification and frozen-toolbar-anchor contracts have reported implementation gaps. Treat their specifications as requirements and validate current code before claiming compliance.

## Backlog

- **Source reconciliation** — `native/macos-host/Sources/RsnapNativeHostKit/` and `packages/rsnap-capture-core/`; deferred because this run preserves the complete documentation corpus and records known conflicts instead of silently rewriting normative or historical claims.
- **Automation disposition** — recurring OpenWiki automation is intentionally absent. Add it only through a separate checked-in decision with correctly routed credentials.
