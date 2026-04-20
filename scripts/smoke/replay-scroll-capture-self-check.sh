#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

usage() {
  cat <<'EOF'
Usage: replay-scroll-capture-self-check.sh

Runs the deterministic replay self-check test without requiring a recorded user trace.
EOF
}

case "${1:-}" in
  --help|-h)
    usage
    exit 0
    ;;
esac

cd "$ROOT_DIR"
exec cargo test -p rsnap-overlay replay_recorded_live_trace_round_trips_one_commit --lib "$@"
