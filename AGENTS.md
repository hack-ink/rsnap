# AGENTS.md — Repository-Specific Rules for Automated Agents

These instructions define repository-specific execution rules and scope limits for this repository.

---

## 1. Execution Model

## 1.1 Workspace Automation (cargo make)

- `Makefile.toml` is the source of truth for generic repo gates: `fmt`, `lint`, `test`, and `checks`.
- Run `cargo make` from the repository root when you need one of those generic repo gates.
- Smoke and performance validation entrypoints live under `scripts/smoke/` and `scripts/perf/`; run those scripts directly instead of adding them back to `Makefile.toml`.
- When task details are needed, inspect `Makefile.toml` for generic gates and the relevant script or runbook for smoke/perf flows.
