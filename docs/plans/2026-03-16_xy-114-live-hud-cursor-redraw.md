```json
{
  "spec": {
    "schema": "plan/1",
    "plan_id": "xy-114-live-hud-cursor-redraw",
    "goal": "Execute XY-114 by fixing live screenshot HUD staleness on cursor-only movement so cursor-derived HUD fields repaint immediately even when sampled RGB stays unchanged, without widening overlay redraw churn.",
    "success_criteria": [
      "A valid saved plan/1 artifact exists for XY-114 and is used as the only execution authority for this lane.",
      "Live cursor-only movement repaints the HUD so coordinate and other cursor-derived fields stay in sync even when sampled RGB and hover state are unchanged.",
      "Overlay redraw ownership stays narrow: same-monitor cursor-only movement still does not trigger overlay window redraw unless overlay-relevant state changes.",
      "Targeted tests cover the new HUD redraw decision and preserve the existing narrow overlay redraw behavior.",
      "Fresh verification evidence shows the touched rsnap-overlay test surface still passes."
    ],
    "constraints": [
      "Keep the fix scoped to live cursor/HUD redraw ownership unless execution reveals a tighter dependency.",
      "Do not widen overlay redraw policy for same-monitor cursor-only movement; fix the stale HUD path separately.",
      "Use repo-native Rust verification on the touched overlay scope before claiming completion."
    ],
    "defaults": {
      "issue_id": "XY-114",
      "issue_url": "https://linear.app/hack-ink/issue/XY-114/fix-live-screenshot-hudwidget-staleness-on-cursor-only-movement",
      "overlay_file": "packages/rsnap-overlay/src/overlay.rs"
    },
    "tasks": [
      {
        "id": "task-1",
        "title": "Confirm the redraw ownership gap and smallest safe fix",
        "status": "done",
        "objective": "Read the live cursor-move and live-sample redraw paths to confirm why cursor-only movement leaves HUD content stale and identify the narrowest fix that preserves existing overlay redraw constraints.",
        "inputs": [
          "packages/rsnap-overlay/src/overlay.rs",
          "packages/rsnap-overlay/src/overlay/window_runtime.rs",
          "XY-114"
        ],
        "outputs": [
          "A concrete root-cause statement and the minimal implementation cut for HUD redraw ownership."
        ],
        "verification": [
          "Trace the live cursor-move path and confirm cursor state and HUD position update before redraw requests are decided.",
          "Trace the live-sample apply path and confirm HUD redraw currently depends on hover, RGB, or loupe changes rather than cursor movement itself."
        ],
        "depends_on": []
      },
      {
        "id": "task-2",
        "title": "Implement the narrow live HUD redraw fix",
        "status": "done",
        "objective": "Adjust the live cursor-move path so HUD redraw requests fire for cursor-driven HUD updates without widening same-monitor overlay redraws.",
        "inputs": [
          "Evidence from task-1"
        ],
        "outputs": [
          "Updated live cursor redraw ownership in packages/rsnap-overlay/src/overlay.rs"
        ],
        "verification": [
          "Cursor-only movement on the same monitor requests a HUD redraw.",
          "Same-monitor cursor-only movement still does not request an overlay redraw unless overlay state changes."
        ],
        "depends_on": [
          "task-1"
        ]
      },
      {
        "id": "task-3",
        "title": "Add focused regression tests for the redraw split",
        "status": "done",
        "objective": "Add targeted tests that lock in the new HUD redraw decision while preserving the existing narrow overlay redraw contract.",
        "inputs": [
          "Updated redraw logic from task-2"
        ],
        "outputs": [
          "New or updated overlay unit tests"
        ],
        "verification": [
          "Tests cover HUD redraw on same-monitor cursor movement and no HUD redraw when cursor and monitor are unchanged.",
          "Tests continue to cover overlay redraw only for monitor or drag changes."
        ],
        "depends_on": [
          "task-2"
        ]
      },
      {
        "id": "task-4",
        "title": "Verify the touched overlay surface and record evidence",
        "status": "done",
        "objective": "Run focused verification for the overlay crate after the fix, then update the saved plan state with explicit evidence and any bounded skips.",
        "inputs": [
          "Updated overlay implementation and tests",
          "docs/plans/2026-03-16_xy-114-live-hud-cursor-redraw.md"
        ],
        "outputs": [
          "Plan evidence capturing verification of the XY-114 lane"
        ],
        "verification": [
          "cargo test -p rsnap-overlay overlay:: --lib",
          "cargo make fmt-rust",
          "git diff --check",
          "python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-16_xy-114-live-hud-cursor-redraw.md"
        ],
        "depends_on": [
          "task-3"
        ]
      }
    ],
    "replan_policy": {
      "owner": "plan-writing",
      "triggers": [
        "Fixing the stale HUD requires changing overlay redraw strategy rather than splitting HUD ownership cleanly.",
        "Targeted tests show the stale coordinates originate from a different live-window rendering path than the current cursor-move and sample-apply ownership split.",
        "Verification exposes a broader live-window architecture issue that cannot be solved with a narrow HUD redraw change."
      ]
    }
  },
  "state": {
    "phase": "done",
    "current_task_id": null,
    "next_task_id": null,
    "blockers": [],
    "evidence": [
      "Task 1 evidence: live cursor-move paths in packages/rsnap-overlay/src/overlay.rs update state.cursor and HUD position via update_cursor_for_live_move(...), but same-monitor cursor-only moves only request repaint through live_sample_request_redraw_intent(...) or apply_live_cursor_sample_detail(...), which means no HUD repaint occurs when hover, RGB, and loupe state stay unchanged.",
      "Task 1 evidence: request_redraw_live_sample_targets(...) in packages/rsnap-overlay/src/overlay/window_runtime.rs already splits overlay, HUD, and loupe redraw targets, so the minimal fix is to request only a HUD redraw on cursor-driven HUD state changes instead of widening overlay redraw ownership.",
      "Task 2 outcome: added OverlaySession::live_hud_redraw_needed_for_cursor_update(...) and used it from the live cursor-move and live cursor-tick paths so cursor-only movement now requests a HUD redraw while leaving overlay redraw policy unchanged.",
      "Task 3 outcome: added a focused unit test covering HUD redraw on cursor or monitor changes, while the existing live_overlay_redraw_needed_for_cursor_update_only_for_monitor_or_drag_changes test continues to lock in narrow overlay redraw behavior.",
      "Task 4 verification: cargo make fmt-rust completed successfully.",
      "Task 4 verification: cargo test -p rsnap-overlay overlay:: --lib passed with 63 tests passing and 0 failures.",
      "Task 4 verification: git diff --check returned clean.",
      "Task 4 verification: python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-16_xy-114-live-hud-cursor-redraw.md returned OK.",
      "Task 4 verification: manual macOS GUI smoke on 2026-03-16 launched a temporary solid-color AppKit window, entered rsnap live capture from the tray menu, held the HUD idle long enough to confirm overlay.hud_redraw_phase_timing stayed stable at 643 -> 643, then moved the cursor across same-monitor uniform-color points and observed redraw counts increase monotonically to 644, 645, and 646.",
      "Task 4 verification note: direct screen capture could not be used as the decisive oracle because the rsnap-hud window is hidden from captured output by current macOS window-sharing behavior, so the GUI closeout used fresh trace-level HUD redraw evidence instead."
    ],
    "last_updated": "2026-03-16T12:04:29Z",
    "replan_reason": null,
    "context_snapshot": {
      "issue_id": "XY-114",
      "user_requested_issue_by_issue_execution": true,
      "linear_issue_state": "Done"
    }
  }
}
```
