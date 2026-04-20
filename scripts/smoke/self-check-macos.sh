#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "${1:-}" in
  --help|-h)
    cat <<'EOF'
Usage: self-check-macos.sh

Runs the macOS smoke readiness sequence:
  1. live-loupe smoke environment self-check
  2. deterministic replay self-check
EOF
    exit 0
    ;;
esac

"$SCRIPT_DIR/live-loupe-perf-self-check-macos.sh"
"$SCRIPT_DIR/replay-scroll-capture-self-check.sh"
