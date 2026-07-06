use color_eyre::eyre::Result;
use image::RgbaImage;

use crate::scroll_capture::{
	DIRECTION_WARNING_MARGIN_X100, DirectionMatch, MotionObservation,
	ResumeFrontierDirectMatchContext, ResumeFrontierMatchLog, ScrollDirection,
	ScrollObserveOutcome, ScrollSession, support,
};

impl ScrollSession {
	pub(super) fn observe_downward_motion_while_resume_frontier_active(
		&mut self,
		frame: RgbaImage,
		motion_rows: u32,
		preview_changed: bool,
	) -> Result<ScrollObserveOutcome> {
		let candidate_observed_viewport_top_y = self
			.observed_viewport_top_y
			.saturating_add(i32::try_from(motion_rows).unwrap_or_default());
		let Some(resume_frontier_top_y) = self.resume_frontier_top_y else {
			return Ok(support::preview_update_outcome(preview_changed));
		};
		let frame_reacquires_last_committed_viewport =
			self.frame_reacquires_last_committed_viewport(&frame);

		if let Some(outcome) = self.handle_resume_frontier_reacquire_block(
			motion_rows,
			preview_changed,
			resume_frontier_top_y,
			frame_reacquires_last_committed_viewport,
		) {
			return Ok(outcome);
		}

		let match_context = ResumeFrontierDirectMatchContext {
			motion_rows,
			candidate_observed_viewport_top_y,
			residual_growth_rows: self
				.growth_rows_for_candidate_viewport_top_y(candidate_observed_viewport_top_y),
		};

		if self.resume_frontier_requires_reacquire {
			if let Some(outcome) = self.block_resume_frontier_before_growth(
				motion_rows,
				preview_changed,
				resume_frontier_top_y,
				candidate_observed_viewport_top_y,
				&frame,
			) {
				return Ok(outcome);
			}

			return self.resolve_resume_frontier_direct_match(
				frame,
				preview_changed,
				frame_reacquires_last_committed_viewport,
				match_context,
			);
		}

		if let Some(outcome) = self.block_resume_frontier_before_growth(
			motion_rows,
			preview_changed,
			resume_frontier_top_y,
			candidate_observed_viewport_top_y,
			&frame,
		) {
			return Ok(outcome);
		}

		self.observed_viewport_top_y = resume_frontier_top_y;

		if match_context.residual_growth_rows == 0 {
			self.log_decision(
				"scroll_capture.resume_frontier_still_blocked",
				ScrollDirection::Down,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("resume_active_candidate_reached_frontier_without_residual_growth"),
			);

			return Ok(support::preview_update_outcome(preview_changed));
		}

		self.resolve_resume_frontier_direct_match(
			frame,
			preview_changed,
			frame_reacquires_last_committed_viewport,
			match_context,
		)
	}

	fn handle_resume_frontier_reacquire_block(
		&mut self,
		motion_rows: u32,
		preview_changed: bool,
		resume_frontier_top_y: i32,
		frame_reacquires_last_committed_viewport: bool,
	) -> Option<ScrollObserveOutcome> {
		if !self.resume_frontier_requires_reacquire {
			return None;
		}
		if !frame_reacquires_last_committed_viewport {
			return None;
		}

		self.resume_frontier_requires_reacquire = false;
		self.observed_viewport_top_y = resume_frontier_top_y;

		self.log_decision(
			"scroll_capture.resume_frontier_still_blocked",
			ScrollDirection::Down,
			Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
			Some(self.observed_viewport_top_y),
			Some(0),
			Some("resume_active_reacquired_last_committed_frame"),
		);

		Some(support::preview_update_outcome(preview_changed))
	}

	fn block_resume_frontier_before_growth(
		&mut self,
		motion_rows: u32,
		preview_changed: bool,
		resume_frontier_top_y: i32,
		candidate_observed_viewport_top_y: i32,
		frame: &RgbaImage,
	) -> Option<ScrollObserveOutcome> {
		if frame == &self.last_committed_frame {
			self.observed_viewport_top_y = resume_frontier_top_y;

			self.log_decision(
				"scroll_capture.resume_frontier_still_blocked",
				ScrollDirection::Down,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("resume_active_frame_matches_last_committed_frame"),
			);

			return Some(support::preview_update_outcome(preview_changed));
		}
		if self.resume_frontier_requires_reacquire {
			return None;
		}
		if candidate_observed_viewport_top_y < resume_frontier_top_y {
			self.observed_viewport_top_y = candidate_observed_viewport_top_y;

			self.log_decision(
				"scroll_capture.resume_frontier_still_blocked",
				ScrollDirection::Down,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("resume_active_candidate_observed_viewport_still_below_frontier"),
			);

			return Some(support::preview_update_outcome(preview_changed));
		}

		None
	}

	fn blocked_resume_frontier_observed_viewport_top_y(
		&self,
		candidate_observed_viewport_top_y: i32,
		preserve_candidate_progress: bool,
	) -> i32 {
		if preserve_candidate_progress || !self.resume_frontier_requires_reacquire {
			return candidate_observed_viewport_top_y;
		}

		self.resume_frontier_top_y.map_or(
			candidate_observed_viewport_top_y,
			|resume_frontier_top_y| {
				candidate_observed_viewport_top_y.min(resume_frontier_top_y.saturating_sub(1))
			},
		)
	}

	fn resolve_resume_frontier_direct_match(
		&mut self,
		frame: RgbaImage,
		preview_changed: bool,
		frame_reacquires_last_committed_viewport: bool,
		context: ResumeFrontierDirectMatchContext,
	) -> Result<ScrollObserveOutcome> {
		let direct_match_hint_rows = Some(self.resume_frontier_direct_match_hint_rows(context));
		let raw_committed_down_match = self.evaluate_reference_overlap_direction_preferred_only(
			&self.last_committed_frame,
			&frame,
			ScrollDirection::Down,
			direct_match_hint_rows,
		);
		let trusted_committed_down_match = raw_committed_down_match
			.filter(|matched| support::resume_direct_match_is_trustworthy(*matched));
		let committed_up_match = self.evaluate_reference_overlap_direction_preferred_only(
			&self.last_committed_frame,
			&frame,
			ScrollDirection::Up,
			direct_match_hint_rows,
		);

		self.log_resume_frontier_match_eval(ResumeFrontierMatchLog {
			motion_rows: context.motion_rows,
			candidate_observed_viewport_top_y: context.candidate_observed_viewport_top_y,
			residual_growth_rows: context.residual_growth_rows,
			raw_committed_down_match,
			trusted_committed_down_match,
			committed_up_match,
			frame_reacquires_last_committed_viewport,
		});

		let preserve_candidate_progress = self.resume_frontier_should_preserve_blocked_progress(
			&frame,
			context,
			committed_up_match,
		);

		match (trusted_committed_down_match, committed_up_match) {
			(Some(down), Some(up))
				if down.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					< up.mean_abs_diff_x100 =>
			{
				self.resume_frontier_commit_direct_match(frame, preview_changed, down, context)
			},
			(Some(down), Some(up))
				if up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					<= down.mean_abs_diff_x100 =>
			{
				Ok(self.block_resume_frontier_direct_match(
					context,
					preview_changed,
					false,
					MotionObservation {
						direction: ScrollDirection::Up,
						motion_rows: up.motion_rows,
					},
					"resume_active_sample_motion_matched_above_committed_frontier",
				))
			},
			(Some(_down), Some(_up)) => Ok(self.block_resume_frontier_direct_match(
				context,
				preview_changed,
				false,
				MotionObservation {
					direction: ScrollDirection::Down,
					motion_rows: context.motion_rows,
				},
				"resume_active_direct_committed_match_ambiguous",
			)),
			(Some(down), None) => {
				self.resume_frontier_commit_direct_match(frame, preview_changed, down, context)
			},
			(None, Some(up)) => Ok(self.block_resume_frontier_direct_match(
				context,
				preview_changed,
				false,
				MotionObservation { direction: ScrollDirection::Up, motion_rows: up.motion_rows },
				"resume_active_direct_committed_match_still_above_frontier",
			)),
			(None, None) => Ok(self.block_resume_frontier_without_direct_match(
				context,
				preview_changed,
				preserve_candidate_progress,
				raw_committed_down_match.is_some(),
			)),
		}
	}

	fn block_resume_frontier_direct_match(
		&mut self,
		context: ResumeFrontierDirectMatchContext,
		preview_changed: bool,
		preserve_candidate_progress: bool,
		detected_motion: MotionObservation,
		block_reason: &'static str,
	) -> ScrollObserveOutcome {
		self.observed_viewport_top_y = self.blocked_resume_frontier_observed_viewport_top_y(
			context.candidate_observed_viewport_top_y,
			preserve_candidate_progress,
		);

		self.log_decision(
			"scroll_capture.resume_frontier_still_blocked",
			ScrollDirection::Down,
			Some(detected_motion),
			Some(self.observed_viewport_top_y),
			Some(0),
			Some(block_reason),
		);

		support::preview_update_outcome(preview_changed)
	}

	fn block_resume_frontier_without_direct_match(
		&mut self,
		context: ResumeFrontierDirectMatchContext,
		preview_changed: bool,
		preserve_candidate_progress: bool,
		has_raw_committed_down_match: bool,
	) -> ScrollObserveOutcome {
		let block_reason = if has_raw_committed_down_match {
			"resume_active_direct_committed_match_too_weak"
		} else {
			"resume_active_direct_committed_match_not_ready"
		};

		self.block_resume_frontier_direct_match(
			context,
			preview_changed,
			preserve_candidate_progress,
			MotionObservation {
				direction: ScrollDirection::Down,
				motion_rows: context.residual_growth_rows,
			},
			block_reason,
		)
	}

	fn resume_frontier_should_preserve_blocked_progress(
		&self,
		frame: &RgbaImage,
		context: ResumeFrontierDirectMatchContext,
		committed_up_match: Option<DirectionMatch>,
	) -> bool {
		if !self.resume_frontier_requires_reacquire || context.residual_growth_rows == 0 {
			return false;
		}
		if committed_up_match.is_some() {
			return false;
		}

		let sample_down_match = self.evaluate_reference_overlap_direction_preferred_only(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Down,
			Some(context.motion_rows),
		);
		let sample_up_match = self.evaluate_reference_overlap_direction_preferred_only(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Up,
			Some(context.motion_rows),
		);

		matches!(
			(sample_down_match, sample_up_match),
			(Some(down), Some(up))
				if down.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					< up.mean_abs_diff_x100
		) || matches!((sample_down_match, sample_up_match), (Some(_), None))
	}

	fn resume_frontier_direct_match_hint_rows(
		&self,
		context: ResumeFrontierDirectMatchContext,
	) -> u32 {
		if !self.resume_frontier_requires_reacquire {
			return context.residual_growth_rows;
		}
		if context.residual_growth_rows > 0 {
			return context.residual_growth_rows;
		}

		context.motion_rows
	}

	fn resume_frontier_commit_direct_match(
		&mut self,
		frame: RgbaImage,
		preview_changed: bool,
		down: DirectionMatch,
		context: ResumeFrontierDirectMatchContext,
	) -> Result<ScrollObserveOutcome> {
		let candidate_viewport_top_y = if self.resume_frontier_requires_reacquire {
			let resume_frontier_top_y =
				self.resume_frontier_top_y.unwrap_or(self.current_viewport_top_y);

			resume_frontier_top_y
				.saturating_add(i32::try_from(down.motion_rows).unwrap_or_default())
		} else {
			let growth_rows = down.motion_rows.min(context.residual_growth_rows);

			self.current_viewport_top_y
				.saturating_add(i32::try_from(growth_rows).unwrap_or_default())
		};

		self.observe_downward_growth_to_viewport(
			frame,
			candidate_viewport_top_y,
			preview_changed,
			Some(MotionObservation {
				direction: ScrollDirection::Down,
				motion_rows: down.motion_rows,
			}),
			"resume_active_direct_committed_frontier_match",
		)
	}

	fn frame_reacquires_last_committed_viewport(&self, frame: &RgbaImage) -> bool {
		frame == &self.last_committed_frame
	}
}
