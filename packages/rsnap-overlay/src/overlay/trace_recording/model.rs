use serde::{Deserialize, Serialize};

use crate::overlay::{MonitorRect, RectPoints, ScrollCaptureFrameSource};
use crate::scroll_capture::{ScrollDirection, ScrollObserveOutcome, ScrollSession};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ScrollCaptureLiveTraceManifest {
	pub(crate) schema: String,
	pub(crate) trace_id: String,
	pub(crate) started_unix_ms: u64,
	pub(crate) preview_width_px: u32,
	pub(crate) monitor: ScrollCaptureTraceMonitor,
	pub(crate) capture_rect_pixels: ScrollCaptureTraceRect,
	pub(crate) base_frame_path: String,
	pub(crate) entries: Vec<ScrollCaptureLiveTraceEntry>,
	pub(crate) final_preview_path: Option<String>,
	pub(crate) final_export_path: Option<String>,
	pub(crate) final_snapshot: Option<ScrollCaptureTraceSessionSnapshot>,
	pub(crate) final_error: Option<String>,
	pub(crate) finalized: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ScrollCaptureTraceMonitor {
	pub(crate) id: u32,
	pub(crate) origin_x: i32,
	pub(crate) origin_y: i32,
	pub(crate) width: u32,
	pub(crate) height: u32,
	pub(crate) scale_factor_x1000: u32,
}
impl From<MonitorRect> for ScrollCaptureTraceMonitor {
	fn from(value: MonitorRect) -> Self {
		Self {
			id: value.id,
			origin_x: value.origin.x,
			origin_y: value.origin.y,
			width: value.width,
			height: value.height,
			scale_factor_x1000: value.scale_factor_x1000,
		}
	}
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ScrollCaptureTraceRect {
	pub(crate) x: u32,
	pub(crate) y: u32,
	pub(crate) width: u32,
	pub(crate) height: u32,
}
impl From<RectPoints> for ScrollCaptureTraceRect {
	fn from(value: RectPoints) -> Self {
		Self { x: value.x, y: value.y, width: value.width, height: value.height }
	}
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "entry_type", rename_all = "snake_case")]
pub(crate) enum ScrollCaptureLiveTraceEntry {
	Input(ScrollCaptureTraceInputEntry),
	Frame(ScrollCaptureTraceFrameEntry),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ScrollCaptureTraceInputEntry {
	pub(crate) applied_at_ms: u64,
	pub(crate) seq: u64,
	pub(crate) cursor_global_x: f64,
	pub(crate) cursor_global_y: f64,
	pub(crate) delta_y: f64,
	pub(crate) gesture_active: bool,
	pub(crate) gesture_ended: bool,
	pub(crate) recorded_age_ms: u64,
	pub(crate) snapshot_after: ScrollCaptureTraceSessionSnapshot,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ScrollCaptureTraceFrameEntry {
	pub(crate) observed_at_ms: u64,
	pub(crate) allow_stale_input: bool,
	pub(crate) prior_block_reason: Option<String>,
	pub(crate) frame_path: String,
	pub(crate) frame_source: ScrollCaptureTraceFrameSource,
	pub(crate) frame_dimensions: [u32; 2],
	pub(crate) snapshot_after: ScrollCaptureTraceSessionSnapshot,
	pub(crate) outcome: ScrollCaptureTraceRecordedOutcome,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScrollCaptureTraceFrameSource {
	Worker { request_id: u64 },
	LiveStream { frame_seq: u64 },
}
impl From<ScrollCaptureFrameSource> for ScrollCaptureTraceFrameSource {
	fn from(value: ScrollCaptureFrameSource) -> Self {
		match value {
			ScrollCaptureFrameSource::Worker { request_id } => Self::Worker { request_id },
			ScrollCaptureFrameSource::LiveStream { frame_seq } => Self::LiveStream { frame_seq },
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScrollCaptureTraceDirection {
	Up,
	Down,
}
impl From<ScrollDirection> for ScrollCaptureTraceDirection {
	fn from(value: ScrollDirection) -> Self {
		match value {
			ScrollDirection::Up => Self::Up,
			ScrollDirection::Down => Self::Down,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ScrollCaptureTraceRecordedOutcome {
	NoChange,
	PreviewUpdated,
	UnsupportedDirection { direction: ScrollCaptureTraceDirection },
	Committed { direction: ScrollCaptureTraceDirection, growth_rows: u32 },
	Error { message: String },
}
impl From<ScrollObserveOutcome> for ScrollCaptureTraceRecordedOutcome {
	fn from(value: ScrollObserveOutcome) -> Self {
		match value {
			ScrollObserveOutcome::NoChange => Self::NoChange,
			ScrollObserveOutcome::PreviewUpdated => Self::PreviewUpdated,
			ScrollObserveOutcome::UnsupportedDirection { direction } => {
				Self::UnsupportedDirection { direction: direction.into() }
			},
			ScrollObserveOutcome::Committed { direction, growth_rows } => {
				Self::Committed { direction: direction.into(), growth_rows }
			},
		}
	}
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ScrollCaptureTraceSessionSnapshot {
	pub(crate) input_direction: Option<ScrollCaptureTraceDirection>,
	pub(crate) input_gesture_active: bool,
	pub(crate) downward_motion_rows_pending: f64,
	pub(crate) input_age_ms: Option<u64>,
	pub(crate) current_viewport_top_y: Option<i32>,
	pub(crate) export_dimensions: Option<[u32; 2]>,
	pub(crate) preview_dimensions: Option<[u32; 2]>,
	pub(crate) growth_commit_count: Option<usize>,
	pub(crate) preview_segment_count: Option<usize>,
	pub(crate) export_segment_count: Option<usize>,
	pub(crate) preview_export_segments_aligned: Option<bool>,
	pub(crate) last_commit_decision_source: Option<String>,
	pub(crate) last_commit_detected_motion_rows: Option<u32>,
	pub(crate) last_commit_effective_motion_rows_hint: Option<u32>,
	pub(crate) last_preview_segment_height_px: Option<u32>,
	pub(crate) last_export_segment_height_px: Option<u32>,
}
impl ScrollCaptureTraceSessionSnapshot {
	pub(crate) fn capture(
		session: Option<&ScrollSession>,
		preview_dimensions: Option<[u32; 2]>,
		input_direction: Option<ScrollDirection>,
		input_gesture_active: bool,
		downward_motion_rows_pending: f64,
		input_age_ms: Option<u64>,
	) -> Self {
		let telemetry = session.map(ScrollSession::commit_telemetry);

		Self {
			input_direction: input_direction.map(Into::into),
			input_gesture_active,
			downward_motion_rows_pending,
			input_age_ms,
			current_viewport_top_y: session.map(ScrollSession::current_viewport_top_y),
			export_dimensions: session.map(ScrollSession::export_dimensions).map(|(w, h)| [w, h]),
			preview_dimensions,
			growth_commit_count: telemetry.as_ref().map(|value| value.growth_commit_count),
			preview_segment_count: telemetry.as_ref().map(|value| value.preview_segment_count),
			export_segment_count: telemetry.as_ref().map(|value| value.export_segment_count),
			preview_export_segments_aligned: telemetry
				.as_ref()
				.map(|value| value.preview_export_segments_aligned),
			last_commit_decision_source: telemetry
				.as_ref()
				.and_then(|value| value.last_commit_decision_source)
				.map(str::to_owned),
			last_commit_detected_motion_rows: telemetry
				.as_ref()
				.and_then(|value| value.last_commit_detected_motion_rows),
			last_commit_effective_motion_rows_hint: telemetry
				.as_ref()
				.and_then(|value| value.last_commit_effective_motion_rows_hint),
			last_preview_segment_height_px: telemetry
				.as_ref()
				.and_then(|value| value.last_preview_segment_height_px),
			last_export_segment_height_px: telemetry
				.as_ref()
				.and_then(|value| value.last_export_segment_height_px),
		}
	}
}
