# Annotation Pen Style

Status: accepted
Date: 2026-04-07
Context:

- `rsnap` uses the pen tool for screenshot annotation, not for professional drawing.
- Users prefer a polished handwritten annotation look over faithful reproduction of every mouse
  wobble.
- The governing behavior contract for this decision is `docs/spec/annotation-pen.md`.
- Earlier iterations exposed the wrong tradeoff:
  - raw-point fidelity preserved tiny dents and uneven curvature that made rough circles and arcs
    look amateurish
  - segmented capsule or dense-dab rendering produced visible scalloping, gaps, or jagged preview
  - auto-closing loops and shape guessing changed user intent too aggressively for general
    annotation strokes
- The product goal is therefore "make the mark look better than the hand drew it" while keeping
  the stroke recognizably human and fast enough for live annotation.

Decision:

- Treat the pen as an annotation stylizer, not a faithful brush.
- Optimize the final stroke for visual quality even when that means discarding small input
  deviations.
- Keep the implementation layered:
  - use a light online stroke model during drag so preview remains smooth and reasonably consistent
    with the final stroke
  - apply a stronger finalize pass on release to suppress micro-wobbles and improve curvature
    continuity
- Default the finalize pass to preserve the large-scale path while aggressively removing
  high-frequency local dents, bumps, and shallow reversals.
- Prefer an open freehand annotation model over generic shape inference or auto-closing behavior.
- Keep the live interaction and the finalized result within the same visual family so the product
  feels like a polished annotation pen instead of a post-hoc shape replacement tool.

Alternatives considered:

- Preserve raw input as the main source of truth.
  - Rejected because it retains hand jitter and produces visibly uneven annotation curves.
- Smooth only after mouse release with no online modeling.
  - Rejected because preview quality remains poor and the final stroke changes too abruptly on
    release.
- Auto-close near-loops or infer shapes such as circles and checks.
  - Rejected because annotation strokes are varied and the correction often overshoots user intent.
- Render the stroke as segmented capsules or dense visible dabs.
  - Rejected because segment boundaries and scalloping are noticeable in `egui` preview.
- Keep increasing generic smoothing passes without explicitly targeting micro-wobbles.
  - Rejected because it softens the whole stroke without reliably eliminating the small dents users
    actually notice.

Consequences:

- Future tuning should bias toward stronger beautification, not higher input fidelity.
- "Small" defects should be interpreted relative to annotation scale, especially brush width and
  short local span, rather than as fixed absolute pixels.
- Changes that preserve tiny dents, shallow notches, or uneven arc curvature are regressions even
  if they are more faithful to the pointer path.
- If a future implementation needs to become more precise, precision should be an explicit mode,
  not the default annotation behavior.
- New work on the pen should prefer:
  - micro-wobble suppression
  - curvature-continuity improvements
  - preview/final consistency
  - subtle online prediction or stabilization
- New work on the pen should avoid by default:
  - generic shape inference
  - auto-closing loops
  - segmented visible stroke primitives
  - raw-path fidelity as a success metric
