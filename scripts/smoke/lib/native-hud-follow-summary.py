import os
import re
import sys
from typing import Optional


def threshold(name: str, default: Optional[float]) -> Optional[float]:
    raw = os.environ.get(name)
    if raw is None or raw == "":
        return default
    return float(raw)


if len(sys.argv) != 2:
    print("usage: native-hud-follow-summary.py <all.log>", file=sys.stderr)
    sys.exit(2)

log_path = sys.argv[1]
run_id_re = re.compile(r"runID=([^ ]+)")
event_re = re.compile(r"event=([^ ]+)")
metric_re = re.compile(
    r"metric=([^ ]+) unit=([^ ]+) samples=(\d+) .*?p50=([0-9.]+) .*?p95=([0-9.]+) .*?max=([0-9.]+)"
)
metrics = {}
events = {}
input_summaries = []
refresh_targets = []
freeze_commits = []
latest_run_id = None


def fields(line: str) -> dict:
    parsed = {}
    for token in line.split():
        if "=" not in token:
            continue
        key, value = token.split("=", 1)
        parsed[key] = value.strip('"')
    return parsed


def int_field(summary: dict, key: str) -> int:
    raw = summary.get(key, "0")
    try:
        return int(raw)
    except ValueError:
        return 0


def expected_min_mouse_events() -> int:
    explicit = os.environ.get("MIN_MOUSE_EVENTS")
    if explicit not in (None, ""):
        return max(0, int(explicit))
    if os.environ.get("PATH_DRIVER", "event") != "event":
        return 0
    if os.environ.get("PATH_MODE", "smooth") != "smooth":
        return 1
    try:
        duration_ms = float(os.environ.get("PATH_DURATION_MS", "2500"))
        rate_hz = float(os.environ.get("PATH_RATE_HZ", "120"))
    except ValueError:
        return 1
    return max(1, int(duration_ms * rate_hz / 1000 * 0.5))


with open(log_path, "r", encoding="utf-8", errors="replace") as handle:
    lines = handle.readlines()

latest_any_run_id = None
latest_input_run_id = None
latest_refresh_run_id = None

for line in lines:
    run_id_match = run_id_re.search(line)
    if not run_id_match:
        continue
    run_id = run_id_match.group(1)
    latest_any_run_id = run_id
    event_match = event_re.search(line)
    if not event_match:
        continue
    event = event_match.group(1)
    if event == "live_chrome.input_summary":
        latest_input_run_id = run_id
    elif event == "live_chrome.refresh_target":
        latest_refresh_run_id = run_id

latest_run_id = latest_input_run_id or latest_refresh_run_id or latest_any_run_id

if latest_run_id is None:
    print("[smoke] FAIL missing native host runID", file=sys.stderr)
    sys.exit(1)

print(f"[smoke] runID {latest_run_id}")

for line in lines:
    run_id_match = run_id_re.search(line)
    if not run_id_match or run_id_match.group(1) != latest_run_id:
        continue
    event_match = event_re.search(line)
    if event_match:
        event = event_match.group(1)
        events[event] = events.get(event, 0) + 1
        if event == "live_chrome.input_summary":
            input_summaries.append(fields(line))
        if event == "live_chrome.refresh_target":
            refresh_targets.append(fields(line))
        if event == "capture_timing.freeze_commit":
            freeze_commits.append(fields(line))
    match = metric_re.search(line)
    if not match:
        continue
    metrics.setdefault(match.group(1), []).append(
        (
            match.group(2),
            int(match.group(3)),
            float(match.group(4)),
            float(match.group(5)),
            float(match.group(6)),
        )
    )

input_capture_id = input_summaries[-1].get("captureID") if input_summaries else None
capture_refresh_targets = [
    target for target in refresh_targets if target.get("captureID") == input_capture_id
]
if capture_refresh_targets:
    target_hz = max(1, int_field(capture_refresh_targets[-1], "targetHz"))
    latest_capture_id = input_capture_id
elif refresh_targets:
    target_hz = max(1, int_field(refresh_targets[-1], "targetHz"))
    latest_capture_id = refresh_targets[-1].get("captureID")
else:
    target_hz = 120
    latest_capture_id = None
target_budget_ms = 1_000 / target_hz
sample_target_hz = 120
display_gap_budget_ms = target_budget_ms + 1.0
sample_gap_budget_ms = (1_000 / sample_target_hz) + 1.0
print(
    f"[smoke] target {target_hz}Hz budget={target_budget_ms:.2f}ms "
    f"sample_target={sample_target_hz}Hz sample_budget={sample_gap_budget_ms:.2f}ms"
)

required = {
    "live_chrome.sample_refresh_gap": threshold(
        "MAX_SAMPLE_REFRESH_GAP_P95_MS", sample_gap_budget_ms
    ),
    "live_chrome.active_layer_chrome_render_gap": threshold(
        "MAX_ACTIVE_LAYER_CHROME_RENDER_GAP_P95_MS", display_gap_budget_ms
    ),
    "live_chrome.frame_tick_gap": threshold("MAX_FRAME_TICK_GAP_P95_MS", display_gap_budget_ms),
    "live_chrome.layer_render_duration": threshold("MAX_LAYER_RENDER_DURATION_P95_MS", None),
    "live_chrome.layer_chrome_render_duration": threshold(
        "MAX_LAYER_CHROME_RENDER_DURATION_P95_MS", target_budget_ms
    ),
}
reported = [
    "live_chrome.pointer_event_gap",
    "live_chrome.sample_refresh_gap",
    "live_chrome.sample_refresh_duration",
    "live_chrome.background_sample_duration",
    "live_chrome.update_duration",
    "live_chrome.main_queue_tick_wait",
    "live_chrome.snapshot_duration",
    "live_chrome.layer_render_duration",
    "live_chrome.layer_chrome_render_gap",
    "live_chrome.active_layer_chrome_render_gap",
    "live_chrome.layer_chrome_render_duration",
    "live_chrome.frame_tick_gap",
]

failures = []
for required_event in ["capture_timing.start_capture", "live_chrome.refresh_target"]:
    if events.get(required_event, 0) == 0:
        failures.append(f"missing event {required_event}")

if input_capture_id is not None and not capture_refresh_targets:
    failures.append(f"missing refresh target for input captureID={input_capture_id}")

if any(commit.get("captureID") == latest_capture_id for commit in freeze_commits):
    failures.append("capture transitioned to frozen during HUD-follow smoke")

if not input_summaries:
    failures.append("missing event live_chrome.input_summary")
else:
    summary = input_summaries[-1]
    mouse_events = int_field(summary, "mouseEvents")
    follow_ticks = int_field(summary, "followTicks")
    fast_attempts = int_field(summary, "fastMoveAttempts")
    fast_successes = int_field(summary, "fastMoveSuccesses")
    loupe_fast_attempts = int_field(summary, "loupeFastMoveAttempts")
    loupe_fast_successes = int_field(summary, "loupeFastMoveSuccesses")
    predicted_moves = int_field(summary, "predictedMoves")
    fallback_refreshes = int_field(summary, "fallbackRefreshes")
    immediate_refreshes = int_field(summary, "immediateRefreshes")
    min_mouse_events = expected_min_mouse_events()
    print(
        "[smoke] input summary "
        f"mouseEvents={mouse_events} followTicks={follow_ticks} "
        f"fastMoveAttempts={fast_attempts} fastMoveSuccesses={fast_successes} "
        f"loupeFastMoveAttempts={loupe_fast_attempts} "
        f"loupeFastMoveSuccesses={loupe_fast_successes} "
        f"predictedMoves={predicted_moves} "
        f"fallbackRefreshes={fallback_refreshes} "
        f"immediateRefreshes={immediate_refreshes} "
        f"minMouseEvents={min_mouse_events}"
    )
    if mouse_events < min_mouse_events:
        failures.append(
            f"live HUD smoke delivered {mouse_events} mouse movement events, "
            f"expected at least {min_mouse_events}"
        )

for name in reported:
    if name not in metrics:
        if required.get(name) is not None:
            failures.append(f"missing metric {name}")
        continue
    batches = metrics[name]
    unit = batches[-1][0]
    total_samples = sum(batch[1] for batch in batches)
    p50 = batches[-1][2]
    p95 = max(batch[3] for batch in batches)
    max_value = max(batch[4] for batch in batches)
    print(
        f"[smoke] metric {name} batches={len(batches)} samples={total_samples} "
        f"p50={p50:.2f}{unit} p95={p95:.2f}{unit} max={max_value:.2f}{unit}"
    )
    limit = required.get(name)
    if limit is not None and p95 > limit:
        failures.append(f"{name} p95={p95:.2f}{unit} exceeds {limit:.2f}{unit}")

if failures:
    for failure in failures:
        print(f"[smoke] FAIL {failure}", file=sys.stderr)
    sys.exit(1)
