#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "${1:-}" in
  --help|-h)
    cat <<'EOF'
Usage: macos.sh

Runs the macOS smoke sequence:
  1. native-host visual/behavior contract smoke
  2. recorded live-trace replay in worker-pairwise mode, or replay self-check when no trace exists
EOF
    exit 0
    ;;
esac

"$SCRIPT_DIR/native-visual-contract-macos.sh"

TRACE_ROOT="${RSNAP_SCROLL_CAPTURE_TRACE_DIR:-$HOME/Library/Application Support/ink.hack.rsnap/scroll-capture-traces}"
if [[ -d "$TRACE_ROOT" ]] && find "$TRACE_ROOT" -mindepth 2 -maxdepth 2 -name manifest.json -print -quit | grep -q .; then
  "$SCRIPT_DIR/replay-scroll-capture.sh"
else
  echo "[smoke] no recorded scroll-capture trace found; running replay self-check"
  "$SCRIPT_DIR/replay-scroll-capture-self-check.sh"
fi
