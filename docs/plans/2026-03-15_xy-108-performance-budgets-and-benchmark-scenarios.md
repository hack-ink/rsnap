```json
{
  "spec": {
    "schema": "plan/1",
    "plan_id": "xy-108-performance-budgets-and-benchmark-scenarios",
    "goal": "Execute XY-108 by defining rsnap's normative performance-tracking contract, grounding it in current repo evidence, and aligning the issue graph to the new cadence policy before downstream implementation tickets proceed.",
    "success_criteria": [
      "A valid saved plan/1 artifact exists for XY-108 and is used as the only execution authority for this lane.",
      "A new normative spec document defines the active render cadence contract, tracked scenarios, metrics, thresholds, and known current-state gaps.",
      "Documentation routing is updated so agents can discover the new performance contract without relying on issue text or chat context.",
      "The rsnap performance-tracking Linear issue set explicitly covers any uncovered implementation gap revealed by the new contract."
    ],
    "constraints": [
      "Follow the repository docs governance: normative performance contracts belong under docs/spec, not docs/guide or docs/plans.",
      "Preserve the user-approved cadence interpretation: active render target is min(120 Hz, active display refresh ceiling).",
      "Use issue-by-issue execution; do not start XY-109 or later issues until XY-108 has a saved plan and execution state.",
      "Keep plan-writing and plan-execution responsibilities separate: execution may update only runtime state, not strategy or task topology."
    ],
    "defaults": {
      "issue_id": "XY-108",
      "issue_url": "https://linear.app/hack-ink/issue/XY-108/perf-define-rsnap-performance-budgets-and-benchmark-scenarios",
      "project_id": "157656f2-538d-4d03-bc08-b6953ec52671",
      "spec_doc": "docs/spec/performance_tracking.md"
    },
    "tasks": [
      {
        "id": "task-1",
        "title": "Capture current cadence behavior and contract gaps",
        "status": "done",
        "objective": "Gather current repo evidence for repaint cadence, smoke metrics, docs routing, and the mismatch between the agreed cadence contract and the live implementation.",
        "inputs": [
          "packages/rsnap-overlay/src/overlay.rs",
          "scripts/live-loupe-perf-smoke-macos.sh",
          "docs/index.md",
          "docs/spec/index.md",
          "XY-108"
        ],
        "outputs": [
          "Plan evidence entries describing the current cadence logic and the specific mismatch against the new contract.",
          "A concrete basis for the normative performance-tracking spec."
        ],
        "verification": [
          "Read current repaint interval logic and confirm whether it matches min(120 Hz, active display refresh ceiling).",
          "Read current smoke signals and doc routing inputs needed by the spec artifact."
        ],
        "depends_on": []
      },
      {
        "id": "task-2",
        "title": "Author the normative performance-tracking spec",
        "status": "done",
        "objective": "Create the authoritative performance-tracking spec for rsnap, including cadence assumptions, scenario inventory, metrics, thresholds, and explicit known gaps.",
        "inputs": [
          "Evidence from task-1",
          "docs/governance.md",
          "docs/spec/v0.md"
        ],
        "outputs": [
          "docs/spec/performance_tracking.md"
        ],
        "verification": [
          "The new spec follows the repository spec document contract.",
          "The spec explicitly records the min(120 Hz, active display refresh ceiling) cadence rule and the current implementation mismatch if it still exists."
        ],
        "depends_on": [
          "task-1"
        ]
      },
      {
        "id": "task-3",
        "title": "Route documentation to the new performance contract",
        "status": "done",
        "objective": "Update documentation routers and relevant cross-links so future work can discover the new performance contract directly from docs.",
        "inputs": [
          "docs/index.md",
          "docs/spec/index.md",
          "docs/spec/performance_tracking.md"
        ],
        "outputs": [
          "Updated docs/spec/index.md",
          "Any minimal cross-links required from existing normative or guide docs"
        ],
        "verification": [
          "Documentation routing points to docs/spec/performance_tracking.md from the correct normative entrypoint.",
          "Cross-links do not duplicate authoritative content."
        ],
        "depends_on": [
          "task-2"
        ]
      },
      {
        "id": "task-4",
        "title": "Align the Linear issue graph with the new contract",
        "status": "done",
        "objective": "Ensure the rsnap performance-tracking issues explicitly cover any implementation work revealed by the new contract, including cadence-alignment work if it is not already represented.",
        "inputs": [
          "docs/spec/performance_tracking.md",
          "Current rsnap Performance Tracking project issues"
        ],
        "outputs": [
          "Updated or newly created Linear issues and relations that cover the contract-following implementation work"
        ],
        "verification": [
          "The issue graph includes explicit ownership for cadence-alignment work if the current code still diverges from the contract.",
          "Downstream issues reference the contract issue rather than carrying ad hoc performance rules."
        ],
        "depends_on": [
          "task-2"
        ]
      },
      {
        "id": "task-5",
        "title": "Validate XY-108 artifacts and record closeout evidence",
        "status": "done",
        "objective": "Run the available validation for the doc and tracker updates, record explicit skip evidence for absent repo-native doc gates, and leave the plan state ready for downstream issues.",
        "inputs": [
          "Updated docs and Linear state",
          "docs/plans/2026-03-15_xy-108-performance-budgets-and-benchmark-scenarios.md"
        ],
        "outputs": [
          "Plan evidence showing what was validated and what was explicitly skipped"
        ],
        "verification": [
          "python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-15_xy-108-performance-budgets-and-benchmark-scenarios.md",
          "git diff --check"
        ],
        "depends_on": [
          "task-3",
          "task-4"
        ]
      }
    ],
    "replan_policy": {
      "owner": "plan-writing",
      "triggers": [
        "The agreed cadence contract changes again.",
        "Documentation governance requires moving the normative artifact to a different location.",
        "Executing XY-108 reveals that the required tracker topology is materially different from the current issue graph.",
        "Verification shows the lane must change repository strategy rather than just docs or tracker content."
      ]
    }
  },
  "state": {
    "phase": "done",
    "current_task_id": null,
    "next_task_id": null,
    "blockers": [],
    "evidence": [
      "Task 1 evidence: current overlay cadence logic in packages/rsnap-overlay/src/overlay.rs uses INTERACTIVE_REPAINT_FPS_FLOOR = 120 and repaint_interval_for_monitor() applies fps.max(INTERACTIVE_REPAINT_FPS_FLOOR), which conflicts with the agreed min(120 Hz, active display refresh ceiling) contract.",
      "Task 1 evidence: current coarse live-path smoke already parses overlay.window_renderer_acquire_frame, overlay.event_loop_stall, overlay.live_sample_apply_latency, and Slow operation detected warnings from scripts/live-loupe-perf-smoke-macos.sh.",
      "Task 2 outcome: authored docs/spec/performance_tracking.md as the normative performance-tracking contract, including cadence policy, tracked scenarios, execution environment classes, and the current implementation gap.",
      "Task 3 outcome: routed the new contract from docs/spec/index.md and added governing-spec cross-links from docs/spec/v0.md and docs/guide/live-sampling-streams.md.",
      "Task 4 outcome: created XY-115 to own repaint-scheduler alignment with the XY-108 cadence contract; XY-115 is blocked by XY-108 and related to XY-110.",
      "Task 5 verification: python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-15_xy-108-performance-budgets-and-benchmark-scenarios.md returned OK.",
      "Task 5 verification: git diff --check returned clean.",
      "Task 5 skip evidence: no repo-native markdown or doc-only verification task currently exists in Makefile.toml, so validation for this lane is limited to contract validation, diff hygiene, and readback of the updated routing references.",
      "Post-closeout evidence: XY-108 was synced to Linear Done after downstream implementation tickets and repo-native performance entrypoints were completed."
    ],
    "last_updated": "2026-03-14T20:41:08Z",
    "replan_reason": null,
    "context_snapshot": {
      "issue_id": "XY-108",
      "user_requested_issue_by_issue_execution": true,
      "user_requested_plan_writing_then_executing": true,
      "linear_issue_state": "Done",
      "follow_up_issue_for_scheduler_alignment": "XY-115"
    }
  }
}
```
