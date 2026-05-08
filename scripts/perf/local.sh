#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

case "${1:-}" in
  --help|-h)
    cat <<'EOF'
Usage: local.sh

Runs the local deterministic performance sweep.
Checks the Rust export and scroll-capture hot paths against deterministic fixtures
and conservative local budgets.
EOF
    exit 0
    ;;
esac

cd "$ROOT_DIR"
cargo run -p rsnap-perf --release --quiet
