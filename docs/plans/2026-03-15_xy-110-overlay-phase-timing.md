```json
{
  "spec": {
    "schema": "plan/1",
    "plan_id": "xy-110-overlay-phase-timing",
    "goal": "Execute XY-110 by extending rsnap overlay observability with structured redraw phase timing for the overlay renderer and live/loupe hot paths without introducing noisy healthy-run logs.",
    "success_criteria": [
      "A valid saved plan/1 artifact exists for XY-110 and is used as the only execution authority for this lane.",
      "Overlay renderer logs expose machine-readable phase timing for the major CPU-side redraw stages needed to localize regressions in the overlay path.",
      "Live or loupe hot-path application logs expose a structured timing signal that can be compared across runs instead of only surfacing over-budget incidents.",
      "Verification evidence shows the touched overlay test surface still passes after the instrumentation changes."
    ],
    "constraints": [
      "Keep plan-writing and plan-execution responsibilities separate: execution may update only runtime state, not strategy or task topology.",
      "Reuse existing tracing and slow-operation patterns instead of inventing a separate logging subsystem.",
      "Keep healthy-run noise low: detailed phase breakdowns may live at trace level, while warn/debug escalation should remain gated by existing frame-budget semantics.",
      "Do not expand the lane into smoke-script or runbook work unless the instrumentation change strictly requires a tiny supporting adjustment."
    ],
    "defaults": {
      "issue_id": "XY-110",
      "issue_url": "https://linear.app/hack-ink/issue/XY-110/perf-instrument-overlay-redraw-and-liveloupe-hot-paths-with-phase",
      "overlay_file": "packages/rsnap-overlay/src/overlay.rs"
    },
    "tasks": [
      {
        "id": "task-1",
        "title": "Map the current overlay redraw and live-sample observability gaps",
        "status": "done",
        "objective": "Read the current overlay redraw, loupe redraw, and live-sample logging paths to determine where phase timing is already present and which hot-path gaps XY-110 still needs to cover.",
        "inputs": [
          "packages/rsnap-overlay/src/overlay.rs",
          "packages/rsnap-overlay/src/overlay/session_state.rs",
          "scripts/live-loupe-perf-smoke-macos.sh",
          "XY-110"
        ],
        "outputs": [
          "A concrete instrumentation cut that adds missing phase signals without duplicating existing warnings."
        ],
        "verification": [
          "Confirm which redraw substeps already use warn_if_redraw_substep_slow or slow-op warnings.",
          "Identify the missing structured phase timing in the overlay renderer and live-sample apply paths."
        ],
        "depends_on": []
      },
      {
        "id": "task-2",
        "title": "Instrument overlay renderer and live/loupe phase timing",
        "status": "done",
        "objective": "Add structured timing summaries and bounded slow-phase escalation for the overlay renderer draw path and live/loupe application path using the existing tracing patterns.",
        "inputs": [
          "Evidence from task-1"
        ],
        "outputs": [
          "Updated overlay redraw instrumentation in packages/rsnap-overlay/src/overlay.rs",
          "Any targeted helper types or tests needed to keep the new signals maintainable"
        ],
        "verification": [
          "Overlay renderer instrumentation exposes phase timing for major redraw stages such as egui work, texture/upload work, frame acquisition, and presentation.",
          "Live or loupe hot-path application timing is available as a structured signal without spamming healthy runs."
        ],
        "depends_on": [
          "task-1"
        ]
      },
      {
        "id": "task-3",
        "title": "Verify the touched overlay instrumentation surface and record evidence",
        "status": "done",
        "objective": "Run fresh verification for the overlay crate after the instrumentation change, then update the saved plan state with explicit evidence and any bounded skips.",
        "inputs": [
          "Updated overlay instrumentation",
          "docs/plans/2026-03-15_xy-110-overlay-phase-timing.md"
        ],
        "outputs": [
          "Plan evidence capturing verification of the overlay instrumentation lane"
        ],
        "verification": [
          "cargo test -p rsnap-overlay overlay:: --lib",
          "git diff --check",
          "python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-15_xy-110-overlay-phase-timing.md"
        ],
        "depends_on": [
          "task-2"
        ]
      }
    ],
    "replan_policy": {
      "owner": "plan-writing",
      "triggers": [
        "The existing overlay redraw metrics already fully satisfy XY-110 and no real instrumentation gap remains.",
        "Covering the missing signals requires a materially broader logging or metrics subsystem change than localized overlay instrumentation.",
        "Verification reveals that smoke parsing or repo-native tasks must change in a larger way instead of remaining a follow-up issue."
      ]
    }
  },
  "state": {
    "phase": "done",
    "current_task_id": null,
    "next_task_id": null,
    "blockers": [],
    "evidence": [
      "Task 1 evidence: the overlay runtime already emitted slow-operation warnings for overlay.window_renderer_acquire_frame, overlay.window_renderer_render_frame, overlay.hud_redraw.* substeps, overlay.loupe_redraw.reposition, and overlay.live_sample_apply_latency when a sample apply exceeded budget.",
      "Task 1 evidence: the remaining gap was a machine-readable healthy-run phase summary for the overlay renderer and live-sample apply path, plus a loupe redraw total signal that did not depend on a slow-path incident.",
      "Task 2 outcome: added structured overlay.window_renderer_phase_timing trace events for the overlay and loupe-tile renderer paths, with per-phase timing fields for prepare_input, sync_hud_bg, run_egui, update_hud_blur_uniform, sync_egui_textures, tessellate, acquire_frame, render_frame, and total.",
      "Task 2 outcome: added bounded slow-phase escalation for previously unlocalized renderer substeps such as prepare_input, sync_hud_bg, run_egui, sync_egui_textures, and tessellate using the existing warn_if_redraw_substep_slow pattern.",
      "Task 2 outcome: added overlay.hud_redraw_phase_timing and overlay.loupe_redraw_phase_timing trace events plus overlay.loupe_redraw.total warn gating so healthy runs expose phase data while slow runs remain easy to triage.",
      "Task 2 outcome: added overlay.live_sample_apply_phase trace events in both the macOS stream path and worker-response path, while preserving overlay.live_sample_apply_latency debug escalation when apply time exceeds the frame budget.",
      "Task 3 verification: cargo make fmt-rust completed successfully.",
      "Task 3 verification: cargo test -p rsnap-overlay overlay:: --lib passed with 62 tests passing and 0 failures.",
      "Task 3 verification: git diff --check returned clean.",
      "Task 3 verification: python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-15_xy-110-overlay-phase-timing.md returned OK.",
      "Task 3 skip evidence: no smoke-script change was required because the new instrumentation reuses the existing tracing surface and can be consumed by follow-up smoke/runbook work."
    ],
    "last_updated": "2026-03-15T10:34:00Z",
    "replan_reason": null,
    "context_snapshot": {
      "depends_on_issue": "XY-108",
      "issue_id": "XY-110",
      "user_requested_issue_by_issue_execution": true,
      "user_requested_plan_writing_then_executing": true,
      "overlay_phase_signal": "overlay.window_renderer_phase_timing"
    }
  }
}
```
