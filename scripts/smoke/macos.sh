#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "${1:-}" in
  --help|-h)
    cat <<'EOF'
Usage: macos.sh

Runs the macOS smoke sequence:
  1. native-host HUD-follow smoke
  2. recorded live-trace replay in worker-pairwise mode
EOF
    exit 0
    ;;
esac

"$SCRIPT_DIR/native-hud-follow-macos.sh"
"$SCRIPT_DIR/replay-scroll-capture.sh"
