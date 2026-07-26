---
title: "Rsnap Annotation Pen Contract"
description: "Rsnap Annotation Pen Contract documentation for Rsnap."
type: "Spec"
status: active
authority: normative
owner: acg-box/rsnap
last_verified: 2026-07-06
---
# Rsnap Annotation Pen Contract

Purpose: Define the normative behavior contract for the Frozen-mode pen tool used for screenshot
annotation.

Status: normative

Read this when: You are implementing, reviewing, tuning, or validating pen-tool behavior in
Frozen mode, including preview, commit, undo/redo, and export.

Not this document: Design rationale for why annotation beautification is preferred over pointer
fidelity, or current implementation notes. Use
`docs/decisions/annotation-pen-style.md` for rationale and `docs/reference/` for implementation
context.

Defines:
- the product goal and required behavior for Frozen-mode pen strokes
- preview, commit, export, and undo/redo invariants for pen annotations
- prohibited default behaviors for generic shape inference and automatic loop closure

## Scope

This contract applies to the `FrozenToolbarTool::Pen` path after a capture has entered Frozen
mode.

This contract governs:

- stroke preview while the pointer is down
- stroke finalization when the pointer is released
- committed annotation rendering in the frozen surface
- annotation export behavior for copy and save flows
- undo and redo behavior for committed pen strokes

This contract does not require:

- professional illustration or precision drawing behavior
- automatic conversion into canonical shapes such as circles, rectangles, arrows, or checks
- exact reproduction of the pointer path

## Product objective

The pen tool is an annotation stylizer, not a faithful freehand brush.

Required product objective:

- The pen tool MUST prioritize producing visually polished screenshot annotations over preserving
  every small deviation in the input path.
- The resulting mark SHOULD look recognizably hand-drawn, but better than the raw pointer motion.
- Small high-frequency wobble, dents, shallow reversals, and uneven curvature SHOULD be treated as
  disposable noise when removing them improves the overall mark.
- Large-scale stroke direction, openness, and endpoint intent MUST remain recognizable.

## Required behavior

### Availability and mode boundary

- The pen tool applies only in Frozen mode.
- The pen tool annotates the current frozen capture and does not modify live-mode selection flow.
- Pen annotations are part of the frozen capture state and therefore participate in preview, copy,
  save, and undo/redo.

### Preview and commit consistency

- Drag-time preview MUST already use a beautified stroke path family rather than raw pointer
  segments.
- Pointer-release finalization MAY apply stronger beautification than drag-time preview.
- Pointer-release finalization MUST remain in the same geometric family as preview and MUST NOT
  replace the stroke with a materially different shape character.
- The user MUST NOT see a low-quality jagged preview that becomes the first acceptable result only
  after release.

### Beautification contract

- Pen beautification MUST prefer final visual quality over raw pointer fidelity.
- The beautification pass MUST suppress small local defects when they are inconsistent with the
  surrounding stroke trend.
- The beautification pass MUST improve curvature continuity for arcs and rounded marks rather than
  only smoothing positions point-by-point.
- "Small" defects MUST be interpreted relative to annotation scale, especially stroke width and
  short local span, rather than as a fixed absolute pixel threshold.
- The default tuning SHOULD be aggressive enough that rough circles, arcs, and check-like marks
  look intentionally smooth without requiring the user to draw like a professional illustrator.

### Open-stroke semantics

- Pen strokes MUST remain open by default.
- The implementation MUST NOT auto-close near-loops by default.
- The implementation MUST NOT infer or replace generic shapes by default.
- If future precision or shape-assisted behavior is added, it MUST be an explicit mode or user
  action rather than the default pen behavior.

### Endpoint and large-scale path intent

- Finalized strokes MUST preserve endpoint intent.
- Finalized strokes MAY move endpoints slightly only when required to preserve visible continuity,
  but they MUST NOT materially relocate the apparent start or end of the mark.
- Finalized strokes MUST preserve the large-scale path trend even when local jitter is discarded.

### Rendering and export

- On-screen pen rendering MUST appear continuous and anti-aliased.
- Exported pen rendering MUST appear continuous and anti-aliased.
- The committed export path used by clipboard copy and save MUST include the same committed pen
  annotations visible in Frozen mode.
- Export MUST NOT omit committed pen strokes and MUST NOT render them with a different stroke
  family from the committed on-screen result.

### Undo and redo

- Undo and redo MUST operate on committed pen strokes.
- Undo and redo MUST round-trip the committed beautified stroke, not the raw pointer samples.
- Re-export after undo or redo MUST reflect the current committed stroke set exactly.

## Explicit non-goals for the default pen behavior

- exact pointer-path fidelity
- generic shape recognition
- auto-closing loops
- exposing every tiny dent or wobble in the raw hand motion
- professional drawing precision as the default interaction goal

## Governing rationale

The accepted rationale for this contract lives in:

- `docs/decisions/annotation-pen-style.md`
