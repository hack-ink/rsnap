use crate::scroll_capture::{
	MotionObservation, ResumeFrontierMatchLog, ScrollDirection, ScrollSession, UpInputMatchLog,
	UpInputSearchWindowLog,
};

impl ScrollSession {
	pub(super) fn log_decision(
		&mut self,
		op: &'static str,
		input_direction: ScrollDirection,
		detected_motion: Option<MotionObservation>,
		candidate_viewport_top_y: Option<i32>,
		growth_rows: Option<u32>,
		block_reason: Option<&'static str>,
	) {
		self.last_block_reason = block_reason;

		tracing::debug!(
			op,
			input_direction = ?input_direction,
			detected_direction = ?detected_motion.map(|motion| motion.direction),
			detected_motion_rows = ?detected_motion.map(|motion| motion.motion_rows),
			candidate_viewport_top_y = ?candidate_viewport_top_y,
			growth_rows = ?growth_rows,
			block_reason = ?block_reason,
			current_viewport_top_y = self.current_viewport_top_y,
			observed_viewport_top_y = self.observed_viewport_top_y,
			resume_frontier_top_y = ?self.resume_frontier_top_y,
			resume_frontier_requires_reacquire = self.resume_frontier_requires_reacquire,
			export_height_px = self.export_image.height(),
			preview_height_px = self.preview_image.height(),
			"Scroll-capture session evaluated a motion decision."
		);
	}

	pub(super) fn log_up_input_match_eval(&self, log: UpInputMatchLog) {
		tracing::info!(
			op = "scroll_capture.up_input_match_eval",
			sample_motion_direction = ?log.sample_motion.map(|motion| motion.direction),
			sample_motion_rows = ?log.sample_motion.map(|motion| motion.motion_rows),
			sample_down_match_rows = ?log.sample_down_match.map(|matched| matched.motion_rows),
			sample_down_match_mean_abs_diff_x100 =
				?log.sample_down_match.map(|matched| matched.mean_abs_diff_x100),
			sample_up_match_rows = ?log.sample_up_match.map(|matched| matched.motion_rows),
			sample_up_match_mean_abs_diff_x100 =
				?log.sample_up_match.map(|matched| matched.mean_abs_diff_x100),
			committed_down_match_rows = ?log.committed_down_match.map(|matched| matched.motion_rows),
			committed_down_match_mean_abs_diff_x100 =
				?log.committed_down_match.map(|matched| matched.mean_abs_diff_x100),
			committed_up_match_rows = ?log.committed_up_match.map(|matched| matched.motion_rows),
			committed_up_match_mean_abs_diff_x100 =
				?log.committed_up_match.map(|matched| matched.mean_abs_diff_x100),
			sample_override_wins = log.sample_override_wins,
			committed_override_wins = log.committed_override_wins,
			current_viewport_top_y = self.current_viewport_top_y,
			observed_viewport_top_y = self.observed_viewport_top_y,
			resume_frontier_top_y = ?self.resume_frontier_top_y,
			resume_frontier_requires_reacquire = self.resume_frontier_requires_reacquire,
			"Scroll-capture session evaluated upward rewind match candidates."
		);
	}

	pub(super) fn log_up_input_search_window_eval(&self, log: UpInputSearchWindowLog<'_>) {
		tracing::info!(
			op = "scroll_capture.up_input_search_window_eval",
			last_motion_rows_hint = ?self.last_motion_rows_hint,
			transient_motion_rows_hint = ?self.transient_motion_rows_hint,
			effective_motion_rows_hint = ?self.effective_motion_rows_hint(),
			sample_delta = ?log.sample_delta,
			frame_equals_last_sample = log.frame_equals_last_sample,
			frame_equals_last_committed = log.frame_equals_last_committed,
			sample_preferred_range_start =
				?log.sample_down_match_eval.preferred_range.map(|range| range.start),
			sample_preferred_range_end =
				?log.sample_down_match_eval.preferred_range.map(|range| range.end),
			sample_max_motion_rows = log.sample_down_match_eval.max_motion_rows,
			sample_down_preferred_only_rows =
				?log.sample_down_match_eval.preferred_only_match.map(|matched| matched.motion_rows),
			sample_down_preferred_only_mean_abs_diff_x100 = ?log.sample_down_match_eval
				.preferred_only_match
				.map(|matched| matched.mean_abs_diff_x100),
			sample_down_final_rows =
				?log.sample_down_match_eval.final_match.map(|matched| matched.motion_rows),
			sample_down_final_mean_abs_diff_x100 = ?log.sample_down_match_eval
				.final_match
				.map(|matched| matched.mean_abs_diff_x100),
			sample_down_used_full_range_fallback =
				log.sample_down_match_eval.used_full_range_fallback,
			sample_up_final_rows =
				?log.sample_up_match_eval.final_match.map(|matched| matched.motion_rows),
			sample_up_final_mean_abs_diff_x100 = ?log.sample_up_match_eval
				.final_match
				.map(|matched| matched.mean_abs_diff_x100),
			committed_preferred_range_start =
				?log.committed_down_match_eval.preferred_range.map(|range| range.start),
			committed_preferred_range_end =
				?log.committed_down_match_eval.preferred_range.map(|range| range.end),
			committed_max_motion_rows = log.committed_down_match_eval.max_motion_rows,
			committed_down_preferred_only_rows = ?log.committed_down_match_eval
				.preferred_only_match
				.map(|matched| matched.motion_rows),
			committed_down_preferred_only_mean_abs_diff_x100 = ?log.committed_down_match_eval
				.preferred_only_match
				.map(|matched| matched.mean_abs_diff_x100),
			committed_down_final_rows =
				?log.committed_down_match_eval.final_match.map(|matched| matched.motion_rows),
			committed_down_final_mean_abs_diff_x100 = ?log.committed_down_match_eval
				.final_match
				.map(|matched| matched.mean_abs_diff_x100),
			committed_down_used_full_range_fallback =
				log.committed_down_match_eval.used_full_range_fallback,
			committed_up_final_rows =
				?log.committed_up_match_eval.final_match.map(|matched| matched.motion_rows),
			committed_up_final_mean_abs_diff_x100 = ?log.committed_up_match_eval
				.final_match
				.map(|matched| matched.mean_abs_diff_x100),
			current_viewport_top_y = self.current_viewport_top_y,
			observed_viewport_top_y = self.observed_viewport_top_y,
			resume_frontier_top_y = ?self.resume_frontier_top_y,
			resume_frontier_requires_reacquire = self.resume_frontier_requires_reacquire,
			"Scroll-capture session evaluated upward-input search windows."
		);
	}

	pub(super) fn log_resume_frontier_match_eval(&self, log: ResumeFrontierMatchLog) {
		tracing::info!(
			op = "scroll_capture.resume_frontier_match_eval",
			motion_rows = log.motion_rows,
			candidate_observed_viewport_top_y = log.candidate_observed_viewport_top_y,
			residual_growth_rows = log.residual_growth_rows,
			raw_committed_down_match_rows =
				?log.raw_committed_down_match.map(|matched| matched.motion_rows),
			raw_committed_down_match_mean_abs_diff_x100 =
				?log.raw_committed_down_match.map(|matched| matched.mean_abs_diff_x100),
			trusted_committed_down_match_rows =
				?log.trusted_committed_down_match.map(|matched| matched.motion_rows),
			trusted_committed_down_match_mean_abs_diff_x100 =
				?log.trusted_committed_down_match.map(|matched| matched.mean_abs_diff_x100),
			committed_up_match_rows = ?log.committed_up_match.map(|matched| matched.motion_rows),
			committed_up_match_mean_abs_diff_x100 =
				?log.committed_up_match.map(|matched| matched.mean_abs_diff_x100),
			frame_reacquires_last_committed_viewport = log.frame_reacquires_last_committed_viewport,
			current_viewport_top_y = self.current_viewport_top_y,
			observed_viewport_top_y = self.observed_viewport_top_y,
			resume_frontier_top_y = ?self.resume_frontier_top_y,
			resume_frontier_requires_reacquire = self.resume_frontier_requires_reacquire,
			"Scroll-capture session evaluated resume-frontier match candidates."
		);
	}
}
