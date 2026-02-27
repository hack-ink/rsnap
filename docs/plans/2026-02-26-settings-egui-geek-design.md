# Settings V2 — Egui Geek Layout Design (2026-02-26)

Date: 2026-02-26
SSOT: `settings-native-rewrite-20260226-c`

## Goal

Implement a dense Settings UI V2 layout built entirely from egui widgets, with deterministic row geometry, shared alignment, and no custom chrome.

## Inspection Summary (V2)

- Existing tabs: `General`, `Hotkeys`, `Capture`, `Output`, `Overlay`, `Advanced`, `About`.
- Settings is now fully egui-native and uses a single set of section + row helpers across all panes.
- `egui_extras` is not required; the layout is built using `ScrollArea` + `Grid` + standard egui widgets only.
- The top bar is removed in V2; there is no search field and no header row.
- Content now starts with section headers inside the scrollable settings body.

## Layout Tokens

- `SETTINGS_NAV_WIDTH = 148.0` (left navigation column)
- `SETTINGS_FORM_MAX_WIDTH` (cap central content width)
- `SETTINGS_ROW_HEIGHT` (dense, consistent row height)
- `SETTINGS_LABEL_WIDTH` (fixed label column width)
- `SETTINGS_CONTROL_WIDTH` (fixed control column width)
- `SETTINGS_GRID_SPACING_X/Y` (grid spacing)
- `SETTINGS_SECTION_GAP` (vertical gap around section headers)
- `SETTINGS_COMBO_WIDTH` (fixed combo width + truncation)
- `SETTINGS_SLIDER_WIDTH` + inline `SETTINGS_VALUE_WIDTH` (slider + monospace value width)

## Scrolling + Row System (Grid)

The settings body is wrapped in a vertical `ScrollArea` so long panes (notably `Advanced`) remain usable without window resizing.

Each section is rendered as:

1. Section header (`ui.label`) acting as a visual delimiter.
2. A `Grid` with two columns (label + control), where each row uses:
   - a consistent label column width determined by the widest label in that section.
   - a control column sized by the controls in that section.
   - inline slider/text values shown immediately after the control in a monospace font.

This keeps label/control rhythm constant even across tabs with different control density.

The helper is token-driven so every tab inherits the same geometry without per-pane drift.

## Density and Readability

- `with_settings_density` scopes compact spacing to the Settings window only.
- Controls share fixed heights and widths; rows remain visually scan-friendly.
- Slider values are rendered inline in monospace adjacent to the control for stable visual rhythm.
- All strings are plain egui widget text (`ui.label`, `ui.small_button`, etc.); no custom painter text and no ad-hoc markdown-like labels.

## Cross-Tab Consistency

- `General` and `Overlay` use full label + control rows for active settings.
- `Hotkeys`, `Capture`, `Output`, and `Advanced` use the same row system with inline monospace values where needed.
- `About` uses the same row system for version output.

## Acceptance Criteria (Status)

- [x] Remove top bar + search from Settings window
- [x] Use `ScrollArea` for vertical scrolling
- [x] Use a two-column `Grid` for deterministic rows
- [x] Render slider values inline in monospace
- [x] Keep alignment tokens shared across all tabs
- [x] Render section headers inside the scroll area as the only grouping element
- [x] Keep all settings text in egui widget primitives only

## Outcome

The settings UI now has a consistent, left-aligned, dense layout across all panes, implemented entirely with egui primitives (`ScrollArea`, `Grid`, and standard widgets), with inline monospace value presentation for sliders and other scalar controls.
