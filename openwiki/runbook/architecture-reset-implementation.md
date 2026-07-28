---
title: "Execute Host/Core Reset Slice"
description: "Executable procedure and verification guidance for Execute Host/Core Reset Slice."
type: "Runbook"
status: active
authority: normative
owner: acg-box/rsnap
last_verified: 2026-07-06
---
# Execute Host/Core Reset Slice

Goal: Land one host/core reset slice without reintroducing mixed ownership, historical shell
assumptions, or cleanup-only churn.

Read this when: You are implementing any architecture-reset slice in code or docs and need the
smallest valid execution sequence.

Inputs: The chosen reset slice; [`openwiki/spec/platform-host-boundary.md`](../spec/platform-host-boundary.md);
[`openwiki/spec/capture-session.md`](../spec/capture-session.md); [`openwiki/reference/host-core-reset.md`](../reference/host-core-reset.md); `Makefile.toml`

Depends on: [`openwiki/spec/platform-host-boundary.md`](../spec/platform-host-boundary.md); [`openwiki/spec/capture-session.md`](../spec/capture-session.md);
[`openwiki/reference/host-core-reset.md`](../reference/host-core-reset.md); [`openwiki/runbook/architecture-reset-validation.md`](./architecture-reset-validation.md)

Outputs: One scoped slice change, the matching docs updates, and explicit validation evidence.

## 1. Choose exactly one primary slice

Pick one primary slice before editing:

- docs and routing
- host/window/input ownership
- display-first Frozen entry
- export-authority effects
- text / IME / keyboard ownership
- scroll capture
- validation and performance hardening

Do not combine multiple product slices unless one change is mechanically required to unblock the
chosen slice.

## 2. Restate the ownership boundary before editing

For the touched behavior, write down two short answers:

- What remains native-host-owned because it depends on OS semantics?
- What remains Rust-core-owned because it defines portable product semantics?

If one concern still has two authorities after this exercise, the slice is not ready to implement.

## 3. Change the boundary before polishing the implementation

Apply the change in this order:

1. Define or tighten the host/core protocol messages for the touched behavior.
2. Move OS-facing ownership into the native host side.
3. Keep product semantics, state, and replayable logic in Rust.
4. Remove or bypass legacy mixed-ownership paths instead of making them more comfortable.

Do not spend a reset slice on file splitting, naming cleanup, or UI polish unless that work is
directly required to establish the intended boundary.

## 4. Update docs in the same slice

When the slice changes architecture truth or execution posture:

- update [`openwiki/spec/platform-host-boundary.md`](../spec/platform-host-boundary.md) if ownership rules changed
- update [`openwiki/spec/capture-session.md`](../spec/capture-session.md) if product behavior changed
- update [`openwiki/reference/host-core-reset.md`](../reference/host-core-reset.md) if the active migration posture changed
- update this runbook or [`openwiki/runbook/architecture-reset-validation.md`](./architecture-reset-validation.md) if the execution or
  validation sequence changed

Do not leave the active docs describing a boundary that the code no longer matches.

## 5. Run the smallest valid repo-native checks

Start with the smallest checked-in command set that matches the slice:

- docs and routing only:
  run the docs-only sequence in [`openwiki/runbook/architecture-reset-validation.md`](./architecture-reset-validation.md)
- Rust logic or cross-platform product behavior:
  run `cargo make test`
- scroll-capture behavior or stitching logic:
  run `cargo test -p rsnap-capture-core scroll_capture --lib`
- performance-sensitive rendering or interaction:
  run `scripts/perf/local.sh`
- dedicated macOS desktop readiness without claiming full live acceptance:
  run `scripts/smoke/self-check-macos.sh`

If the slice changes live desktop behavior materially, finish with the relevant manual validation
from [`openwiki/runbook/architecture-reset-validation.md`](./architecture-reset-validation.md).

## 6. Report evidence and skips explicitly

Close the slice by recording:

- the primary slice you changed
- which checked-in commands or manual validations you ran
- which adjacent slices were intentionally not touched
- any remaining follow-up that belongs to a later slice rather than the current one
