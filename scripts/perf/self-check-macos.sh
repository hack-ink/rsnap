#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "${1:-}" in
  --help|-h)
    cat <<'EOF'
Usage: self-check-macos.sh

Runs the macOS performance readiness sequence:
  1. local deterministic benchmarks
  2. macOS smoke self-check
  3. deterministic replay self-check
EOF
    exit 0
    ;;
esac

"$SCRIPT_DIR/local.sh"
"$SCRIPT_DIR/../smoke/self-check-macos.sh"
