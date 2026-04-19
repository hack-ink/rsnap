+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "maestro:manual-only"
needs_attention_label = "maestro:needs-attention"

[agent]
transport = "stdio://"
personality = "pragmatic"

[execution]
max_attempts = 3
max_turns = 3
max_retry_backoff_ms = 300000
max_concurrent_agents = 1
max_concurrent_agents_by_state = { "In Progress" = 1 }
canonicalize_commands = [
  "cargo make fmt",
  "cargo make lint-fix",
]
verify_commands = [
  "cargo make checks",
]

[context]
read_first = [
  "docs/reference/workspace-layout.md",
  "docs/policy.md",
]
+++
Use `cargo make` whenever an equivalent task exists.

Use single-line `maestro/commit/1` JSON commit messages for local commits. Commit messages describe the tree change only; do not encode landing, CI, or closeout state.

Child-run execution policy inherits from the Codex runtime. Do not add repo-local sandbox or approval policy overrides.

Use the issue-scoped tracker tools autonomously for normal-path state changes and comments on the currently leased issue.

Automatic intake is driven by the service-scoped Linear label `maestro:queued:<service-id>` derived from `maestro.toml` `service_id`. Repository policy in this file decides how eligible queued issues move through the lane; it does not select a Linear project.

Treat `In Review` as a PR-backed handoff state. A normal success path must push the lane branch, create or update a non-draft PR, and only then ask `maestro` to complete the `In Review` handoff.

Before any push that refreshes a PR head, review handoff, review repair, or landing-related sync, run the default repo gate from `[execution]` in this file. For this repository that means `cargo make fmt`, `cargo make lint-fix`, and then `cargo make checks`.

Keep changes scoped to the current issue. Do not widen scope into unrelated cleanup or parallel feature work.

Use the workspace layout reference to keep app-shell changes in `apps/rsnap/` and overlay or capture-session changes in `packages/rsnap-overlay/`. Treat `.worktrees/`, `.workspaces/`, `target/`, and `.codex/` as local environment noise unless the task explicitly concerns them.

Route documentation updates by class: behavioral or schema changes go in `docs/spec/`, operational procedures go in `docs/runbook/`, current layout or implementation references go in `docs/reference/`, and durable rationale goes in `docs/decisions/`. Keep one authoritative document per topic and link instead of duplicating normative text.

When capture-session behavior, scroll-capture behavior, or performance contracts change, update the relevant docs in the same lane. The usual authority is `docs/spec/capture-session.md`, `docs/spec/performance.md`, and `docs/runbook/performance-validation.md`.

Use deterministic validation first. Reach for `cargo make replay-scroll-capture`, `cargo make analyze-scroll-capture-trace`, or the macOS smoke and perf tasks only when the changed surface actually needs that evidence; do not treat dedicated live macOS smoke as a default PR gate.

Do not claim work is complete, fixed, or passing without fresh verification evidence from the selected repo gate or another command that directly proves the claim.

Use Linear as the internal execution tracker of record for this repository. Do not mirror routine internal work into GitHub issues unless the user explicitly asks for public tracking.
