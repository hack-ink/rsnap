#[cfg(target_os = "macos")]
use std::time::Instant;

use color_eyre::Result;

#[cfg(target_os = "macos")]
use crate::overlay::scroll_capture_timing::SCROLL_CAPTURE_DUPLICATE_WORKER_FRAME_RETRY_INTERVAL;
use crate::overlay::{OverlaySession, ScrollCaptureFrameSource, ScrollObserveOutcome};
use crate::scroll_capture::ScrollDirection;
use crate::scroll_capture::ScrollSession;

impl OverlaySession {
	#[allow(clippy::too_many_lines)]
	pub(super) fn handle_scroll_capture_frame_outcome(
		&mut self,
		outcome: &Result<ScrollObserveOutcome>,
		source: ScrollCaptureFrameSource,
		frame_px: (u32, u32),
	) {
		match outcome {
			Ok(ScrollObserveOutcome::NoChange) => {
				self.log_scroll_capture_no_change(source, frame_px)
			},
			Ok(ScrollObserveOutcome::PreviewUpdated) => {
				self.log_scroll_capture_preview_updated(source, frame_px);
			},
			Ok(ScrollObserveOutcome::UnsupportedDirection { direction }) => {
				let export_size = self
					.scroll_capture
					.session
					.as_ref()
					.map_or((0, 0), ScrollSession::export_dimensions);

				tracing::info!(
					op = "scroll_capture.unsupported_direction",
					frame_source = source.as_str(),
					worker_request_id = ?source.worker_request_id(),
					direction = ?direction,
					frame_px = ?frame_px,
					export_px = ?export_size,
					"Scroll-capture sample moved in an unsupported direction."
				);
			},
			Ok(ScrollObserveOutcome::Committed { direction, growth_rows }) => {
				self.log_scroll_capture_committed(source, frame_px, *direction, *growth_rows);
			},
			Err(err) => {
				self.scroll_capture_set_error(format!("{err:#}"));
			},
		}
	}

	fn log_scroll_capture_no_change(
		&mut self,
		source: ScrollCaptureFrameSource,
		frame_px: (u32, u32),
	) {
		let last_block_reason =
			self.scroll_capture.session.as_ref().and_then(ScrollSession::last_block_reason);

		tracing::info!(
			op = "scroll_capture.frame_observed",
			frame_source = source.as_str(),
			worker_request_id = ?source.worker_request_id(),
			outcome = "no_change",
			frame_px = ?frame_px,
			input_direction = ?self.scroll_capture.input_direction,
			input_gesture_active = self.scroll_capture.input_gesture_active,
			last_block_reason = ?last_block_reason,
			export_px = ?self.scroll_capture.session.as_ref().map(ScrollSession::export_dimensions),
			"Scroll-capture observed a frame but kept session state unchanged."
		);

		if let Some(request_id) = source.worker_request_id() {
			#[cfg(target_os = "macos")]
			{
				let now = Instant::now();

				match last_block_reason {
					Some("frame_matches_last_committed_frame") => self
						.schedule_backoff_scroll_capture_worker_retry_if_fresh_downward_input(
							now,
							"worker_duplicate_committed_frame",
							SCROLL_CAPTURE_DUPLICATE_WORKER_FRAME_RETRY_INTERVAL,
						),
					_ => self
						.schedule_immediate_scroll_capture_worker_retry_if_fresh_downward_input(
							now,
							"worker_no_change",
						),
				}
			}

			tracing::info!(
				op = "scroll_capture.worker_frame_processed",
				request_id,
				outcome = "no_change",
				frame_px = ?frame_px,
				input_direction = ?self.scroll_capture.input_direction,
				last_block_reason = ?last_block_reason,
				"Worker-fed scroll-capture frame reached the session without changing preview or export state."
			);
		}
	}

	fn log_scroll_capture_preview_updated(
		&self,
		source: ScrollCaptureFrameSource,
		frame_px: (u32, u32),
	) {
		tracing::info!(
			op = "scroll_capture.frame_observed",
			frame_source = source.as_str(),
			worker_request_id = ?source.worker_request_id(),
			outcome = "preview_updated",
			frame_px = ?frame_px,
			input_direction = ?self.scroll_capture.input_direction,
			input_gesture_active = self.scroll_capture.input_gesture_active,
			export_px = ?self.scroll_capture.session.as_ref().map(ScrollSession::export_dimensions),
			preview_px = ?self.scroll_capture_preview_dimensions().map(|[w, h]| (w, h)),
			"Scroll-capture observed a frame and advanced session sampling state without committing stitched growth."
		);

		if let Some(request_id) = source.worker_request_id() {
			tracing::info!(
				op = "scroll_capture.worker_frame_processed",
				request_id,
				outcome = "preview_updated",
				frame_px = ?frame_px,
				input_direction = ?self.scroll_capture.input_direction,
				"Worker-fed scroll-capture frame refreshed preview state without committing stitched growth."
			);
		}
	}

	fn log_scroll_capture_committed(
		&mut self,
		source: ScrollCaptureFrameSource,
		frame_px: (u32, u32),
		direction: ScrollDirection,
		growth_rows: u32,
	) {
		self.refresh_scroll_preview_committed_image();
		self.refresh_scroll_preview_display_image();
		self.sync_scroll_preview_segments();
		self.request_redraw_scroll_preview_window();

		let telemetry = self.scroll_capture.session.as_ref().map(ScrollSession::commit_telemetry);
		let export_size =
			telemetry.as_ref().map_or((0, 0), |telemetry| telemetry.export_dimensions);
		let preview_size =
			telemetry.as_ref().map_or((0, 0), |telemetry| telemetry.preview_dimensions);

		tracing::info!(
			op = "scroll_capture.committed",
			frame_source = source.as_str(),
			worker_request_id = ?source.worker_request_id(),
			direction = ?direction,
			growth_rows,
			frame_px = ?frame_px,
			export_px = ?export_size,
			preview_px = ?preview_size,
			current_viewport_top_y = ?telemetry.as_ref().map(|telemetry| telemetry.current_viewport_top_y),
			growth_commit_count = ?telemetry.as_ref().map(|telemetry| telemetry.growth_commit_count),
			preview_segment_count = ?telemetry.as_ref().map(|telemetry| telemetry.preview_segment_count),
			export_segment_count = ?telemetry.as_ref().map(|telemetry| telemetry.export_segment_count),
			last_commit_decision_source = ?telemetry.as_ref().map(|telemetry| telemetry.last_commit_decision_source),
			last_commit_detected_motion_rows = ?telemetry.as_ref().map(|telemetry| telemetry.last_commit_detected_motion_rows),
			last_commit_effective_motion_rows_hint = ?telemetry.as_ref().map(|telemetry| telemetry.last_commit_effective_motion_rows_hint),
			last_preview_segment_height_px = ?telemetry.as_ref().map(|telemetry| telemetry.last_preview_segment_height_px),
			last_export_segment_height_px = ?telemetry.as_ref().map(|telemetry| telemetry.last_export_segment_height_px),
			preview_export_segments_aligned = ?telemetry.as_ref().map(|telemetry| telemetry.preview_export_segments_aligned),
			"Scroll sample committed stitched growth."
		);
	}
}
