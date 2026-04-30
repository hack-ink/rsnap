import os
import re
import statistics
import struct
import sys
import time
from datetime import datetime


def threshold(name: str, default: float) -> float:
    raw = os.environ.get(name)
    if raw is None or raw == "":
        return default
    return float(raw)


def mode_threshold(name: str, default: float) -> float:
    mode_name = f"{expected_mode.upper()}_{name}"
    raw = os.environ.get(mode_name)
    if raw is not None and raw != "":
        return float(raw)
    return threshold(name, default)


def fields(line: str) -> dict[str, str]:
    parsed = {}
    for token in line.split():
        if "=" not in token:
            continue
        key, value = token.split("=", 1)
        parsed[key] = value.strip('"')
    return parsed


def bool_field(values: dict[str, str], key: str) -> bool:
    return values.get(key, "").lower() == "true"


def float_field(values: dict[str, str], key: str) -> float:
    return float(values.get(key, "0"))


def int_field(values: dict[str, str], key: str) -> int:
    return int(values.get(key, "0"))


def png_dimensions(path: str) -> tuple[int, int] | None:
    with open(path, "rb") as handle:
        header = handle.read(24)
    if len(header) < 24 or header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
        return None
    return struct.unpack(">II", header[16:24])


def line_epoch(line: str) -> float | None:
    try:
        parsed = datetime.strptime(line[:23], "%Y-%m-%d %H:%M:%S.%f")
    except ValueError:
        return None
    return time.mktime(parsed.timetuple()) + parsed.microsecond / 1_000_000


if len(sys.argv) != 2:
    print("usage: native-visual-contract-summary.py <all.log>", file=sys.stderr)
    sys.exit(2)

log_path = sys.argv[1]
expected_mode = os.environ.get("EXPECTED_HUD_GLASS_MODE", "liquid").strip().lower()
screenshot_path = os.environ.get("VISUAL_SCREENSHOT_PATH", "").strip()
mask_probe_path = os.environ.get("MASK_PROBE_PATH", "").strip()
expected_min_freeze_commits = int(os.environ.get("EXPECTED_MIN_FREEZE_COMMITS", "1") or "1")
smoke_started_epoch = float(os.environ.get("SMOKE_STARTED_EPOCH", "0") or "0")
run_id_re = re.compile(r"runID=([^ ]+)")
event_re = re.compile(r"event=([^ ]+)")

with open(log_path, "r", encoding="utf-8", errors="replace") as handle:
    lines = [
        line
        for line in handle
        if smoke_started_epoch <= 0
        or (line_epoch(line) is not None and line_epoch(line) >= smoke_started_epoch - 1)
    ]

latest_start_run_id = None
latest_any_run_id = None
for line in lines:
    run_id_match = run_id_re.search(line)
    if not run_id_match:
        continue
    run_id = run_id_match.group(1)
    latest_any_run_id = run_id
    event_match = event_re.search(line)
    if event_match and event_match.group(1) == "capture_timing.start_capture":
        latest_start_run_id = run_id

latest_run_id = latest_start_run_id or latest_any_run_id
if latest_run_id is None:
    print("[smoke] FAIL missing native host runID", file=sys.stderr)
    sys.exit(1)

print(f"[smoke] runID {latest_run_id}")

events: dict[str, list[dict[str, str]]] = {}
for line in lines:
    run_id_match = run_id_re.search(line)
    if not run_id_match or run_id_match.group(1) != latest_run_id:
        continue
    event_match = event_re.search(line)
    if not event_match:
        continue
    events.setdefault(event_match.group(1), []).append(fields(line))

failures: list[str] = []
for required_event in [
    "capture_timing.start_capture",
    "capture_timing.freeze_commit",
    "capture_timing.frozen_first_display_handoff",
]:
    if required_event not in events:
        failures.append(f"missing event {required_event}")

freeze_commit = events.get("capture_timing.freeze_commit", [{}])[-1]
handoff = events.get("capture_timing.frozen_first_display_handoff", [{}])[-1]
freeze_commits = events.get("capture_timing.freeze_commit", [])
freeze_commit_failures = events.get("capture_timing.freeze_commit_failed", [])

if len(freeze_commits) < expected_min_freeze_commits:
    failures.append(
        "freeze commit count too small: "
        f"{len(freeze_commits)} < {expected_min_freeze_commits}"
    )
if freeze_commit_failures:
    failures.append(
        "freeze commit failure events observed: "
        f"{len(freeze_commit_failures)}"
    )

if freeze_commit:
    total_ms = float_field(freeze_commit, "totalMs")
    present_ms = float_field(freeze_commit, "presentMs")
    snapshot_wait_ms = float_field(freeze_commit, "snapshotWaitMs")
    base_ready = bool_field(freeze_commit, "baseReady")
    snapshot_source = freeze_commit.get("snapshotSource", "unknown")
    max_total_ms = threshold("MAX_FREEZE_COMMIT_MS", 90.0)
    max_present_ms = mode_threshold(
        "MAX_FREEZE_PRESENT_MS",
        55.0 if expected_mode == "classic" else 35.0,
    )
    max_snapshot_wait_ms = threshold("MAX_FREEZE_SNAPSHOT_WAIT_MS", 45.0)
    print(
        "[smoke] freeze_commit "
        f"totalMs={total_ms:.2f} presentMs={present_ms:.2f} "
        f"snapshotWaitMs={snapshot_wait_ms:.2f} snapshotSource={snapshot_source} "
        f"baseReady={base_ready}"
    )
    if total_ms > max_total_ms:
        failures.append(f"freeze_commit totalMs={total_ms:.2f} exceeds {max_total_ms:.2f}")
    if present_ms > max_present_ms:
        failures.append(f"freeze_commit presentMs={present_ms:.2f} exceeds {max_present_ms:.2f}")
    if snapshot_wait_ms > max_snapshot_wait_ms:
        failures.append(
            f"freeze_commit snapshotWaitMs={snapshot_wait_ms:.2f} "
            f"exceeds {max_snapshot_wait_ms:.2f}"
        )
    if snapshot_source == "window_list_below_overlay":
        failures.append("freeze_commit used synchronous window-list fallback for frozen handoff")
    if not base_ready:
        failures.append("freeze_commit did not prepare the frozen base image")

if handoff:
    total_ms = float_field(handoff, "totalMs")
    material_ms = float_field(handoff, "materialMs")
    live_renderer_stop_ms = float_field(handoff, "liveRendererStopMs")
    display_ms = float_field(handoff, "displayMs")
    toolbar_visible = bool_field(handoff, "toolbarVisible")
    toolbar_item_count = int_field(handoff, "toolbarItemCount")
    uses_liquid = bool_field(handoff, "usesLiquidHudGlass")
    uses_classic = bool_field(handoff, "usesClassicHudGlass")
    liquid_available = bool_field(handoff, "liquidGlassAvailable")
    liquid_toolbar_visible = bool_field(handoff, "frozenToolbarLiquidGlassVisible")
    liquid_toolbar_content_drawn = bool_field(
        handoff, "frozenToolbarLiquidGlassContentDrawn"
    )
    pending_frame_displayed = bool_field(handoff, "pendingFrameDisplayed")
    max_handoff_ms = mode_threshold(
        "MAX_FROZEN_HANDOFF_MS",
        55.0 if expected_mode == "classic" else 35.0,
    )
    print(
        "[smoke] frozen_handoff "
        f"totalMs={total_ms:.2f} materialMs={material_ms:.2f} "
        f"liveRendererStopMs={live_renderer_stop_ms:.2f} displayMs={display_ms:.2f} "
        f"toolbarVisible={toolbar_visible} "
        f"toolbarItemCount={toolbar_item_count} usesLiquidHudGlass={uses_liquid} "
        f"usesClassicHudGlass={uses_classic} liquidGlassAvailable={liquid_available} "
        f"frozenToolbarLiquidGlassVisible={liquid_toolbar_visible} "
        f"frozenToolbarLiquidGlassContentDrawn={liquid_toolbar_content_drawn} "
        f"pendingFrameDisplayed={pending_frame_displayed}"
    )
    if total_ms > max_handoff_ms:
        failures.append(f"frozen_handoff totalMs={total_ms:.2f} exceeds {max_handoff_ms:.2f}")
    if not toolbar_visible:
        failures.append("frozen handoff did not make the toolbar visible")
    if toolbar_item_count < 10:
        failures.append(f"frozen toolbar item count too small: {toolbar_item_count}")
    if pending_frame_displayed:
        failures.append("live-to-frozen handoff displayed a pending frame before the full frozen UI")
    if expected_mode == "liquid" and liquid_available:
        if not uses_liquid:
            failures.append("Liquid Glass is available but frozen handoff did not use it")
        if not liquid_toolbar_visible:
            failures.append("Liquid Glass toolbar view was not visible after frozen handoff")
        if not liquid_toolbar_content_drawn:
            failures.append("Liquid Glass toolbar content did not draw after frozen handoff")
    elif expected_mode == "liquid" and not uses_classic:
        failures.append("Liquid Glass unavailable but classic glass fallback was not active")
    elif expected_mode == "classic":
        if not uses_classic:
            failures.append("Classic Glass mode did not activate the classic blur contract")
        if uses_liquid or liquid_toolbar_visible:
            failures.append("Classic Glass mode unexpectedly used Liquid Glass toolbar chrome")
    elif expected_mode not in {"liquid", "classic"}:
        failures.append(f"unknown EXPECTED_HUD_GLASS_MODE={expected_mode}")

if screenshot_path:
    try:
        size = os.path.getsize(screenshot_path)
        dimensions = png_dimensions(screenshot_path)
    except OSError as exc:
        failures.append(f"visual screenshot missing: {exc}")
    else:
        if dimensions is None:
            failures.append(f"visual screenshot is not a PNG: {screenshot_path}")
        else:
            width, height = dimensions
            print(
                "[smoke] visual_screenshot "
                f"path={screenshot_path} width={width} height={height} bytes={size}"
            )
            if width < 500 or height < 400:
                failures.append(f"visual screenshot too small: {width}x{height}")
            if size < 20_000:
                failures.append(f"visual screenshot suspiciously small: {size} bytes")

if mask_probe_path:
    try:
        with open(mask_probe_path, "r", encoding="utf-8", errors="replace") as handle:
            rows = [line.strip().split(",") for line in handle if line.strip()]
    except OSError as exc:
        failures.append(f"mask probe missing: {exc}")
    else:
        values: dict[str, list[float]] = {"dragging": [], "released": []}
        for row in rows[1:]:
            if len(row) != 3 or row[0] not in values:
                continue
            try:
                values[row[0]].append(float(row[2]))
            except ValueError:
                continue
        dragging = values["dragging"]
        released = values["released"]
        if len(dragging) < 5 or len(released) < 5:
            failures.append(
                f"mask probe sample count too small: dragging={len(dragging)} released={len(released)}"
            )
        else:
            stable_dragging = dragging[len(dragging) // 3 :]
            baseline = statistics.median(stable_dragging)
            released_min = min(released)
            released_max = max(released)
            rise = released_max - baseline
            drop = baseline - released_min
            ratio = released_max / max(0.05, baseline)
            drop_ratio = baseline / max(0.05, released_min)
            max_rise = threshold("MAX_MASK_LUMINANCE_RISE", 0.12)
            max_drop = threshold("MAX_MASK_LUMINANCE_DROP", 0.12)
            max_ratio = threshold("MAX_MASK_LUMINANCE_RATIO", 1.35)
            print(
                "[smoke] mask_probe "
                f"path={mask_probe_path} baseline={baseline:.4f} "
                f"releasedMin={released_min:.4f} releasedMax={released_max:.4f} "
                f"drop={drop:.4f} rise={rise:.4f} "
                f"dropRatio={drop_ratio:.2f} riseRatio={ratio:.2f} "
                f"draggingSamples={len(dragging)} releasedSamples={len(released)}"
            )
            if rise > max_rise and ratio > max_ratio:
                failures.append(
                    "live-to-frozen handoff let the outside-selection scrim brighten "
                    f"(rise={rise:.4f}, ratio={ratio:.2f})"
                )
            if drop > max_drop and drop_ratio > max_ratio:
                failures.append(
                    "live-to-frozen handoff double-darkened the outside-selection scrim "
                    f"(drop={drop:.4f}, ratio={drop_ratio:.2f})"
                )

if failures:
    for failure in failures:
        print(f"[smoke] FAIL {failure}", file=sys.stderr)
    sys.exit(1)
