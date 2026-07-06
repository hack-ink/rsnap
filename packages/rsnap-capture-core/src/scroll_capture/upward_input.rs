use color_eyre::eyre::Result;
use image::RgbaImage;

use crate::scroll_capture::{
	DirectionMatch, DirectionMatchEval, MotionObservation, OverlapSearchConfig, OverlapSearchRange,
	ScrollDirection, ScrollObserveOutcome, ScrollSession, UpInputMatchLog, UpInputSearchWindowLog,
	UpwardInputDiagnostics, support,
};

impl ScrollSession {
	pub(super) fn observe_upward_input(
		&mut self,
		frame: RgbaImage,
		fingerprint: Vec<u8>,
		sample_delta: Option<u32>,
		sample_motion: Option<MotionObservation>,
		_preview_changed: bool,
	) -> Result<ScrollObserveOutcome> {
		let diagnostics = self.diagnose_upward_input(&frame);

		self.log_upward_input_diagnostics(&diagnostics, sample_delta, sample_motion, &frame);

		if let Some(outcome) = self.observe_upward_input_while_rewind_active(
			&frame,
			&fingerprint,
			sample_motion,
			&diagnostics,
		) {
			return Ok(outcome);
		}
		if let Some(motion) = sample_motion {
			return Ok(self.observe_upward_input_with_sample_motion(
				&frame,
				fingerprint,
				motion,
				&diagnostics,
			));
		}
		if let Some((up_match, from_committed)) = support::preferred_upward_input_override_match(
			diagnostics.sample_override_match,
			diagnostics.committed_override_match,
		) {
			let (op, block_reason) = if from_committed {
				(
					"scroll_capture.rewind_armed_from_committed_match",
					"upward_input_matched_last_committed_frame",
				)
			} else {
				("scroll_capture.rewind_armed", "upward_input_matched_last_sample_frame")
			};

			return Ok(self.arm_upward_rewind_with_match(
				&frame,
				fingerprint,
				up_match,
				from_committed,
				op,
				block_reason,
			));
		}

		self.log_decision(
			"scroll_capture.up_input_without_rewind_match",
			ScrollDirection::Up,
			None,
			None,
			None,
			Some("preview_changed_without_upward_match"),
		);

		Ok(self.arm_unconfirmed_upward_rewind(
			&frame,
			fingerprint,
			None,
			diagnostics.committed_down_match_eval.final_match.is_none(),
			"scroll_capture.rewind_armed_without_match",
			"upward_input_preview_changed_without_reliable_upward_proof",
		))
	}

	fn observe_upward_input_while_rewind_active(
		&mut self,
		frame: &RgbaImage,
		fingerprint: &[u8],
		sample_motion: Option<MotionObservation>,
		diagnostics: &UpwardInputDiagnostics,
	) -> Option<ScrollObserveOutcome> {
		if support::rewind_active_upward_motion_should_fail_closed(
			diagnostics.sample_override_match,
			diagnostics.committed_override_match,
			diagnostics.committed_down_match_eval.final_match,
			self.resume_frontier_top_y.is_some(),
		) {
			self.log_decision(
				"scroll_capture.rewind_armed_without_match",
				ScrollDirection::Up,
				sample_motion,
				None,
				None,
				Some("rewind_active_upward_input_conflicted_with_last_committed_downward_match"),
			);

			return Some(self.arm_unconfirmed_upward_rewind(
				frame,
				fingerprint.to_vec(),
				sample_motion,
				false,
				"scroll_capture.rewind_armed_without_match",
				"rewind_active_upward_input_conflicted_with_last_committed_downward_match",
			));
		}

		support::rewind_active_upward_override_match(
			diagnostics.sample_override_match,
			diagnostics.committed_override_match,
			self.resume_frontier_top_y.is_some(),
		)
		.map(|(up_match, from_committed)| {
			let (op, block_reason) = if from_committed {
				(
					"scroll_capture.rewind_armed_from_committed_match",
					"rewind_active_upward_input_preferred_conservative_last_committed_match",
				)
			} else {
				(
					"scroll_capture.rewind_armed",
					"rewind_active_upward_input_preferred_last_sample_match",
				)
			};

			self.arm_upward_rewind_with_match(
				frame,
				fingerprint.to_vec(),
				up_match,
				from_committed,
				op,
				block_reason,
			)
		})
	}

	fn observe_upward_input_with_sample_motion(
		&mut self,
		frame: &RgbaImage,
		fingerprint: Vec<u8>,
		motion: MotionObservation,
		diagnostics: &UpwardInputDiagnostics,
	) -> ScrollObserveOutcome {
		if matches!(motion.direction, ScrollDirection::Up) {
			self.record_last_sample(frame, fingerprint);
			self.observe_upward_rewind(motion.motion_rows);
			self.log_decision(
				"scroll_capture.rewind_armed",
				ScrollDirection::Up,
				Some(motion),
				None,
				None,
				Some("upward_input_classified_as_upward_motion"),
			);

			return ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up };
		}

		if let Some((up_match, from_committed)) = support::preferred_upward_input_override_match(
			diagnostics.sample_override_match,
			diagnostics.committed_override_match,
		) {
			let (op, block_reason) = if from_committed {
				(
					"scroll_capture.rewind_armed_from_committed_match",
					"upward_input_overrode_non_upward_sample_motion_with_last_committed_match",
				)
			} else {
				(
					"scroll_capture.rewind_armed",
					"upward_input_overrode_non_upward_sample_motion_with_last_sample_match",
				)
			};

			return self.arm_upward_rewind_with_match(
				frame,
				fingerprint,
				up_match,
				from_committed,
				op,
				block_reason,
			);
		}

		self.log_decision(
			"scroll_capture.up_input_motion_mismatch",
			ScrollDirection::Up,
			Some(motion),
			None,
			None,
			Some("upward_input_classified_as_non_upward_motion"),
		);

		self.arm_unconfirmed_upward_rewind(
			frame,
			fingerprint,
			Some(motion),
			diagnostics.committed_down_match_eval.final_match.is_none(),
			"scroll_capture.rewind_armed_without_match",
			"upward_input_preview_changed_without_reliable_upward_proof",
		)
	}

	fn diagnose_upward_input(&self, frame: &RgbaImage) -> UpwardInputDiagnostics {
		let effective_motion_rows_hint = self.effective_motion_rows_hint();
		let sample_down_match_eval = self.diagnose_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Down,
			effective_motion_rows_hint,
		);
		let sample_up_match_eval = self.diagnose_upward_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			effective_motion_rows_hint,
		);
		let committed_down_match_eval = self.diagnose_reference_overlap_direction(
			&self.last_committed_frame,
			frame,
			ScrollDirection::Down,
			effective_motion_rows_hint,
		);
		let committed_up_match_eval = self.diagnose_upward_reference_overlap_direction(
			&self.last_committed_frame,
			frame,
			effective_motion_rows_hint,
		);

		UpwardInputDiagnostics {
			sample_override_match: support::preferred_upward_override_match(
				sample_up_match_eval.final_match,
				sample_down_match_eval.final_match,
			),
			committed_override_match: support::preferred_upward_override_match(
				committed_up_match_eval.final_match,
				committed_down_match_eval.final_match,
			),
			sample_down_match_eval,
			sample_up_match_eval,
			committed_down_match_eval,
			committed_up_match_eval,
		}
	}

	fn diagnose_upward_reference_overlap_direction(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
	) -> DirectionMatchEval {
		let hinted_eval = self.diagnose_reference_overlap_direction(
			previous,
			next,
			ScrollDirection::Up,
			motion_rows_hint,
		);

		if hinted_eval.final_match.is_some() || motion_rows_hint.is_none() {
			return hinted_eval;
		}

		let config = OverlapSearchConfig::default();
		let max_motion_rows = support::max_directional_motion_rows(previous, next, config);
		let fallback_range = Some(OverlapSearchRange { start: 1, end: max_motion_rows });
		let fallback_eval = self.diagnose_reference_overlap_direction_with_preferred_range(
			previous,
			next,
			ScrollDirection::Up,
			fallback_range,
			false,
		);

		if fallback_eval.final_match.is_some() { fallback_eval } else { hinted_eval }
	}

	fn log_upward_input_diagnostics(
		&self,
		diagnostics: &UpwardInputDiagnostics,
		sample_delta: Option<u32>,
		sample_motion: Option<MotionObservation>,
		frame: &RgbaImage,
	) {
		self.log_up_input_match_eval(UpInputMatchLog {
			sample_motion,
			sample_down_match: diagnostics.sample_down_match_eval.final_match,
			sample_up_match: diagnostics.sample_up_match_eval.final_match,
			committed_down_match: diagnostics.committed_down_match_eval.final_match,
			committed_up_match: diagnostics.committed_up_match_eval.final_match,
			sample_override_wins: diagnostics.sample_override_match.is_some(),
			committed_override_wins: diagnostics.committed_override_match.is_some(),
		});
		self.log_up_input_search_window_eval(UpInputSearchWindowLog {
			sample_delta,
			sample_down_match_eval: &diagnostics.sample_down_match_eval,
			sample_up_match_eval: &diagnostics.sample_up_match_eval,
			committed_down_match_eval: &diagnostics.committed_down_match_eval,
			committed_up_match_eval: &diagnostics.committed_up_match_eval,
			frame_equals_last_sample: *frame == self.last_sample_frame,
			frame_equals_last_committed: *frame == self.last_committed_frame,
		});
	}

	fn arm_upward_rewind_with_match(
		&mut self,
		frame: &RgbaImage,
		fingerprint: Vec<u8>,
		up_match: DirectionMatch,
		from_committed: bool,
		op: &'static str,
		block_reason: &'static str,
	) -> ScrollObserveOutcome {
		self.last_unconfirmed_upward_fingerprint = None;

		self.record_last_sample(frame, fingerprint);

		if from_committed {
			self.observe_upward_rewind_from_committed(up_match.motion_rows);
		} else {
			self.observe_upward_rewind(up_match.motion_rows);
		}

		self.log_decision(
			op,
			ScrollDirection::Up,
			Some(MotionObservation {
				direction: ScrollDirection::Up,
				motion_rows: up_match.motion_rows,
			}),
			None,
			None,
			Some(block_reason),
		);

		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
	}

	fn arm_unconfirmed_upward_rewind(
		&mut self,
		frame: &RgbaImage,
		fingerprint: Vec<u8>,
		detected_motion: Option<MotionObservation>,
		refresh_sample: bool,
		op: &'static str,
		block_reason: &'static str,
	) -> ScrollObserveOutcome {
		if self.current_viewport_top_y <= 0 && self.resume_frontier_top_y.is_none() {
			if refresh_sample {
				self.last_unconfirmed_upward_fingerprint = None;

				self.record_last_sample(frame, fingerprint);
			}

			return ScrollObserveOutcome::PreviewUpdated;
		}
		if refresh_sample {
			self.last_unconfirmed_upward_fingerprint = None;

			self.record_last_sample(frame, fingerprint);
		} else {
			self.last_unconfirmed_upward_fingerprint = Some(fingerprint.clone());
		}

		self.observe_unconfirmed_upward_rewind();
		self.log_decision(op, ScrollDirection::Up, detected_motion, None, None, Some(block_reason));

		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
	}
}
