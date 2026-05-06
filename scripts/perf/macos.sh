#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "${1:-}" in
  --help|-h)
    cat <<'EOF'
Usage: macos.sh

Runs the full macOS performance sequence:
  1. local non-scroll performance checks
  2. native-host HUD-follow smoke
  3. native-host visual/behavior contract smoke
EOF
    exit 0
    ;;
esac

"$SCRIPT_DIR/local.sh"
"$SCRIPT_DIR/../smoke/native-hud-follow-macos.sh"
"$SCRIPT_DIR/../smoke/native-visual-contract-macos.sh"
