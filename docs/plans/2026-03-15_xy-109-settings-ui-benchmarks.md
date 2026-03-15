```json
{
  "spec": {
    "schema": "plan/1",
    "plan_id": "xy-109-settings-ui-benchmarks",
    "goal": "Execute XY-109 by adding deterministic Criterion benchmarks for rsnap settings UI rendering on a shared CPU-side egui surface that does not depend on a live desktop session.",
    "success_criteria": [
      "A valid saved plan/1 artifact exists for XY-109 and is used as the only execution authority for this lane.",
      "The rsnap app package exposes a narrow benchmark-support surface that reuses the real settings-section rendering code instead of maintaining a benchmark-only duplicate.",
      "A Criterion benchmark target covers at least the steady-state settings UI layout path and a heavier expanded scenario for render-intensive settings content.",
      "Verification evidence shows the new benchmark target compiles and executes successfully with repo-native Rust commands."
    ],
    "constraints": [
      "Keep plan-writing and plan-execution responsibilities separate: execution may update only runtime state, not strategy or task topology.",
      "Do not introduce a benchmark path that requires opening a real winit window or GPU-backed desktop session for normal local runs.",
      "Keep the benchmark surface narrow: expose only the support needed by benches rather than broadly publishing settings-window internals.",
      "Prefer shared runtime rendering code for the benchmark path so benchmark drift cannot silently diverge from the shipping settings UI."
    ],
    "defaults": {
      "issue_id": "XY-109",
      "issue_url": "https://linear.app/hack-ink/issue/XY-109/perf-add-criterion-benchmarks-for-rsnap-settings-and-render-heavy-ui",
      "benchmark_target": "settings_window",
      "app_manifest": "apps/rsnap/Cargo.toml"
    },
    "tasks": [
      {
        "id": "task-1",
        "title": "Confirm the smallest shared benchmark surface for settings rendering",
        "status": "done",
        "objective": "Read the current settings-window code and settle the narrowest safe extraction that lets Criterion benchmark the real egui settings render path without a live window.",
        "inputs": [
          "apps/rsnap/src/settings_window.rs",
          "apps/rsnap/src/settings_window/chrome.rs",
          "apps/rsnap/src/settings_window/sections.rs",
          "apps/rsnap/src/settings_window/hotkey.rs",
          "apps/rsnap/Cargo.toml"
        ],
        "outputs": [
          "A concrete implementation cut for shared settings benchmark support."
        ],
        "verification": [
          "Confirm why the current bin-only app crate and SettingsWindow construction are not directly benchable.",
          "Identify the shared render path and state surface required by the benchmark helper."
        ],
        "depends_on": []
      },
      {
        "id": "task-2",
        "title": "Implement shared settings benchmark support",
        "status": "done",
        "objective": "Expose a narrow library-backed benchmark helper that reuses the real settings-section egui render code and supports deterministic steady-state frame runs.",
        "inputs": [
          "Evidence from task-1"
        ],
        "outputs": [
          "Updated rsnap library surface",
          "Shared settings benchmark helper and any required refactors to keep runtime and benchmark rendering in sync"
        ],
        "verification": [
          "Runtime settings-window code still calls the shared settings-section renderer.",
          "The benchmark helper can run settings render frames without constructing a real Window or GPU renderer."
        ],
        "depends_on": [
          "task-1"
        ]
      },
      {
        "id": "task-3",
        "title": "Add Criterion benchmarks for steady-state settings rendering",
        "status": "done",
        "objective": "Create the Criterion benchmark target, register representative scenarios, and keep the benchmark naming aligned with XY-108's performance-tracking contract.",
        "inputs": [
          "Shared benchmark helper from task-2",
          "apps/rsnap/Cargo.toml"
        ],
        "outputs": [
          "apps/rsnap/benches/settings_window.rs",
          "Cargo manifest updates for Criterion bench support"
        ],
        "verification": [
          "The benchmark target covers a baseline settings-render scenario and a heavier expanded settings scenario.",
          "The benchmark binary uses the shared helper instead of private ad hoc UI duplication."
        ],
        "depends_on": [
          "task-2"
        ]
      },
      {
        "id": "task-4",
        "title": "Verify the benchmark target and record execution evidence",
        "status": "done",
        "objective": "Run fresh verification for the touched rsnap bench surface, then update the saved plan state with concrete evidence and any bounded skips.",
        "inputs": [
          "Updated rsnap app crate and benchmark target",
          "docs/plans/2026-03-15_xy-109-settings-ui-benchmarks.md"
        ],
        "outputs": [
          "Plan evidence capturing compile and execution results for the new benchmark surface"
        ],
        "verification": [
          "cargo test -p rsnap --lib",
          "cargo bench -p rsnap --bench settings_window -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.1",
          "git diff --check",
          "python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-15_xy-109-settings-ui-benchmarks.md"
        ],
        "depends_on": [
          "task-3"
        ]
      }
    ],
    "replan_policy": {
      "owner": "plan-writing",
      "triggers": [
        "The benchmark surface turns out to require a wider app/library split than a narrow helper or lib target can provide.",
        "Criterion cannot execute the chosen settings UI path deterministically without a materially different harness strategy.",
        "Implementing the shared helper exposes broader settings-window architectural coupling that cannot be resolved within XY-109's scope."
      ]
    }
  },
  "state": {
    "phase": "done",
    "current_task_id": null,
    "next_task_id": null,
    "blockers": [],
    "evidence": [
      "Task 1 evidence: apps/rsnap was previously a bin-only crate, and SettingsWindow::open() required a live ActiveEventLoop plus GPU/window state, so benches could not directly import or construct the real settings UI path.",
      "Task 1 evidence: the lowest-risk shared surface was the CPU-side egui settings sections path, which already contained most of the render-heavy settings work and could be reused without opening a real Window.",
      "Task 2 outcome: added apps/rsnap/src/lib.rs and moved the binary entrypoint to consume the shared library module graph so benchmark support and the shipping app now compile against the same rsnap code paths.",
      "Task 2 outcome: refactored settings-window sections and hotkey rendering into shared free-function helpers plus a narrow benchmark harness in apps/rsnap/src/settings_window/bench_support.rs.",
      "Task 3 outcome: added Criterion benchmark target apps/rsnap/benches/settings_window.rs with steady-state layout and layout-plus-tessellation groups across default, expanded_all, and hotkey_recording scenarios.",
      "Task 4 verification: cargo make fmt-rust completed successfully.",
      "Task 4 verification: cargo test -p rsnap --lib passed with 16 tests passing and 0 failures.",
      "Task 4 verification: cargo bench -p rsnap --bench settings_window -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.1 completed successfully.",
      "Task 4 verification: benchmark output reported settings_window_layout/default at approximately 25.9 us, settings_window_layout/expanded_all at approximately 36.3 us, settings_window_frame/default at approximately 42.6 us, and settings_window_frame/expanded_all at approximately 60.7 us on this machine.",
      "Task 4 verification: git diff --check returned clean.",
      "Task 4 verification: python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-15_xy-109-settings-ui-benchmarks.md returned OK."
    ],
    "last_updated": "2026-03-15T10:10:00Z",
    "replan_reason": null,
    "context_snapshot": {
      "depends_on_issue": "XY-108",
      "issue_id": "XY-109",
      "user_requested_issue_by_issue_execution": true,
      "user_requested_plan_writing_then_executing": true,
      "benchmark_target": "settings_window"
    }
  }
}
```
