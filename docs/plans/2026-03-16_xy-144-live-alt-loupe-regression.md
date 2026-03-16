```json
{
  "spec": {
    "schema": "plan/1",
    "plan_id": "xy-144-live-alt-loupe-regression",
    "goal": "Execute XY-144 by restoring the macOS live-Alt loupe interaction so the expanded state renders as two distinct magnetic strips with independent rounded clipping and keeps following pointer movement while expanded.",
    "success_criteria": [
      "A valid saved plan/1 artifact exists for XY-144 and is used as the execution authority for this lane.",
      "macOS live-Alt expansion no longer routes the loupe through a combined HUD content rect; the HUD strip and loupe strip render as distinct windows with independent rounded masks.",
      "The live loupe strip is vertically offset from the HUD strip with a visible gap and hangs below the pos/RGB strip by default instead of sharing the same top-left origin.",
      "Expanded live-Alt positioning resumes pointer-follow behavior while the cursor moves, including cursor-only movement where overlay content does not otherwise change.",
      "Focused regression tests lock in the live-Alt window-routing and redraw/positioning contract.",
      "Fresh verification evidence shows the touched rsnap-overlay surface still passes after the fix.",
      "Manual macOS live-Alt smoke confirms the loupe strip hangs below the pos/RGB strip with a visible gap and keeps following the cursor while expanded."
    ],
    "constraints": [
      "Keep the fix scoped to the live-Alt HUD/loupe routing and pointer-follow path unless execution reveals a tighter dependency.",
      "Do not widen frozen-toolbar behavior or unrelated overlay redraw policy while restoring the live-Alt interaction.",
      "Use repo-native Rust verification for the touched overlay scope before claiming completion."
    ],
    "defaults": {
      "issue_id": "XY-144",
      "issue_url": "https://linear.app/hack-ink/issue/XY-144/fix-macos-live-alt-loupe-regression-cluster",
      "related_issue_ids": [
        "XY-145",
        "XY-146",
        "XY-147"
      ],
      "overlay_file": "packages/rsnap-overlay/src/overlay.rs"
    },
    "tasks": [
      {
        "id": "task-1",
        "title": "Confirm the live-Alt regression boundary and root cause",
        "status": "done",
        "objective": "Trace the current macOS live-Alt routing and compare it with the pre-regression behavior so the fix is grounded in a verified root-cause statement instead of symptom-level guesses.",
        "inputs": [
          "packages/rsnap-overlay/src/overlay.rs",
          "packages/rsnap-overlay/src/overlay/window_runtime.rs",
          "Recent overlay commits touching live loupe and HUD redraw behavior",
          "XY-144",
          "XY-145",
          "XY-146",
          "XY-147"
        ],
        "outputs": [
          "A concrete root-cause statement covering the merged-window presentation regression and the stalled pointer-follow path."
        ],
        "verification": [
          "Trace the current live-Alt flow through live_loupe_uses_hud_window, hud_window_content_rect, set_alt_loupe_window_visible, and update_hud_window_position.",
          "Compare against the last known separate-window implementation to verify where the routing and follow behavior diverged."
        ],
        "depends_on": []
      },
      {
        "id": "task-2",
        "title": "Anchor the live loupe strip below the HUD strip with independent geometry",
        "status": "done",
        "objective": "Keep the macOS live-Alt presentation on distinct HUD and loupe windows and make the loupe strip inherit its default live position from the HUD strip geometry instead of starting from nearly the same cursor-relative origin.",
        "inputs": [
          "Evidence from task-1"
        ],
        "outputs": [
          "Updated live-Alt window-routing and layout code in packages/rsnap-overlay/src/overlay.rs"
        ],
        "verification": [
          "Live-Alt no longer renders the loupe inside the HUD content rect on macOS.",
          "The live loupe strip defaults to a position below the HUD strip with an explicit gap instead of sharing the same top-left origin."
        ],
        "depends_on": [
          "task-1"
        ]
      },
      {
        "id": "task-3",
        "title": "Restore pointer-follow behavior for expanded live-Alt",
        "status": "done",
        "objective": "Ensure cursor movement continues to move the expanded HUD/loupe presentation after the live geometry fix, without regressing the narrow overlay redraw split.",
        "inputs": [
          "Updated live-Alt routing from task-2"
        ],
        "outputs": [
          "Updated live cursor positioning/redraw logic in packages/rsnap-overlay/src/overlay.rs"
        ],
        "verification": [
          "Expanded live-Alt updates HUD and loupe positioning on cursor movement.",
          "Same-monitor cursor-only movement still does not widen overlay redraw ownership beyond the existing HUD-only split."
        ],
        "depends_on": [
          "task-2"
        ]
      },
      {
        "id": "task-4",
        "title": "Add focused regression coverage and verify the overlay surface",
        "status": "done",
        "objective": "Lock in the restored live-Alt contract with targeted tests, run fresh automated verification, and keep the plan open until a manual macOS smoke confirms the visual gap and follow behavior.",
        "inputs": [
          "Updated overlay implementation from task-3",
          "docs/plans/2026-03-16_xy-144-live-alt-loupe-regression.md"
        ],
        "outputs": [
          "New or updated overlay unit tests and recorded verification evidence"
        ],
        "verification": [
          "cargo test -p rsnap-overlay overlay:: --lib",
          "cargo make fmt-rust",
          "git diff --check",
          "python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-16_xy-144-live-alt-loupe-regression.md",
          "Manual macOS smoke: enter live mode, hold Alt, verify the loupe strip sits below the HUD strip with visible spacing and follows cursor movement."
        ],
        "depends_on": [
          "task-3"
        ]
      }
    ],
    "replan_policy": {
      "owner": "plan-writing",
      "triggers": [
        "Restoring the dual-strip live-Alt presentation requires a broader overlay window-architecture rewrite instead of a local routing correction.",
        "Pointer-follow behavior still stalls after the separate-window routing is restored, implying a different event or waker contract than the current cursor-positioning path.",
        "Targeted verification exposes unrelated overlay regressions that cannot be bounded to the live-Alt interaction lane."
      ]
    }
  },
  "state": {
    "phase": "done",
    "current_task_id": null,
    "next_task_id": null,
    "blockers": [],
    "evidence": [
      "Task 1 evidence: current HEAD routed macOS live-Alt through live_loupe_uses_hud_window() and hud_window_content_rect(...), while update_hud_window_position(...) returned early during live Alt, so the expanded presentation was both composited into one HUD rect and prevented from following pointer movement through the normal HUD position path.",
      "Task 1 evidence: comparing against the pre-c0f06147 overlay implementation confirmed the live loupe previously used its own loupe window on the live path, with update_hud_window_position(...) continuing to move the HUD and update_loupe_window_position(...) handling the second strip independently.",
      "Task 2 outcome: packages/rsnap-overlay/src/overlay.rs now keeps live-Alt on a dedicated loupe window, no longer unions the loupe tile into the HUD content rect, and no longer hides/skips the loupe window on the live path.",
      "Task 3 outcome: update_hud_window_position(...) no longer exits early for live Alt unless the loupe is explicitly routed through the HUD window, so the expanded HUD/loupe path resumes pointer-follow positioning via the standard HUD move flow.",
      "Task 4 outcome: added focused macOS regression tests covering dedicated live-Alt loupe window routing, compact HUD content rect sizing, the fact that live-Alt loupe redraw is not skipped on the live path, and the shared Alt press-edge state machine used by modifier/key fallback handling.",
      "Task 4 verification: cargo test -p rsnap-overlay overlay:: --lib passed with 68 tests passing and 0 failures after the HUD-anchored 8px loupe spacing and Alt activation fixes.",
      "Task 4 verification: cargo make fmt-rust completed successfully after the final patch set.",
      "Task 4 verification: git diff --check returned clean after the final patch set.",
      "Task 4 verification: python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-16_xy-144-live-alt-loupe-regression.md returned OK.",
      "Task 4 implementation update on 2026-03-17: live loupe default geometry now anchors to the HUD helper window height/outer position when available, with an 8px strip gap so the loupe hangs below the HUD strip instead of starting from a near-identical cursor-relative origin.",
      "Task 4 implementation update on 2026-03-17: Alt activation now refreshes cursor/monitor context on the activation edge and routes Alt state changes through shared modifier/key handling, so the first startup Alt press and toggle mode no longer depend on a follow-up mouse move to surface the HUD/loupe windows.",
      "Task 4 verification: user-reported manual macOS GUI smoke on 2026-03-17 confirmed first-launch Alt activation, toggle-mode activation without extra mouse movement, the restored 8px strip gap, and continued pointer-follow behavior while expanded.",
      "Replan evidence on 2026-03-17: manual user validation after the first fix reported residual overlap because the loupe strip still appeared to start from the same top-left point as the pos/RGB HUD strip.",
      "Replan evidence on 2026-03-17: current packages/rsnap-overlay/src/overlay.rs still positions live loupe windows from cursor+48/cursor+32 while the HUD window uses cursor+48/cursor+24, leaving only an 8px vertical offset and explaining the remaining overlap."
    ],
    "last_updated": "2026-03-16T16:49:00Z",
    "replan_reason": null,
    "context_snapshot": {
      "issue_id": "XY-144",
      "related_issue_ids": [
        "XY-145",
        "XY-146",
        "XY-147"
      ],
      "user_requested_plan_and_impl": true,
      "residual_overlap_reported_on_2026_03_17": true,
      "manual_smoke_confirmed_on_2026_03_17": true
    }
  }
}
```
