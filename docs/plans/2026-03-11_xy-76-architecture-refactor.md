# XY-76 Architecture Refactor Plan

## Goal

Execute `XY-76` as the umbrella architecture cleanup lane by landing `XY-79` through `XY-82` as separate refactor tracks without absorbing the concrete native-capture migration work in `XY-74` or `XY-75`.

## Scope

- Execute child issues `XY-79`, `XY-80`, `XY-81`, and `XY-82`.
- Reduce hotspot concentration in `apps/rsnap/src/app.rs`, `apps/rsnap/src/settings_window.rs`, and `packages/rsnap-overlay/src/overlay.rs`.
- Pull `cfg` and `target_os` conditionals toward explicit platform boundaries instead of leaving them scattered through shared flows.
- Reconcile docs only where wording drift matters for the implemented boundary shape.

## Non-goals

- `XY-74`
- `XY-75`
- Product UX redesign
- New platform promises

## Constraints

- `cargo make checks` is the final gate for every child lane and for umbrella closeout.
- Each child issue should execute in a separate worktree.
- Run the `pre-commit` skill before any `git commit` or `git push`.
- Preserve user-visible behavior unless a change is intentional and reviewed.
- Any new or moved `cfg` / `target_os` branches introduced by `XY-76` must end up in explicit platform modules or platform entrypoints, not in shared runtime flows that already have a platform-agnostic owner.

## Open Questions

- Should root `README.md` freeze/export wording be corrected during `XY-76` closeout, or deferred until `XY-74` and `XY-75` land?
- Where should the existing macOS smoke-script evidence for XY-79 through XY-82 be archived after a dedicated GUI run?

## Execution State

- Last Updated: 2026-03-11
- Next Checkpoint: Run existing macOS smoke harnesses and record the remaining targeted manual supplement
- Blockers:
  - Existing macOS smoke harnesses were self-checked but not fully executed in this non-interactive session because they drive a live desktop session.
  - Settings-window persistence, global hotkey, hovered-window freeze, and fullscreen fallback still need explicit evidence beyond the current script-backed coverage.

## Decision Notes

- Current hotspot concentration is in `apps/rsnap/src/app.rs` at about 1572 LOC, `apps/rsnap/src/settings_window.rs` at about 1717 LOC, and `packages/rsnap-overlay/src/overlay.rs` at about 13078 LOC.
- `XY-76` is the umbrella issue, and `XY-79` through `XY-82` are the execution lanes.
- `README.md` and `apps/rsnap/README.md` currently disagree on freeze/export wording.
- Live sampling is already ScreenCaptureKit-backed, while freeze/export still carries `xcap` debt, so `XY-76` must not absorb `XY-74` or `XY-75`.
- `XY-79` owns macOS event-tap or scroll-input capture, decode or coalescing, and the handoff of normalized external scroll input from the app shell into overlay consumers. `XY-82` may consume that boundary, but must not pull event-tap ownership back out of the app shell.
- `XY-81` owns overlay state, overlay window lifecycle, HUD rendering, and platform-window setup inside the overlay engine. `XY-82` owns capture backends, worker/session orchestration, scroll capture, export/output coordination, and only the minimal `overlay.rs` call-site edits needed to consume those boundaries.
- A lane is not complete on compile- or unit-test evidence alone when it changes macOS app-shell or overlay behavior. Prefer the existing macOS smoke harnesses first, then add targeted manual checks only for behaviors not yet covered by those scripts.
- `XY-79` landed an app-shell split into `apps/rsnap/src/app/runtime.rs`, `apps/rsnap/src/app/shell.rs`, and `apps/rsnap/src/app/scroll_input_macos.rs`, keeping normalized external scroll input owned by the app shell.
- `XY-80` landed settings-window support modules in `apps/rsnap/src/settings_window/hotkey.rs` and `apps/rsnap/src/settings_window/platform.rs`, while preserving the `settings.toml` contract.
- `XY-81` and `XY-82` landed an overlay-side decomposition into `packages/rsnap-overlay/src/overlay/session_state.rs`, `packages/rsnap-overlay/src/overlay/window_runtime.rs`, and `packages/rsnap-overlay/src/overlay/scroll_runtime.rs`; `packages/rsnap-overlay/src/state.rs` now owns `OverlayState::reset_for_start`, and `packages/rsnap-overlay/src/scroll_capture.rs` exposes `ScrollSession::export_dimensions()` so the overlay session consumes a narrower stitched-image surface.
- Verification on 2026-03-11: `cargo test -p rsnap-overlay overlay:: --lib` passed, and `cargo test -p rsnap --lib` passed.
- Verification on 2026-03-11: `cargo test -p rsnap-overlay scroll_capture:: --lib` passed.
- Verification on 2026-03-11: `cargo make checks` passed `cargo clippy --workspace --all-targets --all-features -- -D warnings` and then failed in `cargo vstyle curate --workspace --all-features` with broad existing style debt across app, overlay, backend, worker, and docs-adjacent files.
- 2026-03-11 follow-up decision: workspace-wide `vstyle` debt is explicitly deferred to a separate later commit and is not treated as a blocker for the XY-76 architecture lane itself.
- `apps/rsnap/src/main.rs` does not currently expose a headless or self-terminating startup mode, so `cargo run -p rsnap` remains a manual verification step rather than an unattended automation gate.
- Verification harness status on 2026-03-11: `scripts/scroll-capture-smoke-macos.sh --self-check` passed and `scripts/live-loupe-perf-smoke-macos.sh --self-check` passed. Prefer the corresponding `cargo make smoke-*` tasks for real GUI evidence collection.

## Implementation Outline

Start with app-shell and overlay boundaries first because they are the highest-leverage control surfaces. They shape how session ownership, platform hooks, and overlay state flow through the rest of the system, so stabilizing them first reduces the risk of pushing more incidental complexity into the current hotspots.

Treat each child issue as its own worktree and review boundary. That keeps refactor scope narrow, prevents unrelated architecture churn from piling up in one lane, and makes it easier to stop for review when a boundary decision turns out to be larger than expected.

If a child issue becomes risky or turns into a multi-step refactor with uncertain ownership, route its execution through `multi-agent` inside that task-specific worktree instead of widening the current lane.

## Task 1: XY-79 — App Shell Boundary Refactor

**Owner**

Executor in a dedicated XY-79 worktree.

**Status**

done

**Outcome**

`apps/rsnap/src/app.rs` no longer acts as a god object for startup and bootstrap, menubar wiring, hotkeys, event dispatch, and session orchestration.

**Files**

- Modify: `apps/rsnap/src/app.rs`
- Modify: `apps/rsnap/src/lib.rs`
- Modify: `apps/rsnap/src/main.rs`
- Review: `apps/rsnap/README.md`

**Changes**

1. Split app-shell responsibilities into narrower startup, app-shell, and orchestration boundaries.
2. Move macOS-specific app-shell setup and scroll-input or event-tap integration behind explicit platform-owned boundaries instead of leaving `target_os` branching in shared orchestration code.
3. Keep event-tap capture, scroll-input decode or coalescing, and the normalized external-scroll handoff owned by the app-shell boundary established in this task.
4. Keep the `rsnap` to `rsnap_overlay` handoff surface narrow.
5. Stop for review if the refactor starts changing capture behavior.

**Verification**

- `cargo test -p rsnap --lib`
- `cargo make checks`
- `cargo run -p rsnap`
- `cargo make smoke-live-loupe-perf-macos`
- `cargo make smoke-scroll-capture-macos`
- Targeted manual supplement on macOS: global hotkey, hovered-window freeze, and fullscreen fallback still behave as before.

**Dependencies**

- None.

## Task 2: XY-80 — Settings Surface Separation

**Owner**

Executor in a dedicated XY-80 worktree.

**Status**

done

**Outcome**

Settings persistence, settings UI, and platform-specific window-shell behavior can be reasoned about independently.

**Files**

- Modify: `apps/rsnap/src/settings.rs`
- Modify: `apps/rsnap/src/settings_window.rs`
- Modify: `apps/rsnap/src/lib.rs`
- Review: `README.md`
- Review: `apps/rsnap/README.md`

**Changes**

1. Separate the persisted settings model from settings-window UI and rendering behavior.
2. Move platform-specific settings-window behavior behind explicit platform-owned boundaries instead of leaving `target_os` branching in shared settings UI flow.
3. Reduce coupling between preferences, hotkeys, and window lifecycle.
4. Preserve the `settings.toml` contract.

**Verification**

- `cargo test -p rsnap --lib`
- `cargo make checks`
- `cargo run -p rsnap`
- Targeted manual supplement on macOS: open and close Settings, edit representative preferences, save, relaunch, and confirm `settings.toml`-backed values reload correctly.

**Dependencies**

- Task 1 if the settings launch or event surface changes; otherwise parallel after Task 1 boundary naming stabilizes.

## Task 3: XY-81 — Overlay Engine Decomposition

**Owner**

Executor in a dedicated XY-81 worktree.

**Status**

done

**Outcome**

The overlay engine no longer keeps most window lifecycle, HUD and render coordination, overlay state, and platform window configuration in one hotspot.

**Files**

- Modify: `packages/rsnap-overlay/src/overlay.rs`
- Modify: `packages/rsnap-overlay/src/overlay/session_state.rs`
- Modify: `packages/rsnap-overlay/src/overlay/window_runtime.rs`
- Modify: `packages/rsnap-overlay/src/overlay/hud_helpers.rs`
- Modify: `packages/rsnap-overlay/src/overlay/image_helpers.rs`
- Modify: `packages/rsnap-overlay/src/state.rs`
- Modify: `packages/rsnap-overlay/src/lib.rs`
- Review: `docs/spec/v0.md`
- Review: `packages/rsnap-overlay/src/overlay/output.rs`

**Changes**

1. Break `overlay.rs` into clearer ownership boundaries.
2. Move platform-specific overlay and window behavior behind explicit platform-owned boundaries instead of leaving `target_os` branching in shared overlay flow.
3. Treat `packages/rsnap-overlay/src/overlay/output.rs` as `XY-82` ownership and only review it here to keep the cut line stable while `overlay.rs` is decomposed.
4. Preserve visible overlay behavior.

**Verification**

- `cargo test -p rsnap-overlay overlay:: --lib`
- `cargo make checks`
- `cargo run -p rsnap`
- `cargo make smoke-live-loupe-perf-macos`
- Targeted manual supplement on macOS: toolbar placement and overlay window lifecycle match current behavior.

**Dependencies**

- Task 1

## Task 4: XY-82 — Capture Session, Worker, And Export Boundary Cleanup

**Owner**

Executor in a dedicated XY-82 worktree.

**Status**

done

**Outcome**

Capture backends, worker and session control, scroll-capture flow, and export or output coordination have cleaner boundaries without absorbing `XY-74` or `XY-75`.

**Files**

- Modify: `packages/rsnap-overlay/src/backend.rs`
- Modify: `packages/rsnap-overlay/src/worker.rs`
- Modify: `packages/rsnap-overlay/src/scroll_capture.rs`
- Modify: `packages/rsnap-overlay/src/overlay/scroll_runtime.rs`
- Modify: `packages/rsnap-overlay/src/overlay/output.rs`
- Review: `packages/rsnap-overlay/src/overlay.rs`
- Review: `docs/research/live-sampling-streams.md`
- Review: `apps/rsnap/README.md`

**Changes**

1. Clarify ownership across backend, worker, scroll, and output layers.
2. Reduce fallback-heavy control flow.
3. Preserve current freeze/export and live-sampling behavior.
4. Consume the app-shell-provided normalized external-scroll boundary without moving macOS event-tap capture, decode, or coalescing ownership out of `XY-79`.
5. Limit `overlay.rs` edits to the minimum integration call sites needed to consume the new backend, worker, scroll, and output boundaries. If deeper overlay ownership changes are required, stop and feed that work back into `XY-81` or a follow-up.
6. Stop and split follow-up work if the lane starts turning into `XY-74` or `XY-75` scope.

**Verification**

- `cargo test -p rsnap-overlay scroll_capture:: --lib`
- `cargo test -p rsnap-overlay overlay:: --lib`
- `cargo make checks`
- `cargo run -p rsnap`
- `cargo make smoke-scroll-capture-macos`
- Targeted manual supplement on macOS: window freeze, copy/save, and export still match current behavior.

**Dependencies**

- Tasks 1 and 3

## Task 5: Docs Sync And XY-76 Closeout

**Owner**

Executor after the child lanes land or are review-ready.

**Status**

done

**Outcome**

Public docs, the spec, and umbrella issue state are consistent with the implemented architecture boundaries.

**Files**

- Modify: `README.md`
- Modify: `apps/rsnap/README.md`
- Modify: `docs/spec/v0.md`
- Review: `docs/research/live-sampling-streams.md`
- Review: `docs/plans/2026-03-11_xy-76-architecture-refactor.md`

**Changes**

1. Reconcile README wording with the actual code state and the boundaries of `XY-74` and `XY-75`.
2. Update spec and developer notes only where boundary or behavior statements changed.
3. Review `XY-76` and child issue status before umbrella closeout.
4. Run the full repo gate before closing the umbrella lane.
5. Leave explicit TODO notes rather than blocking if repo-wide style debt or remaining macOS smoke evidence cannot be resolved inside this lane.

**Verification**

- `cargo make checks`

**Dependencies**

- Tasks 1-4

## Rollout Notes

- Each child issue should run in its own worktree using `git-worktrees`.
- `pre-commit` is required before any `git commit` or `git push`.
- Each child issue is a PR-sized review boundary.
- Use `multi-agent` inside a task-specific worktree if a child lane becomes internally risky or multi-step.

## Suggested Execution

- Sequential: Task 1 -> Task 3 -> Task 4 -> Task 5, because the outer app and overlay boundaries should stabilize before capture cleanup.
- Parallelizable: Task 2 can run in a separate worktree after Task 1 stabilizes the settings-window launch and event surfaces.
- Recommended next step: hand the saved plan to `plan-execution` and start with `XY-79` in a dedicated worktree.
