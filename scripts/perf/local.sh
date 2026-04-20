#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

case "${1:-}" in
  --help|-h)
    cat <<'EOF'
Usage: local.sh

Runs the local deterministic performance sweep:
  1. settings-window benchmark
  2. scroll-capture benchmark
EOF
    exit 0
    ;;
esac

cd "$ROOT_DIR"
cargo bench -p rsnap --bench settings_window -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.1
cargo bench -p rsnap-overlay --bench scroll_capture -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.1
