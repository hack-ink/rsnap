```json
{
  "spec": {
    "schema": "plan/1",
    "plan_id": "xy-115-overlay-repaint-cadence-alignment",
    "goal": "Execute XY-115 by aligning overlay repaint interval derivation with the XY-108 cadence contract and verifying the logic with targeted tests.",
    "success_criteria": [
      "A valid saved plan/1 artifact exists for XY-115 and is used as the only execution authority for this lane.",
      "Overlay repaint interval derivation follows min(120 Hz, active display refresh ceiling) instead of treating 120 Hz as a floor.",
      "Targeted tests cover <=120 Hz, >120 Hz, and unknown-refresh fallback behavior.",
      "Verification evidence shows the touched overlay test surface still passes after the cadence change."
    ],
    "constraints": [
      "Do not rewrite XY-115 strategy outside plan-writing; execution may update only runtime state.",
      "Keep the change scoped to repaint cadence derivation and targeted tests unless execution exposes a tighter dependency.",
      "Preserve the XY-108 contract: cap active redraw at 120 Hz when the active display refresh ceiling is above 120 Hz.",
      "Use repo-native Rust verification for the touched overlay scope before claiming completion."
    ],
    "defaults": {
      "issue_id": "XY-115",
      "issue_url": "https://linear.app/hack-ink/issue/XY-115/perf-align-overlay-repaint-scheduling-with-the-xy-108-cadence-contract"
    },
    "tasks": [
      {
        "id": "task-1",
        "title": "Confirm current repaint derivation and choose the minimal implementation cut",
        "status": "done",
        "objective": "Read the current repaint interval logic and determine the smallest safe code change that enforces the XY-108 cadence contract.",
        "inputs": [
          "packages/rsnap-overlay/src/overlay.rs",
          "docs/spec/performance_tracking.md",
          "XY-115"
        ],
        "outputs": [
          "A concrete implementation cut for cadence derivation and test coverage."
        ],
        "verification": [
          "Read current repaint_interval_for_monitor() and confirm the contract mismatch.",
          "Read the performance contract for the required cadence semantics."
        ],
        "depends_on": []
      },
      {
        "id": "task-2",
        "title": "Implement cadence-aligned repaint derivation",
        "status": "done",
        "objective": "Modify overlay repaint interval derivation so active redraw uses min(120 Hz, active display refresh ceiling) with explicit fallback behavior.",
        "inputs": [
          "Evidence from task-1"
        ],
        "outputs": [
          "Updated overlay repaint cadence implementation"
        ],
        "verification": [
          "Code path no longer treats 120 Hz as a mandatory floor for known lower-refresh monitors.",
          "Code path caps known higher-refresh monitors at 120 Hz."
        ],
        "depends_on": [
          "task-1"
        ]
      },
      {
        "id": "task-3",
        "title": "Add targeted cadence derivation tests",
        "status": "done",
        "objective": "Add focused tests for lower-refresh, higher-refresh, and fallback repaint cadence cases so the contract is enforced by code.",
        "inputs": [
          "Updated overlay cadence implementation"
        ],
        "outputs": [
          "New or updated overlay unit tests"
        ],
        "verification": [
          "Tests cover <=120 Hz, >120 Hz, and fallback behavior."
        ],
        "depends_on": [
          "task-2"
        ]
      },
      {
        "id": "task-4",
        "title": "Verify the touched overlay test surface and record plan evidence",
        "status": "done",
        "objective": "Run targeted verification for the changed overlay scope, then update the saved plan state with concrete evidence.",
        "inputs": [
          "Updated overlay code and tests",
          "docs/plans/2026-03-15_xy-115-overlay-repaint-cadence-alignment.md"
        ],
        "outputs": [
          "Plan evidence showing implementation and test verification results"
        ],
        "verification": [
          "cargo test -p rsnap-overlay overlay:: --lib",
          "git diff --check",
          "python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-15_xy-115-overlay-repaint-cadence-alignment.md"
        ],
        "depends_on": [
          "task-3"
        ]
      }
    ],
    "replan_policy": {
      "owner": "plan-writing",
      "triggers": [
        "The XY-108 cadence contract changes again.",
        "Implementing the repaint change reveals a larger scheduler redesign instead of a localized derivation fix.",
        "Verification shows the touched scope now requires broader overlay sequencing changes."
      ]
    }
  },
  "state": {
    "phase": "done",
    "current_task_id": null,
    "next_task_id": null,
    "blockers": [],
    "evidence": [
      "Task 1 evidence: repaint_interval_for_monitor() previously applied fps.max(120), which overscheduled known lower-refresh monitors and violated the XY-108 cadence contract.",
      "Task 2 outcome: overlay repaint cadence now uses OverlaySession::interactive_repaint_fps() to cap known refresh rates at 120 Hz while preserving explicit fallback behavior.",
      "Task 2 outcome: docs/spec/performance_tracking.md was updated so the contract status reflects aligned cadence derivation rather than a still-open scheduler mismatch.",
      "Task 3 outcome: added targeted overlay tests for <=120 Hz, >120 Hz, and fallback repaint cadence cases.",
      "Task 4 verification: cargo test -p rsnap-overlay overlay:: --lib passed with 62 tests passing and 0 failures.",
      "Task 4 verification: git diff --check returned clean.",
      "Task 4 verification: python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-15_xy-115-overlay-repaint-cadence-alignment.md returned OK."
    ],
    "last_updated": "2026-03-14T20:00:00Z",
    "replan_reason": null,
    "context_snapshot": {
      "depends_on_issue": "XY-108",
      "issue_id": "XY-115",
      "user_requested_issue_by_issue_execution": true,
      "user_requested_plan_writing_then_executing": true,
      "verification_command": "cargo test -p rsnap-overlay overlay:: --lib"
    }
  }
}
```
