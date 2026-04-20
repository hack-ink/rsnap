#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

usage() {
  cat <<'EOF'
Usage: replay-scroll-capture.sh [replay-args]

Runs the recorded live-trace replay example in worker-pairwise mode.

Common replay args:
  --trace <manifest-path>   replay a specific trace manifest
  --list                    list the available traces
  --self-check              run the example's internal self-check mode
EOF
}

case "${1:-}" in
  --help|-h)
    usage
    exit 0
    ;;
esac

cd "$ROOT_DIR"
exec cargo run -p rsnap-overlay --example scroll_capture_replay -- --force-worker-pairwise "$@"
