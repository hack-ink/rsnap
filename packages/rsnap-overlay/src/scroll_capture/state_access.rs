use image::RgbaImage;

use crate::scroll_capture::{ScrollCommitTelemetry, ScrollSession};

impl ScrollSession {
	pub(crate) fn preview_image(&self) -> &RgbaImage {
		&self.preview_image
	}

	pub(crate) fn preview_display_image(&self) -> RgbaImage {
		self.export_image.clone()
	}

	pub(crate) fn preview_display_mode(&self) -> &'static str {
		"committed"
	}

	pub(crate) fn export_image(&self) -> &RgbaImage {
		&self.export_image
	}

	pub(crate) fn current_viewport_top_y(&self) -> i32 {
		self.current_viewport_top_y
	}

	pub(crate) fn export_dimensions(&self) -> (u32, u32) {
		self.export_image.dimensions()
	}

	pub(crate) fn last_block_reason(&self) -> Option<&'static str> {
		self.last_block_reason
	}

	pub(crate) fn commit_telemetry(&self) -> ScrollCommitTelemetry {
		let last_commit = self.growth_history.last();

		ScrollCommitTelemetry {
			current_viewport_top_y: self.current_viewport_top_y,
			preview_dimensions: self.preview_image.dimensions(),
			export_dimensions: self.export_image.dimensions(),
			growth_commit_count: self.growth_history.len(),
			preview_segment_count: self.bottom_preview_segments.len(),
			export_segment_count: self.bottom_segments.len(),
			preview_export_segments_aligned: self.bottom_segments.len()
				== self.bottom_preview_segments.len()
				&& self.bottom_segments.len() == self.growth_history.len(),
			last_commit_decision_source: last_commit.map(|commit| commit.decision_source),
			last_commit_detected_motion_rows: last_commit
				.and_then(|commit| commit.detected_motion_rows),
			last_commit_effective_motion_rows_hint: last_commit
				.and_then(|commit| commit.effective_motion_rows_hint),
			last_block_reason: self.last_block_reason,
			last_downward_sample_registration_result: self.last_downward_sample_registration_result,
			last_downward_sample_registration_source: self.last_downward_sample_registration_source,
			last_downward_sample_registration_motion_rows: self
				.last_downward_sample_registration_motion_rows,
			last_downward_sample_registration_provisional_viewport_top_y: self
				.last_downward_sample_registration_provisional_viewport_top_y,
			observed_sample_registration_result: self.last_observed_sample_registration_result,
			observed_sample_registration_reason: self.last_observed_sample_registration_reason,
			observed_sample_registration_motion_rows: self
				.last_observed_sample_registration_motion_rows,
			observed_sample_registration_mean_abs_diff_x100: self
				.last_observed_sample_registration_mean_abs_diff_x100,
			preview_only_local_registration_result: self
				.last_preview_only_local_registration_result,
			preview_only_local_registration_reason: self
				.last_preview_only_local_registration_reason,
			preview_only_local_registration_motion_rows: self
				.last_preview_only_local_registration_motion_rows,
			preview_only_local_registration_mean_abs_diff_x100: self
				.last_preview_only_local_registration_mean_abs_diff_x100,
			last_downward_viewport_candidate_count: self.last_downward_viewport_candidate_count,
			last_downward_viewport_candidates_before_prune: self
				.last_downward_viewport_candidates_before_prune
				.clone(),
			last_downward_viewport_candidates_after_prune: self
				.last_downward_viewport_candidates_after_prune
				.clone(),
			sample_eval_last_motion_rows_hint: self.last_sample_eval_last_motion_rows_hint,
			sample_eval_transient_motion_rows_hint: self
				.last_sample_eval_transient_motion_rows_hint,
			sample_eval_effective_motion_rows_hint: self
				.last_sample_eval_effective_motion_rows_hint,
			sample_eval_transient_burst_search_enabled: self
				.last_sample_eval_transient_burst_search_enabled,
			preview_only_local_viewport_top_y: self
				.last_preview_only_downward_local_sample
				.as_ref()
				.map(|sample| sample.viewport_top_y),
			last_preview_segment_height_px: self
				.bottom_preview_segments
				.last()
				.map(RgbaImage::height),
			last_export_segment_height_px: self.bottom_segments.last().map(RgbaImage::height),
		}
	}

	pub(crate) fn undo_last_append(&mut self) -> bool {
		let Some(_commit) = self.growth_history.pop() else {
			return false;
		};
		let _ = self.bottom_segments.pop();
		let _ = self.bottom_preview_segments.pop();
		let Ok(export_image) = self.rebuild_export_image() else {
			return false;
		};
		let Ok(preview_image) = self.rebuild_preview_image() else {
			return false;
		};

		self.export_image = export_image;
		self.preview_image = preview_image;

		if let Some(previous) = self.growth_history.last() {
			self.last_motion_rows_hint = Some(previous.growth_rows);
			self.current_viewport_top_y = previous.viewport_top_y;
			self.observed_viewport_top_y = previous.viewport_top_y;
			self.last_committed_frame = previous.frame.clone();
			self.worker_pairwise_previous_frame = previous.frame.clone();
			self.worker_pairwise_requires_committed_reacquire = false;
			self.last_sample_frame = previous.frame.clone();
			self.last_sample_fingerprint = Some(super::scroll_capture_fingerprint(&previous.frame));
			self.last_downward_observed_frame = previous.frame.clone();
			self.last_downward_observed_fingerprint =
				Some(super::scroll_capture_fingerprint(&previous.frame));

			self.clear_preview_only_downward_local_sample();

			self.last_unconfirmed_upward_fingerprint = None;
			self.resume_frontier_top_y = None;
			self.resume_frontier_requires_reacquire = false;
		} else {
			self.last_committed_frame = self.anchor_frame.clone();
			self.worker_pairwise_previous_frame = self.anchor_frame.clone();
			self.worker_pairwise_requires_committed_reacquire = false;
			self.last_sample_frame = self.anchor_frame.clone();
			self.last_sample_fingerprint =
				Some(super::scroll_capture_fingerprint(&self.anchor_frame));
			self.last_downward_observed_frame = self.anchor_frame.clone();
			self.last_downward_observed_fingerprint =
				Some(super::scroll_capture_fingerprint(&self.anchor_frame));

			self.clear_preview_only_downward_local_sample();

			self.last_unconfirmed_upward_fingerprint = None;
			self.last_motion_rows_hint = None;
			self.current_viewport_top_y = 0;
			self.observed_viewport_top_y = 0;
			self.resume_frontier_top_y = None;
			self.resume_frontier_requires_reacquire = false;
		}

		true
	}
}
