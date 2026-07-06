use color_eyre::eyre;
use image::RgbaImage;

use crate::scroll_capture::{
	self, INITIAL_DOWNWARD_MAX_MOTION_ROWS, MotionObservation, ScrollDirection,
	ScrollObserveOutcome, ScrollSession, pairwise_shift,
};

const WORKER_PAIRWISE_CORROBORATION_TOLERANCE_ROWS: u32 = 24;
const WORKER_PAIRWISE_COMMITTED_CATCHUP_MIN_MOTION_ROWS: u32 = 24;

impl ScrollSession {
	pub(crate) fn observe_worker_pairwise_frame(
		&mut self,
		frame: RgbaImage,
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
		self.clear_last_downward_sample_registration();

		if frame.width() != self.anchor_frame.width() {
			return Err(eyre::eyre!(
				"frame width mismatch: expected {} got {}",
				self.anchor_frame.width(),
				frame.width()
			));
		}

		let fingerprint = scroll_capture::scroll_capture_fingerprint(&frame);
		let previous_worker_frame = self.worker_pairwise_previous_frame.clone();

		if self.worker_pairwise_requires_committed_reacquire {
			if frame == self.last_committed_frame {
				self.worker_pairwise_requires_committed_reacquire = false;

				return Ok(self.observe_worker_pairwise_no_change(
					frame,
					fingerprint,
					"worker_pairwise_reacquired_last_committed_frame",
				));
			}

			if let Some(motion_rows) = self.worker_pairwise_committed_catchup_motion_rows(&frame) {
				self.worker_pairwise_requires_committed_reacquire = false;

				return self.observe_resolved_worker_pairwise_downward_motion(
					frame,
					fingerprint,
					motion_rows,
					Some(motion_rows),
				);
			}

			return Ok(self.block_worker_pairwise_until_committed_reacquire(frame, fingerprint));
		}
		if frame == previous_worker_frame {
			let reason = if frame == self.last_committed_frame {
				"frame_matches_last_committed_frame"
			} else {
				"frame_matches_worker_pairwise_previous_frame"
			};

			return Ok(self.observe_worker_pairwise_no_change(frame, fingerprint, reason));
		}

		let (matched, corroborated_shift_rows) = if let Some(matched) =
			pairwise_shift::trusted_pairwise_downward_shift_match(&previous_worker_frame, &frame)
		{
			let max_pixel_fallback_motion_rows =
				previous_worker_frame.height().saturating_div(2).max(1);

			if matched.motion_rows > max_pixel_fallback_motion_rows {
				let candidate_viewport_top_y = self
					.current_viewport_top_y
					.saturating_add(i32::try_from(matched.motion_rows).unwrap_or_default());
				let growth_rows =
					self.growth_rows_for_candidate_viewport_top_y(candidate_viewport_top_y);

				return Ok(self.block_worker_pairwise_growth(
					frame,
					fingerprint,
					matched.motion_rows,
					candidate_viewport_top_y,
					growth_rows,
					"worker_pairwise_pixel_overlap_exceeded_fallback_budget",
				));
			}

			(matched, Some(matched.motion_rows))
		} else if let Some(matched) =
			pairwise_shift::classify_pairwise_downward_sample_motion_against(
				&previous_worker_frame,
				&frame,
			) {
			(
				matched,
				pairwise_shift::trusted_pairwise_downward_shift_rows_near_motion(
					&previous_worker_frame,
					&frame,
					matched.motion_rows,
					WORKER_PAIRWISE_CORROBORATION_TOLERANCE_ROWS,
				),
			)
		} else {
			if let Some(upward_motion_rows) =
				pairwise_shift::trusted_pairwise_upward_shift_rows(&previous_worker_frame, &frame)
			{
				return Ok(self.observe_worker_pairwise_upward_motion(
					frame,
					fingerprint,
					upward_motion_rows,
				));
			}
			if let Some(motion_rows) = self.worker_pairwise_committed_catchup_motion_rows(&frame) {
				return self.observe_resolved_worker_pairwise_downward_motion(
					frame,
					fingerprint,
					motion_rows,
					Some(motion_rows),
				);
			}

			return Ok(self.observe_worker_pairwise_no_change(
				frame,
				fingerprint,
				"worker_pairwise_no_downward_offset",
			));
		};

		self.observe_resolved_worker_pairwise_downward_motion(
			frame,
			fingerprint,
			matched.motion_rows,
			corroborated_shift_rows,
		)
	}

	pub(crate) fn observe_worker_pairwise_frame_with_motion_hint(
		&mut self,
		frame: RgbaImage,
		motion_rows_hint: Option<u32>,
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
		let previous_hint = self.transient_motion_rows_hint;

		self.transient_motion_rows_hint = motion_rows_hint;

		self.record_last_sample_eval_context();

		let result = self.observe_worker_pairwise_frame(frame);

		self.transient_motion_rows_hint = previous_hint;

		result
	}

	fn worker_pairwise_committed_catchup_motion_rows(&self, frame: &RgbaImage) -> Option<u32> {
		if frame == &self.last_committed_frame {
			return None;
		}

		let hinted_match = self.normalized_transient_motion_rows_hint().and_then(|hint| {
			let tolerance = (hint / 2).clamp(
				WORKER_PAIRWISE_CORROBORATION_TOLERANCE_ROWS,
				INITIAL_DOWNWARD_MAX_MOTION_ROWS,
			);

			pairwise_shift::trusted_pairwise_downward_shift_rows_near_motion(
				&self.last_committed_frame,
				frame,
				hint,
				tolerance,
			)
		});
		let fallback_match = || {
			pairwise_shift::trusted_pairwise_downward_shift_match(&self.last_committed_frame, frame)
				.map(|matched| matched.motion_rows)
		};

		hinted_match
			.or_else(fallback_match)
			.filter(|motion_rows| *motion_rows >= WORKER_PAIRWISE_COMMITTED_CATCHUP_MIN_MOTION_ROWS)
	}

	fn observe_resolved_worker_pairwise_downward_motion(
		&mut self,
		frame: RgbaImage,
		fingerprint: Vec<u8>,
		pairwise_motion_rows: u32,
		corroborated_shift_rows: Option<u32>,
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
		let effective_motion_rows = match Self::resolve_worker_pairwise_motion_rows(
			pairwise_motion_rows,
			corroborated_shift_rows,
		) {
			Ok(motion_rows) => motion_rows,
			Err(block_reason) => {
				return Ok(self.block_worker_pairwise_growth(
					frame,
					fingerprint,
					pairwise_motion_rows,
					self.current_viewport_top_y,
					0,
					block_reason,
				));
			},
		};

		tracing::debug!(
			op = "scroll_capture.worker_pairwise_motion_resolved",
			pairwise_motion_rows,
			corroborated_motion_rows = corroborated_shift_rows,
			effective_motion_rows,
			current_viewport_top_y = self.current_viewport_top_y,
			observed_viewport_top_y = self.observed_viewport_top_y,
			"Scroll-capture worker pairwise motion resolved against pixel overlap."
		);

		if effective_motion_rows == 0 {
			return Ok(self.block_worker_pairwise_growth(
				frame,
				fingerprint,
				effective_motion_rows,
				self.current_viewport_top_y,
				0,
				"worker_pairwise_zero_effective_motion",
			));
		}

		let candidate_viewport_top_y = self
			.current_viewport_top_y
			.saturating_add(i32::try_from(effective_motion_rows).unwrap_or_default());
		let growth_rows = self.growth_rows_for_candidate_viewport_top_y(candidate_viewport_top_y);
		let frame_max_growth_rows = frame.height().saturating_sub(1).max(1);

		if growth_rows == 0 || growth_rows > frame_max_growth_rows {
			return Ok(self.block_worker_pairwise_growth(
				frame,
				fingerprint,
				effective_motion_rows,
				candidate_viewport_top_y,
				growth_rows,
				"worker_pairwise_growth_exceeded_frame_bounds",
			));
		}

		self.log_decision(
			"scroll_capture.worker_pairwise_growth_candidate",
			ScrollDirection::Down,
			Some(MotionObservation {
				direction: ScrollDirection::Down,
				motion_rows: effective_motion_rows,
			}),
			Some(candidate_viewport_top_y),
			Some(growth_rows),
			Some("worker_pairwise"),
		);

		self.worker_pairwise_previous_frame = frame.clone();
		self.worker_pairwise_requires_committed_reacquire = false;

		self.clear_preview_only_downward_recovery_carryover();

		if self.resume_frontier_top_y.is_some() {
			let outcome = self.observe_downward_motion_while_resume_frontier_active(
				frame.clone(),
				effective_motion_rows,
				true,
			)?;

			if !matches!(
				outcome,
				ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, .. }
			) {
				self.record_last_sample(&frame, fingerprint);
			}

			return Ok(outcome);
		}

		self.apply_growth(
			frame.clone(),
			growth_rows,
			candidate_viewport_top_y,
			"worker_pairwise",
			Some(pairwise_motion_rows),
			Some(effective_motion_rows),
			None,
		)
	}

	fn observe_worker_pairwise_no_change(
		&mut self,
		frame: RgbaImage,
		fingerprint: Vec<u8>,
		reason: &'static str,
	) -> ScrollObserveOutcome {
		if reason == "worker_pairwise_no_downward_offset" && frame != self.last_committed_frame {
			self.record_last_sample(&frame, fingerprint);
			self.clear_preview_only_downward_recovery_carryover();

			self.worker_pairwise_requires_committed_reacquire = true;

			self.log_decision(
				"scroll_capture.worker_pairwise_no_change",
				ScrollDirection::Down,
				None,
				Some(self.observed_viewport_top_y),
				Some(0),
				Some(reason),
			);

			return ScrollObserveOutcome::NoChange;
		}

		self.update_worker_pairwise_reference_frame(frame, fingerprint);
		self.log_decision(
			"scroll_capture.worker_pairwise_no_change",
			ScrollDirection::Down,
			None,
			Some(self.observed_viewport_top_y),
			Some(0),
			Some(reason),
		);

		ScrollObserveOutcome::NoChange
	}

	fn observe_worker_pairwise_upward_motion(
		&mut self,
		frame: RgbaImage,
		fingerprint: Vec<u8>,
		motion_rows: u32,
	) -> ScrollObserveOutcome {
		self.record_last_sample(&frame, fingerprint);
		self.clear_preview_only_downward_recovery_carryover();

		if self.current_viewport_top_y <= 0 && self.resume_frontier_top_y.is_none() {
			if frame != self.last_committed_frame {
				self.worker_pairwise_requires_committed_reacquire = true;
			}

			self.log_decision(
				"scroll_capture.worker_pairwise_upward_at_top",
				ScrollDirection::Up,
				Some(MotionObservation { direction: ScrollDirection::Up, motion_rows }),
				Some(self.current_viewport_top_y),
				Some(0),
				Some("worker_pairwise_upward_motion_without_committed_growth"),
			);

			return ScrollObserveOutcome::PreviewUpdated;
		}

		self.worker_pairwise_previous_frame = frame.clone();

		self.observe_upward_rewind(motion_rows);
		self.log_decision(
			"scroll_capture.worker_pairwise_rewind_armed",
			ScrollDirection::Up,
			Some(MotionObservation { direction: ScrollDirection::Up, motion_rows }),
			None,
			None,
			Some("worker_pairwise_detected_upward_motion"),
		);

		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
	}

	fn block_worker_pairwise_growth(
		&mut self,
		frame: RgbaImage,
		fingerprint: Vec<u8>,
		motion_rows: u32,
		candidate_viewport_top_y: i32,
		growth_rows: u32,
		reason: &'static str,
	) -> ScrollObserveOutcome {
		self.record_last_sample(&frame, fingerprint);
		self.clear_preview_only_downward_recovery_carryover();

		if motion_rows > 0 {
			self.worker_pairwise_requires_committed_reacquire = true;
		}

		self.log_decision(
			"scroll_capture.worker_pairwise_growth_blocked",
			ScrollDirection::Down,
			Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
			Some(candidate_viewport_top_y),
			Some(growth_rows),
			Some(reason),
		);

		ScrollObserveOutcome::NoChange
	}

	fn block_worker_pairwise_until_committed_reacquire(
		&mut self,
		frame: RgbaImage,
		fingerprint: Vec<u8>,
	) -> ScrollObserveOutcome {
		self.record_last_sample(&frame, fingerprint);
		self.clear_preview_only_downward_recovery_carryover();
		self.log_decision(
			"scroll_capture.worker_pairwise_growth_blocked",
			ScrollDirection::Down,
			None,
			Some(self.current_viewport_top_y),
			Some(0),
			Some("worker_pairwise_requires_committed_reacquire_after_blocked_gap"),
		);

		ScrollObserveOutcome::NoChange
	}

	pub(crate) fn resolve_worker_pairwise_motion_rows(
		pairwise_motion_rows: u32,
		corroborated_shift_rows: Option<u32>,
	) -> std::result::Result<u32, &'static str> {
		let Some(corroborated_shift_rows) = corroborated_shift_rows else {
			return Err("worker_pairwise_missing_or_ambiguous_overlap_corroboration");
		};

		if corroborated_shift_rows == 0 {
			return Err("worker_pairwise_zero_overlap_corroboration");
		}
		if pairwise_motion_rows.abs_diff(corroborated_shift_rows)
			> WORKER_PAIRWISE_CORROBORATION_TOLERANCE_ROWS
		{
			return Err("worker_pairwise_overlap_motion_mismatch");
		}

		Ok(corroborated_shift_rows)
	}

	fn update_worker_pairwise_reference_frame(&mut self, frame: RgbaImage, fingerprint: Vec<u8>) {
		self.record_last_sample(&frame, fingerprint.clone());
		self.record_last_downward_observed_sample(&frame, fingerprint);

		self.worker_pairwise_previous_frame = frame;

		self.clear_preview_only_downward_recovery_carryover();
	}
}
