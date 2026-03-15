```json
{
  "spec": {
    "schema": "plan/1",
    "plan_id": "xy-111-scroll-capture-benchmarks",
    "goal": "Execute XY-111 by adding deterministic non-GUI benchmarks for rsnap-overlay scroll-capture stitching and image-processing hot paths on fixed synthetic fixtures, plus the minimum fixture/baseline documentation needed to run them repeatedly.",
    "success_criteria": [
      "A valid saved plan/1 artifact exists for XY-111 and is used as the only execution authority for this lane.",
      "The rsnap-overlay crate exposes a narrow benchmark-support surface that reuses the shipping scroll-capture logic on deterministic synthetic fixtures instead of benchmark-only duplicate algorithms.",
      "A Criterion benchmark target covers at least one representative scroll-capture matching or stitching path and one lower-level image-processing helper path on fixed fixtures.",
      "A guide documents the committed fixture shape and the local baseline workflow for the scroll-capture benchmark target.",
      "Fresh verification evidence shows the new rsnap-overlay benchmark target compiles and executes successfully with repo-native Rust commands."
    ],
    "constraints": [
      "Keep plan-writing and plan-execution responsibilities separate: execution may update only runtime state, not strategy or task topology.",
      "Do not require a live desktop session, winit window, or GPU renderer for the scroll-capture benchmark path.",
      "Keep the benchmark-support surface narrow: publish only what bench targets need rather than broadly exposing scroll-capture internals.",
      "Prefer code-generated synthetic fixtures checked into the crate over external binary assets so the fixture contract stays easy to inspect and refresh."
    ],
    "defaults": {
      "issue_id": "XY-111",
      "issue_url": "https://linear.app/hack-ink/issue/XY-111/perf-benchmark-scroll-capture-stitching-and-image-processing-hot-paths",
      "benchmark_target": "scroll_capture",
      "overlay_manifest": "packages/rsnap-overlay/Cargo.toml",
      "guide_path": "docs/guide/scroll-capture-benchmarks.md"
    },
    "tasks": [
      {
        "id": "task-1",
        "title": "Confirm the stable scroll-capture benchmark surface and fixture contract",
        "status": "done",
        "objective": "Read the existing scroll-capture implementation and tests, then settle the narrowest reusable benchmark surface and fixture style that exercises real matching and stitching work without involving GUI runtime state.",
        "inputs": [
          "packages/rsnap-overlay/src/scroll_capture.rs",
          "packages/rsnap-overlay/src/overlay.rs",
          "packages/rsnap-overlay/Cargo.toml",
          "docs/spec/performance_tracking.md"
        ],
        "outputs": [
          "A concrete implementation cut for shared scroll-capture benchmark support and the fixtures it will expose."
        ],
        "verification": [
          "Confirm which existing synthetic test fixtures already model scroll-capture growth and overlap matching well enough for a deterministic benchmark seed.",
          "Identify at least one representative end-to-end scroll-session hot path and one lower-level helper path to benchmark."
        ],
        "depends_on": []
      },
      {
        "id": "task-2",
        "title": "Implement shared rsnap-overlay benchmark support",
        "status": "done",
        "objective": "Add a narrow library-backed benchmark helper that constructs deterministic scroll-capture fixtures and drives the shipping ScrollSession and image-helper code without duplicating the algorithm outside the crate.",
        "inputs": [
          "Evidence from task-1"
        ],
        "outputs": [
          "Updated rsnap-overlay library surface",
          "Shared benchmark-support helper and any minimal scroll-capture visibility adjustments needed for benches"
        ],
        "verification": [
          "The benchmark helper reuses scroll-capture runtime code rather than an ad hoc reimplementation.",
          "The helper can produce repeatable fixture frames and run representative scroll-capture work without GUI initialization."
        ],
        "depends_on": [
          "task-1"
        ]
      },
      {
        "id": "task-3",
        "title": "Add Criterion benchmarks and fixture workflow documentation",
        "status": "done",
        "objective": "Register the deterministic rsnap-overlay benchmark target, wire representative scenarios through Criterion, and document the fixture shape plus local baseline workflow.",
        "inputs": [
          "Shared benchmark helper from task-2",
          "packages/rsnap-overlay/Cargo.toml"
        ],
        "outputs": [
          "packages/rsnap-overlay/benches/scroll_capture.rs",
          "Cargo manifest updates for Criterion bench support",
          "docs/guide/scroll-capture-benchmarks.md"
        ],
        "verification": [
          "The benchmark target covers a representative scroll-capture matching or stitching path and a lower-level fingerprint or image-helper path.",
          "The guide explains the committed fixture contract and the local baseline workflow for repeated runs."
        ],
        "depends_on": [
          "task-2"
        ]
      },
      {
        "id": "task-4",
        "title": "Verify the benchmark target and record execution evidence",
        "status": "done",
        "objective": "Run fresh verification for the touched rsnap-overlay bench surface, then update the saved plan state with concrete evidence and any bounded skips.",
        "inputs": [
          "Updated rsnap-overlay crate and benchmark target",
          "docs/plans/2026-03-15_xy-111-scroll-capture-benchmarks.md"
        ],
        "outputs": [
          "Plan evidence capturing compile and execution results for the new scroll-capture benchmark surface"
        ],
        "verification": [
          "cargo make fmt-rust",
          "cargo test -p rsnap-overlay --lib",
          "cargo bench -p rsnap-overlay --bench scroll_capture -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.1",
          "git diff --check",
          "python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-15_xy-111-scroll-capture-benchmarks.md"
        ],
        "depends_on": [
          "task-3"
        ]
      }
    ],
    "replan_policy": {
      "owner": "plan-writing",
      "triggers": [
        "The scroll-capture hot path cannot be benchmarked deterministically without a materially wider crate API or a different benchmark framework.",
        "The only representative benchmarkable path requires external fixture assets or GUI initialization that would break the non-GUI constraint.",
        "Documenting the fixture and baseline workflow exposes a broader benchmark entrypoint problem that belongs in XY-112 instead of this issue."
      ]
    }
  },
  "state": {
    "phase": "done",
    "current_task_id": null,
    "next_task_id": null,
    "blockers": [],
    "evidence": [
      "Task 1 evidence: overlay.rs already contained synthetic scroll-capture helpers like make_scroll_capture_window and observe_scroll_capture_frame, and scroll_capture.rs unit tests already exercised deterministic overlap, fingerprint, and session growth behavior, so the benchmark surface could safely reuse code-generated fixtures instead of external images.",
      "Task 1 evidence: the chosen benchmark surfaces were direct fingerprint generation, direct overlap matching, and a one-step downward ScrollSession commit, which together cover a lower-level image helper path plus representative matching and stitching work without GUI runtime state.",
      "Task 2 outcome: ScrollSession::new is now available to normal crate code, and packages/rsnap-overlay/src/scroll_capture.rs now contains a narrow bench_support module that builds deterministic synthetic fixtures and drives shipping scroll-capture logic.",
      "Task 2 outcome: packages/rsnap-overlay/src/lib.rs now re-exports only the bench support surface through rsnap_overlay::bench_support instead of broadly exposing scroll-capture internals.",
      "Task 3 outcome: packages/rsnap-overlay/Cargo.toml now registers Criterion for packages/rsnap-overlay/benches/scroll_capture.rs, and the bench target covers fingerprint, overlap-match, and session-commit groups across baseline and wide scenarios.",
      "Task 3 outcome: docs/guide/scroll-capture-benchmarks.md documents the committed synthetic fixture contract plus the local save-baseline and baseline-compare workflow, and docs/spec/performance_tracking.md now routes scenario-3 operators to that guide.",
      "Task 4 verification: cargo make fmt-rust completed successfully.",
      "Task 4 verification: cargo test -p rsnap-overlay --lib passed with 117 tests passing and 0 failures.",
      "Task 4 verification: cargo bench -p rsnap-overlay --bench scroll_capture -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.1 completed successfully.",
      "Task 4 verification: benchmark output reported scroll_capture_fingerprint/baseline at approximately 12.8 us, scroll_capture_overlap_match/baseline at approximately 78.3 us, and scroll_capture_session_commit/baseline at approximately 204 us on this machine; the wide scenario reported approximately 20.4 us, 113 us, and 314 us respectively.",
      "Task 4 verification: cargo bench -p rsnap-overlay --bench scroll_capture -- --help | rg \"baseline|save-baseline\" confirmed Criterion exposes --save-baseline and --baseline options for the documented workflow.",
      "Task 4 verification: git diff --check returned clean.",
      "Task 4 verification: python3 /Users/xavier/.codex/skills/plan-writing/scripts/validate_plan_contract.py --path docs/plans/2026-03-15_xy-111-scroll-capture-benchmarks.md returned OK."
    ],
    "last_updated": "2026-03-14T20:35:00Z",
    "replan_reason": null,
    "context_snapshot": {
      "depends_on_issue": "XY-108",
      "issue_id": "XY-111",
      "user_requested_issue_by_issue_execution": true,
      "user_requested_plan_writing_then_executing": true,
      "benchmark_target": "scroll_capture"
    }
  }
}
```
