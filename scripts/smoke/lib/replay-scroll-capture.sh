#!/usr/bin/env bash

replay_scroll_capture_smoke_dir() {
  cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd
}

replay_scroll_capture_repo_root() {
  cd -- "$(replay_scroll_capture_smoke_dir)/../.." && pwd
}

replay_scroll_capture_usage() {
  local mode="$1"

  case "$mode" in
    replay)
      cat <<'EOF'
Usage: replay-scroll-capture.sh [replay-args]

Runs the recorded live-trace replay example in worker-pairwise mode.

Common replay args:
  --trace <manifest-path>   replay a specific trace manifest
  --list                    list the available traces
  --self-check              delegate to replay-scroll-capture-self-check.sh
EOF
      ;;
    analyze)
      cat <<'EOF'
Usage: analyze-scroll-capture-trace.sh [replay-args]

Runs the recorded live-trace replay example in summary-only JSON analysis mode.

Common replay args:
  --trace <manifest-path>   analyze a specific trace manifest
  --list                    list the available traces
  --self-check              delegate to replay-scroll-capture-self-check.sh
EOF
      ;;
    *)
      printf 'unknown replay-scroll-capture mode: %s\n' "$mode" >&2
      return 2
      ;;
  esac
}

replay_scroll_capture_has_flag() {
  local needle="$1"
  shift

  local arg
  for arg in "$@"; do
    if [[ "$arg" == "$needle" ]]; then
      return 0
    fi
  done

  return 1
}

replay_scroll_capture_assert_self_check_only() {
  local arg
  for arg in "$@"; do
    if [[ "$arg" != "--self-check" ]]; then
      printf '%s\n' \
        '--self-check cannot be combined with replay args; use replay-scroll-capture-self-check.sh directly for extra test flags.' >&2
      return 2
    fi
  done

  return 0
}

replay_scroll_capture_has_trace_arg() {
  local arg
  for arg in "$@"; do
    if [[ "$arg" == "--trace" ]]; then
      return 0
    fi
  done

  return 1
}

replay_scroll_capture_default_trace_root() {
  case "$(uname -s)" in
    Darwin)
      printf '%s/Library/Application Support/ink.hack.rsnap/scroll-capture-traces\n' "$HOME"
      ;;
    *)
      printf '%s/.local/share/ink.hack.rsnap/scroll-capture-traces\n' "$HOME"
      ;;
  esac
}

replay_scroll_capture_maybe_skip_without_traces() {
  replay_scroll_capture_has_trace_arg "$@" && return 0
  replay_scroll_capture_has_flag "--list" "$@" && return 0

  local trace_root latest_manifest
  trace_root="$(replay_scroll_capture_default_trace_root)"
  latest_manifest="$(
    find "$trace_root" -mindepth 2 -maxdepth 2 -name manifest.json -print -quit 2>/dev/null || true
  )"
  if [[ -n "$latest_manifest" ]]; then
    return 0
  fi

  printf '[replay] SKIP no recorded scroll-capture traces under %s; pass --trace <manifest-path> to replay a specific trace.\n' "$trace_root"
  return 1
}

replay_scroll_capture_run() {
  local mode="$1"
  shift

  if replay_scroll_capture_has_flag "--help" "$@" || replay_scroll_capture_has_flag "-h" "$@"; then
    replay_scroll_capture_usage "$mode"
    return 0
  fi

  if replay_scroll_capture_has_flag "--self-check" "$@"; then
    replay_scroll_capture_assert_self_check_only "$@" || return $?
    exec "$(replay_scroll_capture_smoke_dir)/replay-scroll-capture-self-check.sh"
  fi

  local extra_args=()
  case "$mode" in
    replay)
      ;;
    analyze)
      extra_args=(--json --summary-only)
      ;;
    *)
      printf 'unknown replay-scroll-capture mode: %s\n' "$mode" >&2
      return 2
      ;;
  esac

  if ! replay_scroll_capture_maybe_skip_without_traces "$@"; then
    return 0
  fi

  cd "$(replay_scroll_capture_repo_root)"
  exec cargo run -p rsnap-overlay --example scroll_capture_replay -- \
    --force-worker-pairwise \
    "${extra_args[@]}" \
    "$@"
}
