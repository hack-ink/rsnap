# Settings Window And App Capture Cleanup Plan

Goal: Continue the post-`XY-80` cleanup by extracting the bulk of `SettingsWindow`
section-rendering, chrome/theme UI, hotkey logic, render/surface plumbing, and adjacent app-level
capture/session and macOS scroll-input helpers out of their hotspot roots into dedicated support
modules, while preserving the existing `settings.toml` contract and visible settings-window
behavior.

Scope:
- Reduce hotspot concentration inside `apps/rsnap/src/settings_window.rs`.
- Extract section-rendering and shared widget/helper methods into `apps/rsnap/src/settings_window/sections.rs`.
- Extract settings-window chrome/theme/autosize UI helpers into `apps/rsnap/src/settings_window/chrome.rs`.
- Consolidate hotkey parsing, formatting, row rendering, and recording-state helpers under `apps/rsnap/src/settings_window/hotkey.rs`.
- Extract the draw path and GPU/surface setup helpers into `apps/rsnap/src/settings_window/render.rs`.
- Extract overlay-session wiring and capture-hotkey action plumbing out of `apps/rsnap/src/app.rs` into dedicated `app` support modules once `settings_window.rs` is no longer the primary hotspot.
- Extract `apps/rsnap/src/app/scroll_input_macos.rs` queue/replay state, CGEvent decoding, and event-tap lifecycle into flat support modules once the app root is no longer the primary hotspot.
- Keep the existing isolated worktree lane at `x/settings-window-ui-split`.
- Verify the refactor with repo-native Rust commands and targeted settings-window checks.

Assumptions:
- This cleanup continues after `XY-80` and depends on the existing settings and app support boundaries.
- Repo-native Rust commands plus targeted macOS smoke checks are the intended verification surface.
- This document is retained as historical execution context and may drift from the current repo state.

Steps:
- Extract settings-window support modules from the hotspot root.
- Extract app capture-session and macOS scroll-input support modules once the settings hotspot is reduced.
- Verify each extraction batch with targeted tests and smoke checks.

Status: Closed on 2026-03-12. Retained for historical context; may drift from current code.

## Non-goals

- Changing settings persistence behavior or the `settings.toml` schema.
- Reworking capture-hotkey recording ownership.
- Broad `overlay`, `backend`, or scroll-capture behavior cleanup or bug fixing.
- UI redesign beyond module extraction.

## Constraints

- Preserve user-visible settings-window behavior unless a change is intentional and reviewed.
- Keep event-loop ownership and action queue ownership in `apps/rsnap/src/settings_window.rs`.
- Keep edits scoped to the existing worktree lane plus the `settings_window` file family and directly related `apps/rsnap/src/app*` support modules.
- Run the repo's commit/push gate before any eventual `git commit` or `git push`.

## Open Questions

- None.

## Execution State

- Last Updated: 2026-03-12
- Next Checkpoint: None (lane closed)
- Blockers:
  - Follow-up bug debt now lives in `XY-98`, which tracks the existing `cargo make smoke-scroll-capture-macos` failure (`scroll capture did not append any rows`, repeated `reason="stale_input"`) outside this closed cleanup lane.

## Decision Notes

- `docs/plans/2026-03-11_xy-76-architecture-refactor.md` is complete and does not currently cover this follow-up cleanup slice, so this lane needs its own execution artifact.
- The current runtime repeatedly interrupted builder helper lanes before they touched the worktree, so this already-isolated lane will proceed directly in `x/settings-window-ui-split` instead of spending more time on blocked helper retries.
- `apps/rsnap/src/settings_window/hotkey.rs` and `apps/rsnap/src/settings_window/platform.rs` already prove the support-module pattern for this file family.
- Automated verification on 2026-03-12: `cargo test -p rsnap --lib` passed after extracting the section-rendering helpers into `apps/rsnap/src/settings_window/sections.rs`.
- Automated verification on 2026-03-12: `cargo test -p rsnap --lib` passed again after extracting settings-window chrome/theme/autosize helpers into `apps/rsnap/src/settings_window/chrome.rs` and moving hotkey row rendering alongside hotkey parsing in `apps/rsnap/src/settings_window/hotkey.rs`.
- The remaining concentrated hotspot in `apps/rsnap/src/settings_window.rs` is the draw/surface/GPU helper cluster, so this lane will take one more narrow structural extraction before attempting final closeout.
- After the render split, the remaining non-lifecycle hotspot in `apps/rsnap/src/settings_window.rs` is the capture-hotkey recording helper cluster, which can still move into `apps/rsnap/src/settings_window/hotkey.rs` without touching settings persistence or scroll-capture behavior.
- Automated verification on 2026-03-12: `cargo test -p rsnap --lib` passed again after extracting `apps/rsnap/src/settings_window/render.rs` and then consolidating the remaining capture-hotkey recording helpers into `apps/rsnap/src/settings_window/hotkey.rs`.
- Additional verification on 2026-03-12: `cargo make smoke-self-check-macos` passed. `cargo make smoke-macos` reached a passing live-loupe run, then failed in the scroll-capture sub-smoke; a direct `cargo make smoke-scroll-capture-macos` rerun reproduced `scroll capture did not append any rows` with repeated `scroll_capture.observation_blocked reason="stale_input"` log lines.
- User direction on 2026-03-12 explicitly widened this lane to allow touching previously-problematic paths for cleanup, so the next structural batch can include app-level capture/session plumbing as long as it does not change scroll-capture behavior.
- Automated verification on 2026-03-12: `cargo test -p rsnap --lib` passed again after extracting overlay-session wiring into `apps/rsnap/src/app/capture.rs` and capture-hotkey action plumbing into `apps/rsnap/src/app/hotkeys.rs`, shrinking `apps/rsnap/src/app.rs` to a thinner root module.
- Additional verification on 2026-03-12: `cargo make smoke-self-check-macos` still passed after the `app.rs` extraction, so the app-level cleanup did not break the lightweight macOS startup/self-check path.
- The remaining app-shell hotspot after the `app.rs` split was `apps/rsnap/src/app/scroll_input_macos.rs`, and its responsibility boundaries already matched three stable seams: queue/replay state, CGEvent decode, and event-tap lifecycle.
- Automated verification on 2026-03-12: `cargo test -p rsnap --lib` and `cargo make smoke-self-check-macos` both passed again after extracting `apps/rsnap/src/app/scroll_input_macos/state.rs`, `apps/rsnap/src/app/scroll_input_macos/decode.rs`, and `apps/rsnap/src/app/scroll_input_macos/tap.rs`.
- Manual verification on 2026-03-12: using an isolated temporary `HOME`, opened Settings, changed representative General (`log_filter=warn`), Overlay (`hud_glass_enabled=false`), Capture (`window_capture_alpha_mode=matte_dark`), and Output (`output_naming=sequence`) values through the GUI, confirmed they were written to `settings.toml`, relaunched rsnap, and confirmed the same values were loaded back into the Settings UI.
- Tracker follow-up on 2026-03-12: moved the `rsnap Entropy Reduction` project to `In Progress` and split the lingering scroll-capture `stale_input` failure into `XY-98` so the closed cleanup lane no longer carries an anonymous blocker.
- Pre-review verification on 2026-03-12: target-file `rustfmt +nightly --check`, `cargo test -p rsnap --lib`, `cargo clippy -p rsnap --all-targets --all-features -- -D warnings`, and `cargo make smoke-self-check-macos` all passed on the current diff. Workspace-wide `cargo make fmt-rust-check` still reports unrelated overlay baseline formatting in `packages/rsnap-overlay/src/overlay/output.rs` and `packages/rsnap-overlay/src/overlay/window_runtime.rs`, so it is not the decisive gate for this lane.

## Implementation Outline

Treat this as a narrow structural extraction, not a behavior change. The root `settings_window.rs` file should keep window lifecycle, event handling, and action queue ownership. The support modules should own the large section-rendering helpers, slider/value widgets, hotkey behavior, titlebar/theme/autosize UI helpers, and draw/surface/GPU helpers that otherwise dominate the file.

The first checkpoint should stop at a compile- and test-verified module split. Manual GUI verification remains relevant because the settings window is user-facing, but the code batch should not widen into unrelated theme, overlay, or platform work just to "finish" cleanup.

## Task 1: Extract Section Rendering Helpers

**Owner**

Executor in the `x/settings-window-ui-split` worktree.

**Status**

done

**Outcome**

`apps/rsnap/src/settings_window.rs` becomes smaller and keeps only lifecycle-oriented ownership, while section rendering and shared slider helpers live in `apps/rsnap/src/settings_window/sections.rs`.

**Files**

- Modify: `apps/rsnap/src/settings_window.rs`
- Create: `apps/rsnap/src/settings_window/sections.rs`
- Review: `apps/rsnap/src/settings_window/hotkey.rs`
- Review: `apps/rsnap/src/settings_window/platform.rs`

**Changes**

1. Add `mod sections;` and move the section-rendering helper batch into the new support module.
2. Keep window lifecycle, event handling, capture-hotkey recording ownership, `GpuContext`, and `pick_surface_*` in the root file for the first split; later checkpoints may still extract additional UI helpers.
3. Keep helper imports and constants explicit so the split stays readable and flat-module compliant.

**Verification**

- `cargo test -p rsnap --lib`

**Dependencies**

- None.

## Task 2: Extract Chrome And Hotkey UI Helpers

**Owner**

Executor in the `x/settings-window-ui-split` worktree.

**Status**

done

**Outcome**

`apps/rsnap/src/settings_window.rs` keeps state ownership, event handling, draw orchestration, and GPU/surface lifecycle, while `apps/rsnap/src/settings_window/chrome.rs` owns settings-window chrome/theme/autosize helpers and `apps/rsnap/src/settings_window/hotkey.rs` owns the hotkey row rendering alongside existing parsing/formatting logic.

**Files**

- Modify: `apps/rsnap/src/settings_window.rs`
- Modify: `apps/rsnap/src/settings_window/hotkey.rs`
- Create: `apps/rsnap/src/settings_window/chrome.rs`

**Changes**

1. Add `mod chrome;` and move `ui`, `maybe_autosize_window`, titlebar/theme controls, and theme sync helpers into `apps/rsnap/src/settings_window/chrome.rs`.
2. Move `render_hotkeys_section` and `SettingsWindow::format_capture_hotkey` into `apps/rsnap/src/settings_window/hotkey.rs`, keeping the guidance string there as the shared source for both parsing and UI.
3. Keep root-file ownership over the event loop, action queue, capture-hotkey state transitions, draw path, and surface reconfiguration.

**Verification**

- `cargo test -p rsnap --lib`

**Dependencies**

- Task 1.

## Task 3: Extract Render And Surface Helpers

**Owner**

Executor in the `x/settings-window-ui-split` worktree.

**Status**

done

**Outcome**

`apps/rsnap/src/settings_window.rs` keeps state and event ownership, while `apps/rsnap/src/settings_window/render.rs` owns the egui/wgpu draw path plus GPU/surface setup and reconfiguration helpers.

**Files**

- Modify: `apps/rsnap/src/settings_window.rs`
- Create: `apps/rsnap/src/settings_window/render.rs`

**Changes**

1. Add `mod render;` and move `draw`, `acquire_frame`, surface recreation/reconfiguration, resize, `GpuContext`, and `pick_surface_*` into the new support module.
2. Keep `open` in the root file, but source `GpuContext::new_with_surface` from the new render module.
3. Preserve the public `SettingsWindow::draw` API and avoid any behavior change in repaint scheduling or surface recovery.

**Verification**

- `cargo test -p rsnap --lib`

**Dependencies**

- Task 1.
- Task 2.

## Task 4: Consolidate Remaining Hotkey Recording Helpers

**Owner**

Executor in the `x/settings-window-ui-split` worktree.

**Status**

done

**Outcome**

`apps/rsnap/src/settings_window.rs` keeps window/event ownership, while `apps/rsnap/src/settings_window/hotkey.rs` owns hotkey notice presentation plus recording-state transition helpers alongside existing parsing and row-rendering logic.

**Files**

- Modify: `apps/rsnap/src/settings_window.rs`
- Modify: `apps/rsnap/src/settings_window/hotkey.rs`

**Changes**

1. Move `CaptureHotkeyNotice` presentation and the capture-hotkey recording helper methods into `apps/rsnap/src/settings_window/hotkey.rs`.
2. Keep `handle_window_event` in the root file, but let it call the hotkey-owned helper methods.
3. Preserve the external `SettingsWindow` APIs used by `app/runtime.rs` for recording-state and notice updates.

**Verification**

- `cargo test -p rsnap --lib`

**Dependencies**

- Task 1.
- Task 2.
- Task 3.

## Task 5: Verify Settings Behavior And Close Out The Lane

**Owner**

Executor after Task 4 lands cleanly.

**Status**

done

**Outcome**

The lane has recorded both automated and targeted manual evidence, while the existing scroll-capture smoke failure remains explicit as out-of-scope follow-up debt rather than a false green closeout.

**Files**

- Modify: `docs/plans/2026-03-12_settings-window-section-extraction.md`
- Review: `apps/rsnap/src/settings_window.rs`
- Review: `apps/rsnap/src/settings_window/chrome.rs`
- Review: `apps/rsnap/src/settings_window/hotkey.rs`
- Review: `apps/rsnap/src/settings_window/render.rs`
- Review: `apps/rsnap/src/settings_window/sections.rs`

**Changes**

1. Record the actual verification results and any remaining manual validation debt.
2. If GUI-only checks cannot be completed in-session, leave a terse follow-up note instead of widening scope.
3. Keep unrelated smoke failures explicit instead of folding them into a false closeout.

**Verification**

- `cargo test -p rsnap --lib`
- `cargo make smoke-self-check-macos`
- `cargo make smoke-macos`
- `cargo make smoke-scroll-capture-macos`
- Manual supplement: open Settings, change representative values in General, Overlay, Capture, and Output, then relaunch and confirm persistence still matches prior behavior.

**Dependencies**

- Task 1.
- Task 2.
- Task 3.
- Task 4.

## Task 6: Extract App Capture Session Helpers

**Owner**

Executor in the `x/settings-window-ui-split` worktree.

**Status**

done

**Outcome**

`apps/rsnap/src/app.rs` keeps `App` state and the lightweight root entrypoints, while overlay-session wiring and capture-hotkey action plumbing live in dedicated `app` support modules that isolate the scroll-input entry path without changing behavior.

**Files**

- Modify: `apps/rsnap/src/app.rs`
- Create: `apps/rsnap/src/app/capture.rs`
- Create: `apps/rsnap/src/app/hotkeys.rs`

**Changes**

1. Add `mod capture;` and move overlay config, overlay-session start/end handling, and macOS scroll-input observer wiring into `apps/rsnap/src/app/capture.rs`.
2. Add `mod hotkeys;` and move capture-hotkey labels plus settings-window capture-hotkey action plumbing into `apps/rsnap/src/app/hotkeys.rs`.
3. Keep `App` state ownership, root event types, and settings-window open/focus handling in `apps/rsnap/src/app.rs`.

**Verification**

- `cargo test -p rsnap --lib`
- `cargo make smoke-self-check-macos`

**Dependencies**

- Task 4.

## Task 7: Extract Scroll Input Support Modules

**Owner**

Executor in the `x/settings-window-ui-split` worktree.

**Status**

done

**Outcome**

`apps/rsnap/src/app/scroll_input_macos.rs` becomes a thin platform root that owns the macOS framework bindings and re-exports, while queue/replay state, CGEvent decode, and event-tap lifecycle live in dedicated support modules.

**Files**

- Modify: `apps/rsnap/src/app/scroll_input_macos.rs`
- Create: `apps/rsnap/src/app/scroll_input_macos/state.rs`
- Create: `apps/rsnap/src/app/scroll_input_macos/decode.rs`
- Create: `apps/rsnap/src/app/scroll_input_macos/tap.rs`

**Changes**

1. Move bounded queue/replay state and its tests into `apps/rsnap/src/app/scroll_input_macos/state.rs`.
2. Move CGEvent field decoding and phase classification helpers into `apps/rsnap/src/app/scroll_input_macos/decode.rs`.
3. Move event-tap install, callback, and lifecycle wiring into `apps/rsnap/src/app/scroll_input_macos/tap.rs`, while keeping the framework bindings in the root file.

**Verification**

- `cargo test -p rsnap --lib`
- `cargo make smoke-self-check-macos`

**Dependencies**

- Task 6.

## Rollout Notes

- Stay in the existing isolated worktree lane for this change stream.
- Run the local commit/push gate before any eventual commit or push.

## Suggested Execution

- Sequential: Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 6 -> Task 7 -> Task 5, because the app-level cleanup depends on the stabilized settings-window boundaries, and closeout should still happen last.
- Parallelizable: None; this is one narrow cleanup lane with a shared ownership surface.
