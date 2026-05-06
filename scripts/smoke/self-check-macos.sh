#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "${1:-}" in
  --help|-h)
    cat <<'EOF'
Usage: self-check-macos.sh

Runs the macOS smoke readiness sequence:
  1. native HUD-follow smoke environment self-check
EOF
    exit 0
    ;;
esac

"$SCRIPT_DIR/native-hud-follow-macos.sh" --self-check
