Pre-commit report

Preflight
- `git status --porcelain` (exit: 0)
- `git diff --stat` (exit: 0)
- `rg -n \"[\\x{4E00}-\\x{9FFF}]\" -S .` (skipped: no repository policy found requiring this gate)

Commit message
- proposed: `{"schema":"cmsg/1","type":"feat","scope":"tauri","summary":"scaffold rsnap app shell","intent":"establish tauri workspace, tray/hotkey, and capture stub","impact":"adds src-tauri backend and ui frontend scaffold; capture saves last_capture.png to cache","breaking":true,"risk":"medium","refs":[]}`
- validation: skipped: `pre-commit/scripts/validate_cmsg.py` not present in this repository
  - fallback: `python3 -c 'import json,sys; ...'` (exit: 0)

Repo gates
- Makefile.toml gate: ran
  - `cargo make lint-fix` (exit: 0)
  - `cargo make fmt` (exit: 0)
  - `cargo make test` (exit: 0)
- docs gate: skipped: docs validation command not defined in repo docs
- workflows gate: skipped: workflow verification command not defined in repo docs

