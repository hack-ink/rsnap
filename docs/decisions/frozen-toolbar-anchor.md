---
title: "Frozen Toolbar Stable Anchor"
description: "Frozen Toolbar Stable Anchor documentation for Rsnap."
type: "Decision"
status: active
authority: normative
owner: acg-box/rsnap
last_verified: 2026-07-06
---
# Frozen Toolbar Stable Anchor

Status: accepted
Date: 2026-04-14
Context:

- The Frozen toolbar has a primary capsule plus a secondary style capsule for Pen/Text controls.
- Earlier implementations mixed two incompatible meanings for toolbar position:
  - "primary capsule anchor"
  - "whole visible toolbar union origin"
- That ambiguity produced repeated regressions:
  - default placement reserved space for the secondary capsule too early
  - opening the secondary capsule visibly moved the primary toolbar
  - platform-specific native-window bookkeeping leaked through as toolbar motion
- The governing behavior contract for this decision is `docs/spec/frozen-toolbar-layout.md`.

Decision:

- Treat the primary capsule as the single stable anchor for Frozen toolbar positioning.
- Derive the style capsule layout from that anchor instead of allowing style-capsule geometry to
  rewrite toolbar position.
- Keep default placement based on primary-capsule geometry, then choose style-capsule placement
  above or below afterward.
- Accept degraded style-capsule presentation under tight screen constraints before reflowing the
  primary capsule to make room.

Alternatives considered:

- Keep positioning based on the union bounds of the primary and style capsules.
  - Rejected because it makes expansion geometry feed back into the primary toolbar position.
- Reserve worst-case style-capsule space during default placement.
  - Rejected because it moves the primary toolbar before the user has even asked to open the
    secondary controls.
- Use platform-specific native-window offsets as the main geometry authority.
  - Rejected because it obscures the product contract and creates motion bugs that are artifacts of
    window bookkeeping rather than intended UI behavior.

Consequences:

- Stored Frozen toolbar positions must keep the meaning "primary capsule anchor".
- Future work on Pen/Text controls should attach as derived layout around the stable anchor rather
  than introducing a new source of truth for toolbar position.
- Native-window sizing, padding, or hit-testing changes are regressions if they cause visible
  primary-toolbar motion during style-capsule expansion or collapse.
- Any future redesign that intentionally allows the primary capsule to move when style controls are
  shown would need to explicitly supersede this decision and update the governing spec.
