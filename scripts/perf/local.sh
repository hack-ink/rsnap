#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

case "${1:-}" in
  --help|-h)
    cat <<'EOF'
Usage: local.sh

Runs the local deterministic performance sweep.
No local deterministic benchmarks are enabled while scroll capture is disabled.
EOF
    exit 0
    ;;
esac

cd "$ROOT_DIR"
echo "[perf] no local deterministic benchmarks are enabled while scroll capture is disabled."
