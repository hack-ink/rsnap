#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/smoke/lib/replay-scroll-capture.sh
source "$SCRIPT_DIR/lib/replay-scroll-capture.sh"

replay_scroll_capture_run replay "$@"
