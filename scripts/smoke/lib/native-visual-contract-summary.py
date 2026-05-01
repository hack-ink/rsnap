import os
import re
import statistics
import struct
import sys
import time
import zlib
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


def png_rgb_rows(path: str) -> tuple[int, int, int, list[bytes]] | None:
    with open(path, "rb") as handle:
        data = handle.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        return None

    offset = 8
    width = height = channels = 0
    idat = bytearray()
    while offset + 8 <= len(data):
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        kind = data[offset + 4 : offset + 8]
        payload = data[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            width, height = struct.unpack(">II", payload[:8])
            bit_depth = payload[8]
            color_type = payload[9]
            interlace = payload[12]
            if bit_depth != 8 or interlace != 0 or color_type not in {2, 6}:
                return None
            channels = 4 if color_type == 6 else 3
        elif kind == b"IDAT":
            idat.extend(payload)
        elif kind == b"IEND":
            break

    if width <= 0 or height <= 0 or channels <= 0:
        return None

    raw = zlib.decompress(bytes(idat))
    stride = width * channels
    rows: list[bytes] = []
    previous = bytearray(stride)
    cursor = 0
    for _ in range(height):
        filter_type = raw[cursor]
        cursor += 1
        row = bytearray(raw[cursor : cursor + stride])
        cursor += stride
        for index in range(stride):
            left = row[index - channels] if index >= channels else 0
            up = previous[index]
            up_left = previous[index - channels] if index >= channels else 0
            if filter_type == 1:
                row[index] = (row[index] + left) & 0xFF
            elif filter_type == 2:
                row[index] = (row[index] + up) & 0xFF
            elif filter_type == 3:
                row[index] = (row[index] + ((left + up) // 2)) & 0xFF
            elif filter_type == 4:
                row[index] = (row[index] + paeth(left, up, up_left)) & 0xFF
            elif filter_type != 0:
                return None
        rows.append(bytes(row))
        previous = row
    return width, height, channels, rows


def paeth(left: int, up: int, up_left: int) -> int:
    estimate = left + up - up_left
    left_distance = abs(estimate - left)
    up_distance = abs(estimate - up)
    up_left_distance = abs(estimate - up_left)
    if left_distance <= up_distance and left_distance <= up_left_distance:
        return left
    if up_distance <= up_left_distance:
        return up
    return up_left


def count_leaked_drag_border(
    path: str, display_bounds: str, drag_points: str
) -> tuple[float, int] | None:
    decoded = png_rgb_rows(path)
    if decoded is None:
        return None
    width, height, channels, rows = decoded
    left, top, right, bottom = map(float, display_bounds.replace(" ", "").split(","))
    start_raw, end_raw = drag_points.split(";")[:2]
    sx, sy = map(float, start_raw.split(","))
    ex, ey = map(float, end_raw.split(","))
    scale_x = width / max(1.0, right - left)
    scale_y = height / max(1.0, bottom - top)
    min_x = int((min(sx, ex) - left) * scale_x)
    max_x = int((max(sx, ex) - left) * scale_x)
    min_y = int((min(sy, ey) - top) * scale_y)
    max_y = int((max(sy, ey) - top) * scale_y)
    margin = max(12, int(10 * max(scale_x, scale_y)))
    spans = [
        (0, max(0, min_x - margin)),
        (min(width, max_x + margin), width),
    ]
    row_indexes = [
        y
        for edge_y in (min_y, max_y)
        for y in range(max(0, edge_y - 2), min(height, edge_y + 3))
    ]

    border_pixels = 0
    sampled = 0
    for y in row_indexes:
        row = rows[y]
        for span_left, span_right in spans:
            for x in range(span_left, span_right):
                sampled += 1
                base = x * channels
                red, green, blue = row[base], row[base + 1], row[base + 2]
                if blue >= 150 and green >= 140 and red >= 100 and blue - red >= 20:
                    border_pixels += 1
    if sampled == 0:
        return 0.0, 0
    return border_pixels / sampled, border_pixels


def count_leaked_horizontal_seam(
    path: str, display_bounds: str, drag_points: str
) -> tuple[float, int] | None:
    decoded = png_rgb_rows(path)
    if decoded is None:
        return None
    width, height, channels, rows = decoded
    left, top, right, bottom = map(float, display_bounds.replace(" ", "").split(","))
    start_raw, end_raw = drag_points.split(";")[:2]
    sx, sy = map(float, start_raw.split(","))
    ex, ey = map(float, end_raw.split(","))
    scale_x = width / max(1.0, right - left)
    scale_y = height / max(1.0, bottom - top)
    min_x = int((min(sx, ex) - left) * scale_x)
    max_x = int((max(sx, ex) - left) * scale_x)
    min_y = int((min(sy, ey) - top) * scale_y)
    max_y = int((max(sy, ey) - top) * scale_y)
    margin = max(12, int(10 * max(scale_x, scale_y)))
    spans = [
        (0, max(0, min_x - margin)),
        (min(width, max_x + margin), width),
    ]
    edge_specs = [(min_y, -1), (max_y, 1)]

    seam_pixels = 0
    sampled = 0
    for edge_y, outside_direction in edge_specs:
        seam_rows = list(range(max(0, edge_y - 1), min(height, edge_y + 2)))
        baseline_rows = [
            edge_y + outside_direction * distance
            for distance in range(6, 13)
            if 0 <= edge_y + outside_direction * distance < height
        ]
        if not seam_rows or not baseline_rows:
            continue
        for span_left, span_right in spans:
            for x in range(span_left, span_right):
                sampled += 1
                seam_luminance = max(
                    pixel_luminance(rows[row_index], x, channels) for row_index in seam_rows
                )
                baseline_luminance = sum(
                    pixel_luminance(rows[row_index], x, channels) for row_index in baseline_rows
                ) / len(baseline_rows)
                if seam_luminance >= 125 and seam_luminance - baseline_luminance >= 35:
                    seam_pixels += 1
    if sampled == 0:
        return 0.0, 0
    return seam_pixels / sampled, seam_pixels


def pixel_luminance(row: bytes, x: int, channels: int) -> float:
    base = x * channels
    red, green, blue = row[base], row[base + 1], row[base + 2]
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue


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
drag_screenshot_path = os.environ.get("VISUAL_DRAG_SCREENSHOT_PATH", "").strip()
visual_display_bounds = os.environ.get("VISUAL_DISPLAY_BOUNDS", "").strip()
visual_drag_points = os.environ.get("VISUAL_DRAG_POINTS", "").strip()
mask_probe_path = os.environ.get("MASK_PROBE_PATH", "").strip()
expected_min_freeze_commits = int(os.environ.get("EXPECTED_MIN_FREEZE_COMMITS", "1") or "1")
expected_min_frozen_transform_commits = int(
    os.environ.get("EXPECTED_MIN_FROZEN_TRANSFORM_COMMITS", "0") or "0"
)
expected_freeze_editability = [
    value.strip().lower() == "true"
    for value in os.environ.get("EXPECTED_FREEZE_EDITABILITY", "").split(",")
    if value.strip()
]
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
handoffs = events.get("capture_timing.frozen_first_display_handoff", [])
handoff = handoffs[-1] if handoffs else {}
freeze_commits = events.get("capture_timing.freeze_commit", [])
freeze_commit_failures = events.get("capture_timing.freeze_commit_failed", [])
frozen_transform_commits = events.get("capture.frozen_selection_transform_commit", [])
max_window_list_below_overlay_commits = int(
    os.environ.get("MAX_WINDOW_LIST_BELOW_OVERLAY_COMMITS", "0") or "0"
)

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
if expected_min_frozen_transform_commits:
    print(
        "[smoke] frozen_transform_commits "
        f"expected>={expected_min_frozen_transform_commits} "
        f"actual={len(frozen_transform_commits)}"
    )
    if len(frozen_transform_commits) < expected_min_frozen_transform_commits:
        failures.append(
            "frozen transform commit count too small: "
            f"{len(frozen_transform_commits)} < {expected_min_frozen_transform_commits}"
        )
max_latest_unchanged_frame_age_ms = threshold("MAX_LATEST_UNCHANGED_FRAME_AGE_MS", 150.0)
max_total_ms = threshold("MAX_FREEZE_COMMIT_MS", 90.0)
max_present_ms = mode_threshold(
    "MAX_FREEZE_PRESENT_MS",
    55.0 if expected_mode == "classic" else 35.0,
)
max_snapshot_wait_ms = threshold("MAX_FREEZE_SNAPSHOT_WAIT_MS", 45.0)
window_list_below_overlay_commits = 0
for index, commit in enumerate(freeze_commits, start=1):
    commit_source = commit.get("snapshotSource", "unknown")
    commit_frame_age_ms = float_field(commit, "frameAgeMs")
    commit_filter_complete = bool_field(commit, "selfCaptureFilterComplete")
    total_ms = float_field(commit, "totalMs")
    present_ms = float_field(commit, "presentMs")
    snapshot_wait_ms = float_field(commit, "snapshotWaitMs")
    base_ready = bool_field(commit, "baseReady")
    self_capture_safe = bool_field(commit, "selfCaptureSafe")
    print(
        f"[smoke] freeze_commit[{index}] "
        f"totalMs={total_ms:.2f} presentMs={present_ms:.2f} "
        f"snapshotWaitMs={snapshot_wait_ms:.2f} snapshotSource={commit_source} "
        f"selfCaptureSafe={self_capture_safe} "
        f"selfCaptureFilterComplete={commit_filter_complete} baseReady={base_ready}"
    )
    if total_ms > max_total_ms:
        failures.append(
            f"freeze_commit[{index}] totalMs={total_ms:.2f} exceeds {max_total_ms:.2f}"
        )
    if present_ms > max_present_ms:
        failures.append(
            f"freeze_commit[{index}] presentMs={present_ms:.2f} exceeds {max_present_ms:.2f}"
        )
    if snapshot_wait_ms > max_snapshot_wait_ms:
        failures.append(
            f"freeze_commit[{index}] snapshotWaitMs={snapshot_wait_ms:.2f} "
            f"exceeds {max_snapshot_wait_ms:.2f}"
        )
    if not bool_field(commit, "selfCaptureSafe"):
        failures.append(
            f"freeze_commit[{index}] used a frame that could contain rsnap's own capture UI"
        )
    if commit_source == "window_list_below_overlay":
        window_list_below_overlay_commits += 1
    if commit_source == "latest_unchanged" and not commit_filter_complete and (
        commit_frame_age_ms > max_latest_unchanged_frame_age_ms
    ):
        failures.append(
            f"freeze_commit[{index}] latest_unchanged frameAgeMs={commit_frame_age_ms:.2f} "
            f"exceeds {max_latest_unchanged_frame_age_ms:.2f}"
        )
    if not base_ready:
        failures.append(f"freeze_commit[{index}] did not prepare the frozen base image")
if window_list_below_overlay_commits > max_window_list_below_overlay_commits:
    failures.append(
        "window-list below-overlay freeze commits exceeded limit: "
        f"{window_list_below_overlay_commits} > {max_window_list_below_overlay_commits}"
    )
if expected_freeze_editability:
    actual_editability = [
        bool_field(handoff_event, "frozenSelectionEditable")
        for handoff_event in handoffs[-len(expected_freeze_editability) :]
    ]
    print(
        "[smoke] freeze_editability "
        f"expected={expected_freeze_editability} actual={actual_editability}"
    )
    if actual_editability != expected_freeze_editability:
        failures.append(
            "frozen editability sequence mismatch: "
            f"expected {expected_freeze_editability}, got {actual_editability}"
        )

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

if drag_screenshot_path:
    for drag_path in [path for path in drag_screenshot_path.split(":") if path]:
        try:
            size = os.path.getsize(drag_path)
            dimensions = png_dimensions(drag_path)
        except OSError as exc:
            failures.append(f"drag screenshot missing: {exc}")
        else:
            if dimensions is None:
                failures.append(f"drag screenshot is not a PNG: {drag_path}")
            else:
                width, height = dimensions
                print(
                    "[smoke] drag_screenshot "
                    f"path={drag_path} width={width} height={height} bytes={size}"
                )
                if width < 500 or height < 400:
                    failures.append(f"drag screenshot too small: {width}x{height}")
                if visual_display_bounds and visual_drag_points:
                    leak = count_leaked_drag_border(
                        drag_path, visual_display_bounds, visual_drag_points
                    )
                    if leak is None:
                        failures.append("drag screenshot could not be decoded for border-leak check")
                    else:
                        leak_ratio, leak_pixels = leak
                        max_leak_ratio = threshold("MAX_DRAG_BORDER_LEAK_RATIO", 0.015)
                        max_leak_pixels = int(threshold("MAX_DRAG_BORDER_LEAK_PIXELS", 80))
                        print(
                            "[smoke] drag_border_leak "
                            f"ratio={leak_ratio:.5f} pixels={leak_pixels}"
                        )
                        if leak_ratio > max_leak_ratio and leak_pixels > max_leak_pixels:
                            failures.append(
                                "live drag border leaked outside the selection "
                                f"(ratio={leak_ratio:.5f}, pixels={leak_pixels})"
                            )
                    seam = count_leaked_horizontal_seam(
                        drag_path, visual_display_bounds, visual_drag_points
                    )
                    if seam is None:
                        failures.append("drag screenshot could not be decoded for seam check")
                    else:
                        seam_ratio, seam_pixels = seam
                        max_seam_ratio = threshold("MAX_DRAG_HORIZONTAL_SEAM_RATIO", 0.01)
                        max_seam_pixels = int(threshold("MAX_DRAG_HORIZONTAL_SEAM_PIXELS", 80))
                        print(
                            "[smoke] drag_horizontal_seam "
                            f"ratio={seam_ratio:.5f} pixels={seam_pixels}"
                        )
                        if seam_ratio > max_seam_ratio and seam_pixels > max_seam_pixels:
                            failures.append(
                                "live drag scrim leaked a horizontal seam outside the selection "
                                f"(ratio={seam_ratio:.5f}, pixels={seam_pixels})"
                            )

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
            if len(row) != 3:
                continue
            phase = "dragging" if row[0] == "holding" else row[0]
            if phase not in values:
                continue
            try:
                values[phase].append(float(row[2]))
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
