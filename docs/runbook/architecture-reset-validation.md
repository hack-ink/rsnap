---
title: "Validate Host/Core Reset Work"
description: "Validate Host/Core Reset Work documentation for Rsnap."
type: "Runbook"
status: active
authority: normative
owner: acgxv/rsnap
last_verified: 2026-07-06
---
# Validate Host/Core Reset Work

Goal: Validate architecture-reset changes without routing through superseded shell-specific
assumptions.

Read this when: You changed docs, boundaries, native-host code, or Rust-core code under the host /
core reset and need a bounded validation sequence.

Inputs: A build of Rsnap, the touched change set, and the governing specs:
`docs/spec/capture-session.md`, `docs/spec/platform-host-boundary.md`, and
`docs/spec/performance.md`.

Depends on: `docs/spec/capture-session.md`; `docs/spec/platform-host-boundary.md`;
`docs/runbook/performance-validation.md`

Outputs: Explicit evidence for the touched slices plus a list of skipped slices that were out of
scope.

## 1. Identify the touched slice

Before running validation, classify the change into one or more of:

- docs and routing only
- host/window/input ownership
- display-first Frozen entry
- export-authority effects
- text / IME / keyboard ownership
- scroll capture
- performance-sensitive rendering or interaction

Do not claim full reset validation when only one slice changed.

## 2. Run deterministic validation first

- For docs-only changes, run the bounded router and link checks in the next subsection.
- For code changes, run the repository's selected gate for the current lane before relying on live
  manual checks.
- If the touched slice has deterministic entrypoints, treat those as authoritative before live
  desktop validation.

### Docs-only validation

When the touched slice is only documentation or routing:

1. Run `git diff --check`.
2. Re-read `docs/index.md`, `docs/policy.md`, and every touched lane index to confirm they route to
   the same active document set.
3. Verify every `docs/*.md` path reference in the documentation resolves:

```bash
python3 - <<'PY'
from pathlib import Path
import re
root = Path(".").resolve()
docs = root / "docs"
pattern = re.compile(r"`(docs/[A-Za-z0-9_./-]+\\.md)`")
missing = []
for path in docs.rglob("*.md"):
    text = path.read_text()
    for rel in pattern.findall(text):
        target = root / rel
        if not target.exists():
            missing.append((path.relative_to(root), rel))
if missing:
    for src, rel in missing:
        print(f"{src}: missing {rel}")
    raise SystemExit(1)
print("All docs/*.md path references resolve.")
PY
```

4. If the change rewires active routing, run
   `rg -n "historical|superseded" docs/index.md docs/policy.md docs/spec docs/reference docs/runbook docs/decisions`
   and confirm any remaining matches are cautionary notes rather than route instructions.

This repository does not currently ship a dedicated docs smoke script under `dev/`, so the checks
above are the executable docs-only gate for now.

## 3. Apply slice-specific manual validation

### Host/window/input ownership

- Verify the target app does not remain visually deactivated after capture selection completes.
- Verify normal capture interaction does not require visible activation artifacts just to finish the
  flow.
- Verify cancelling and immediately restarting capture does not inherit stale focus, cursor, or
  input state.

### Display-first Frozen entry

- Verify the first Frozen display image appears in one visible handoff.
- Verify normal-path entry does not rely on hidden-window fallback behavior.
- Verify toolbar visibility and display-driven interactions start from the committed display image.

### Export-authority effects

- Verify copy/save/OCR and similar final-byte-dependent effects remain gated on export readiness.
- Verify later export-authority completion does not overwrite an already-visible display image.

### Text / IME / keyboard ownership

- Verify plain ASCII text input works in the touched path.
- Verify IME preedit and commit both work where the touched slice claims support.
- Verify `Esc` and other keyboard controls route to the correct scope.

### Scroll capture

- Verify downward-only growth and fail-closed behavior still hold.
- Verify preview, copy, and save render from the same committed stitched canvas.
- Verify `Esc` / `Back` restores the original Frozen capture.

## 4. Run performance validation when the slice can affect responsiveness

If the touched change affects cursor tracking, drag latency, redraw cadence, live sampling,
capture-entry latency, or other interaction-sensitive paths, run the relevant checks from
`docs/runbook/performance-validation.md`.

## 5. Report scope and skips explicitly

- List which slices were actually validated.
- List which slices were not touched and were therefore skipped.
- If a live validation path could not be run, say so explicitly instead of implying full coverage.
