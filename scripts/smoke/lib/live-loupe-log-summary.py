import re
import sys
from collections import Counter

(
    log_path,
    session_end_ts,
    max_acquire,
    max_stalls,
    max_resized,
    max_slow,
    max_live_apply_warns,
    max_live_apply_ms,
) = sys.argv[1:]
max_acquire = int(max_acquire)
max_stalls = int(max_stalls)
max_resized = int(max_resized)
max_slow = int(max_slow)
max_live_apply_warns = int(max_live_apply_warns) if max_live_apply_warns else None
max_live_apply_ms = int(max_live_apply_ms) if max_live_apply_ms else None

metrics = {
    "acquire_frame_warns": 0,
    "event_loop_stalls": 0,
    "resized_events": 0,
    "slow_op_warnings": 0,
    "live_sample_apply_latency_warns": 0,
    "max_acquire_ms": 0,
    "max_stall_ms": 0,
    "max_live_sample_apply_latency_ms": 0,
}
resize_sizes = Counter()
slow_ops = Counter()
redraw_breakdown_counts = Counter()
redraw_breakdown_max_ms = {}

redraw_breakdown_ops = {
    "overlay.hud_redraw.total",
    "overlay.hud_redraw.renderer_draw",
    "overlay.hud_redraw.request_inner_size",
    "overlay.hud_redraw.position_update",
    "overlay.loupe_redraw.tile_draw",
    "overlay.loupe_redraw.request_inner_size",
    "overlay.loupe_redraw.reposition",
    "overlay.hud_window_set_outer_position",
    "overlay.loupe_window_set_outer_position",
}

re_elapsed = re.compile(r'elapsed_ms=(\d+)')
re_stall = re.compile(r'stall_ms=(\d+)')
re_latency = re.compile(r'latency_ms=(\d+)')
re_resize = re.compile(r'PhysicalSize \{ width: (\d+), height: (\d+) \}')
re_op = re.compile(r'op="([^"]+)"')
session_start_idx = None
session_end_idx = None
end_markers = (
    "Capture cancelled.",
    "Capture copied to clipboard.",
    "Capture saved to file.",
    "Capture failed.",
    "Capture overlay ended.",
)

with open(log_path, "r", encoding="utf-8", errors="replace") as handle:
    lines = handle.readlines()

for idx, line in enumerate(lines):
    if session_start_idx is None and "Capture overlay started." in line:
        session_start_idx = idx
        continue
    if session_start_idx is not None and any(marker in line for marker in end_markers):
        session_end_idx = idx
        break

if session_start_idx is None:
    print("[smoke] FAIL missing live session start marker", file=sys.stderr)
    sys.exit(1)

window = []
for line in lines[session_start_idx:session_end_idx]:
    if not session_end_ts:
        window.append(line)
        continue

    timestamp = line.split(" ", 1)[0]
    if timestamp > session_end_ts:
        break
    window.append(line)

if not window:
    window = lines[session_start_idx:]

for line in window:
    if 'op="overlay.window_renderer_acquire_frame"' in line:
        metrics["acquire_frame_warns"] += 1
        match = re_elapsed.search(line)
        if match:
            metrics["max_acquire_ms"] = max(metrics["max_acquire_ms"], int(match.group(1)))
    if 'op="overlay.event_loop_stall"' in line:
        metrics["event_loop_stalls"] += 1
        match = re_stall.search(line)
        if match:
            metrics["max_stall_ms"] = max(metrics["max_stall_ms"], int(match.group(1)))
    if 'op="overlay.live_sample_apply_latency"' in line:
        metrics["live_sample_apply_latency_warns"] += 1
        match = re_latency.search(line)
        if match:
            metrics["max_live_sample_apply_latency_ms"] = max(
                metrics["max_live_sample_apply_latency_ms"],
                int(match.group(1)),
            )
    if "WindowEvent::Resized" in line:
        metrics["resized_events"] += 1
        match = re_resize.search(line)
        if match:
            resize_sizes[f'{match.group(1)}x{match.group(2)}'] += 1
    if "Slow operation detected" in line:
        metrics["slow_op_warnings"] += 1
        match = re_op.search(line)
        if match:
            op = match.group(1)
            slow_ops[op] += 1
            if op in redraw_breakdown_ops:
                redraw_breakdown_counts[op] += 1
                elapsed_match = re_elapsed.search(line)
                if elapsed_match:
                    redraw_breakdown_max_ms[op] = max(
                        redraw_breakdown_max_ms.get(op, 0),
                        int(elapsed_match.group(1)),
                    )

top_sizes = ",".join(f"{size}:{count}" for size, count in resize_sizes.most_common(5)) or "none"
top_slow_ops = ",".join(f"{op}:{count}" for op, count in slow_ops.most_common(5)) or "none"
redraw_breakdown = ",".join(
    f"{op}:max_ms={redraw_breakdown_max_ms.get(op, 0)}:count={redraw_breakdown_counts[op]}"
    for op in sorted(
        redraw_breakdown_counts,
        key=lambda op: (redraw_breakdown_max_ms.get(op, 0), redraw_breakdown_counts[op], op),
        reverse=True,
    )[:10]
) or "none"
session_end_label = (
    "script_path_end"
    if session_end_idx is None or (
        session_end_ts and lines[session_end_idx].split(" ", 1)[0] > session_end_ts
    )
    else end_markers[next(i for i, marker in enumerate(end_markers) if marker in lines[session_end_idx])]
)

print(
    "[smoke] metrics "
    f'session_lines={len(window)} '
    f'session_end="{session_end_label}" '
    f'acquire_frame_warns={metrics["acquire_frame_warns"]} '
    f'max_acquire_ms={metrics["max_acquire_ms"]} '
    f'event_loop_stalls={metrics["event_loop_stalls"]} '
    f'max_stall_ms={metrics["max_stall_ms"]} '
    f'resized_events={metrics["resized_events"]} '
    f'slow_op_warnings={metrics["slow_op_warnings"]} '
    f'live_sample_apply_latency_warns={metrics["live_sample_apply_latency_warns"]} '
    f'max_live_sample_apply_latency_ms={metrics["max_live_sample_apply_latency_ms"]}'
)
print(f"[smoke] top_resize_sizes {top_sizes}")
print(f"[smoke] top_slow_ops {top_slow_ops}")
print(f"[smoke] redraw_breakdown {redraw_breakdown}")

failures = []
if metrics["acquire_frame_warns"] > max_acquire:
    failures.append(f'acquire_frame_warns={metrics["acquire_frame_warns"]} exceeds {max_acquire}')
if metrics["event_loop_stalls"] > max_stalls:
    failures.append(f'event_loop_stalls={metrics["event_loop_stalls"]} exceeds {max_stalls}')
if metrics["resized_events"] > max_resized:
    failures.append(f'resized_events={metrics["resized_events"]} exceeds {max_resized}')
if metrics["slow_op_warnings"] > max_slow:
    failures.append(f'slow_op_warnings={metrics["slow_op_warnings"]} exceeds {max_slow}')
if (
    max_live_apply_warns is not None
    and metrics["live_sample_apply_latency_warns"] > max_live_apply_warns
):
    failures.append(
        "live_sample_apply_latency_warns="
        f'{metrics["live_sample_apply_latency_warns"]} exceeds {max_live_apply_warns}'
    )
if (
    max_live_apply_ms is not None
    and metrics["max_live_sample_apply_latency_ms"] > max_live_apply_ms
):
    failures.append(
        "max_live_sample_apply_latency_ms="
        f'{metrics["max_live_sample_apply_latency_ms"]} exceeds {max_live_apply_ms}'
    )

if failures:
    for item in failures:
        print(f"[smoke] FAIL {item}", file=sys.stderr)
    sys.exit(1)
