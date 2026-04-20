#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "${1:-}" in
  --help|-h)
    cat <<'EOF'
Usage: live-loupe-perf-self-check-macos.sh

Runs the macOS environment/tooling self-check for the live-loupe smoke harness.
EOF
    exit 0
    ;;
esac

exec "$SCRIPT_DIR/live-loupe-perf-macos.sh" --self-check
