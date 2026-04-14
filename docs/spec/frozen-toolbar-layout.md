# Frozen Toolbar Layout Contract

Purpose: Define the normative placement and expansion invariants for the Frozen-mode toolbar and
its annotation style capsule.

Status: normative

Read this when: You are implementing, reviewing, or validating Frozen-mode toolbar positioning,
default placement, dragging, or the Pen/Text style capsule expansion behavior.

Not this document: Current implementation details for overlay windows, or the rationale for why
the stable-anchor interaction model was chosen. Use `docs/reference/` for current implementation
context and `docs/decisions/frozen-toolbar-anchor.md` for rationale.

Defines:
- the stable-anchor contract for the Frozen toolbar primary capsule
- default placement rules for the Frozen toolbar before the style capsule is expanded
- allowed above/below placement behavior for the Pen/Text style capsule
- prohibited geometry feedback from the style capsule back into the primary toolbar anchor

## Terms

- Primary capsule: the always-visible main Frozen toolbar pill containing tool selection and core
  actions.
- Style capsule: the secondary pill that exposes Pen/Text color and size controls.
- Primary anchor: the screen-space top-left origin of the primary capsule.

## Scope

This contract applies to Frozen mode after a capture has been established and the Frozen toolbar is
visible.

This contract governs:

- default Frozen toolbar placement
- user-driven drag repositioning of the Frozen toolbar
- Pen/Text style capsule expansion and collapse
- platform-specific toolbar-window bookkeeping when native auxiliary windows are used

This contract does not define:

- styling, colors, blur, or material treatment
- the tool contents inside the primary or style capsules
- the pen or text editing behavior itself

## Required behavior

### Stable primary anchor

- The primary capsule is the stable toolbar anchor.
- Expanding or collapsing the style capsule MUST NOT move the primary capsule.
- Choosing whether the style capsule appears above or below the primary capsule MUST be a derived
  layout result and MUST NOT rewrite the primary anchor.
- Platform-specific native window sizing, padding, or bookkeeping MUST NOT produce user-visible
  motion of the primary capsule during style-capsule expansion or collapse.

### Default placement

- Default Frozen toolbar placement MUST be computed from the primary capsule geometry, not from
  reserving future space for the style capsule.
- If the primary capsule fits below the frozen capture with the configured screen margin, the
  default placement MUST keep it there even when the style capsule would not later fit below.
- The style capsule MUST NOT cause the default primary placement to be pre-shifted upward, downward,
  or sideways merely to reserve expansion space.

### Style capsule placement

- The style capsule is attached to the primary capsule and may render either above or below it.
- When expanded, the style capsule SHOULD render below the primary capsule if there is sufficient
  space within the current screen constraints.
- If there is not sufficient space below, the style capsule SHOULD render above the primary
  capsule instead.
- The style capsule MAY choose the side with more available room when neither side fully fits, but
  that fallback MUST NOT move the primary anchor.

### Constrained-space behavior

- When screen constraints are tight, the implementation MAY clamp, crop, or otherwise degrade only
  the style-capsule presentation before it moves the primary capsule.
- The primary capsule MAY still be clamped to screen margins or monitor bounds as part of normal
  toolbar placement and dragging rules, but those clamps are independent of style-capsule
  expansion.

### Dragging and hit testing

- User drag repositioning MUST operate on the primary capsule anchor.
- Expanding the style capsule MUST NOT change the meaning of stored toolbar positions from
  "primary capsule anchor" to "whole visible union origin".
- The style capsule MUST NOT enlarge the drag-start region for moving the toolbar; interacting with
  the style capsule is distinct from dragging the primary capsule.

## Practical example

- Valid behavior:
  - the primary capsule is placed below the frozen capture because it fits there
  - later, the user opens Text style controls
  - the style capsule renders above the primary capsule because below is tight
  - the primary capsule stays exactly where it was
- Invalid behavior:
  - the primary capsule is initially shifted upward to reserve possible style-capsule space
  - or the primary capsule jumps when the style capsule opens or closes
