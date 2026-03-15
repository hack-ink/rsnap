```json
{
  "spec": {
    "schema": "plan/1",
    "plan_id": "xy-112-performance-entrypoints-and-runbook",
    "goal": "Execute XY-112 by adding repo-native cargo-make entrypoints for rsnap performance tracking and a durable runbook that explains which commands to use for local deterministic benchmarks versus dedicated macOS GUI smoke.",
    "success_criteria": [
      "A valid saved plan/1 artifact exists for XY-112 and is used as the only execution authority for this lane.",
      "Makefile.toml exposes repo-native performance entrypoints for the new local benchmark surfaces and for dedicated macOS smoke checks.",
      "A guide explains the performance command surface, baseline save/compare workflow, and environment expectations for local benches versus dedicated GUI smoke.",
      "README or equivalent contributor-facing docs point at the new performance command surface.",
      "Fresh verification evidence shows the new repo-native commands execute successfully for the local benchmark path and the macOS self-check path."
    ],
    "constraints": [
      "Keep plan-writing and plan-execution responsibilities separate: execution may update only runtime state, not strategy or task topology.",
      "Reuse the existing smoke tasks instead of replacing or renaming the stable low-level smoke commands.",
      "Do not turn dedicated macOS GUI smoke into a shared-runner PR gate; documentation should keep that boundary explicit.",
      "Keep baseline save/compare documentation aligned with the actual Criterion command surface used by the committed benchmark targets."
    ],
    "defaults": {
      "issue_id": "XY-112",
      "issue_url": "https://linear.app/hack-ink/issue/XY-112/perf-add-repo-native-benchmark-entrypoints-and-performance-tracking",
      "local_perf_task": "perf-local",
      "macos_self_check_task": "perf-self-check-macos",
      "macos_smoke_task": "perf-macos",
      "runbook_path": "docs/guide/performance-checks.md"
    },
    "tasks": [
      {
        "id": "task-1",
        "title": "Map the current performance command surface and documentation gaps",
        "status": "done",
        "objective": "Read the current cargo-make tasks, README, and performance docs to settle the smallest clear command surface that distinguishes local deterministic benches from dedicated macOS GUI smoke.",
        "inputs": [
          "Makefile.toml",
          "README.md",
          "docs/spec/performance_tracking.md",
          "docs/guide/scroll-capture-benchmarks.md",
          "docs/guide/live-sampling-streams.md",
          "XY-112"
        ],
        "outputs": [
          "A concrete task and documentation cut for repo-native performance entrypoints."
        ],
        "verification": [
          "Confirm which benchmark commands already exist only as direct cargo bench invocations rather than cargo-make tasks.",
          "Confirm which existing smoke tasks should be reused as stable low-level dedicated-session entrypoints."
        ],
        "depends_on": []
      },
      {
        "id": "task-2",
        "title": "Add cargo-make performance entrypoints",
        "status": "done",
        "objective": "Add repo-native performance tasks that expose the settings UI and scroll-capture benchmark surfaces plus clear dedicated macOS smoke entrypoints without obscuring the existing low-level commands.",
        "inputs": [
          "Evidence from task-1",
          "Makefile.toml"
        ],
        "outputs": [
          "Updated Makefile.toml performance tasks"
        ],
        "verification": [
          "A contributor can run one high-level task for local deterministic performance benches.",
          "A contributor can run one high-level task for macOS performance self-checks or dedicated smoke while still preserving the existing low-level smoke commands."
        ],
        "depends_on": [
          "task-1"
        ]
      },
      {
        "id": "task-3",
        "title": "Document the performance workflow and command selection",
        "status": "done",
        "objective": "Write a durable runbook and contributor-facing doc updates that explain baseline save/compare, environment requirements, and which command to run for component render regressions versus end-to-end GUI regressions.",
        "inputs": [
          "New performance tasks from task-2",
          "docs/spec/performance_tracking.md"
        ],
        "outputs": [
          "docs/guide/performance-checks.md",
          "README.md updates",
          "Any minimal doc routing updates needed for discoverability"
        ],
        "verification": [
          "The runbook explains the boundary between local benches and dedicated macOS smoke.",
          "The runbook documents Criterion baseline save/compare flow for the committed benchmark targets.",
          "Contributor-facing docs mention the new cargo-make performance commands."
        ],
        "depends_on": [
          "task-2"
        ]
      },
      {
        "id": "task-4",
        "title": "Verify the repo-native performance workflow and record evidence",
        "status": "done",
        "objective": "Run fresh verification for the new performance entrypoints and update the saved plan state with explicit evidence and any bounded skips.",
        "inputs": [
          "Updated Makefile.toml and performance docs",
          "docs/plans/2026-03-15_xy-112-performance-entrypoints-and-runbook.md"
        ],
        "outputs": [
          "Plan evidence capturing verification of the repo-native performance command surface"
        ],
        "verification": [
          "cargo make perf-local",
          "cargo make perf-self-check-macos",
          "git diff --check",
          "python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-15_xy-112-performance-entrypoints-and-runbook.md"
        ],
        "depends_on": [
          "task-3"
        ]
      }
    ],
    "replan_policy": {
      "owner": "plan-writing",
      "triggers": [
        "The best command surface requires a materially different task hierarchy than simple cargo-make aliases and composites.",
        "Documenting baseline save and compare requires task-level parameterization that cargo-make cannot express cleanly without widening scope into scripting.",
        "Verification shows the dedicated macOS self-check path must change in a broader way instead of being reused as-is."
      ]
    }
  },
  "state": {
    "phase": "done",
    "current_task_id": null,
    "next_task_id": null,
    "blockers": [],
    "evidence": [
      "Task 1 evidence: Makefile.toml previously exposed only the dedicated macOS smoke tasks; the settings-window and scroll-capture benchmark surfaces existed only as direct cargo bench commands rather than repo-native cargo-make entrypoints.",
      "Task 1 evidence: README documented smoke-self-check-macos and smoke-macos, so the cleanest command-surface change was to preserve those low-level smoke commands and add a higher-level performance layer above them.",
      "Task 2 outcome: Makefile.toml now exposes perf-bench-settings-window, perf-bench-scroll-capture, perf-local, perf-self-check-macos, and perf-macos so contributors can choose deterministic local benches or dedicated macOS smoke without rediscovering raw commands.",
      "Task 2 outcome: the new perf-self-check-macos and perf-macos tasks reuse the existing smoke-* tasks instead of renaming or replacing the stable low-level smoke entrypoints.",
      "Task 3 outcome: docs/guide/performance-checks.md now documents command selection, environment expectations, and Criterion save-baseline or baseline comparison flow for the committed benchmark targets.",
      "Task 3 outcome: README.md now points contributors at cargo make perf-local, cargo make perf-self-check-macos, and cargo make perf-macos, while docs/spec/performance_tracking.md now routes operators to the performance-checks guide for procedures.",
      "Task 4 verification: cargo make fmt-toml completed successfully.",
      "Task 4 verification: cargo make perf-local completed successfully and executed both committed Criterion benchmark targets through the new repo-native command surface.",
      "Task 4 verification: cargo make perf-self-check-macos completed successfully, executed both local benchmark targets, and both smoke self-check scripts returned '[smoke] self-check ok'.",
      "Task 4 verification: git diff --check returned clean.",
      "Task 4 verification: python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-15_xy-112-performance-entrypoints-and-runbook.md returned OK."
    ],
    "last_updated": "2026-03-14T20:39:46Z",
    "replan_reason": null,
    "context_snapshot": {
      "depends_on_issue": "XY-108",
      "issue_id": "XY-112",
      "user_requested_issue_by_issue_execution": true,
      "user_requested_plan_writing_then_executing": true,
      "local_perf_task": "perf-local"
    }
  }
}
```
