use std::ops::RangeInclusive;

use image::RgbaImage;

use crate::scroll_capture::support;
use crate::scroll_capture::{
	BOOTSTRAP_HINTED_INITIAL_GROWTH_MAX_ROWS, DIRECTION_WARNING_MARGIN_X100,
	DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS, DirectionMatch, DownwardRegistration,
	DownwardRegistrationWithSource, DownwardSampleMatch, DownwardSampleMatchSource,
	DownwardViewportCandidate, DownwardViewportCandidateSource,
	EXTREME_TRANSIENT_PREVIEW_LOCAL_TAIL_MULTIPLIER, INITIAL_DOWNWARD_MAX_MOTION_ROWS,
	MotionObservation, OverlapSearchConfig, PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS,
	PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS, PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS,
	ScrollDirection, ScrollSession, TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MAX_MOTION_ROWS,
	TRANSIENT_MOTION_HINT_MAX_MULTIPLIER, TRANSIENT_MOTION_HINT_MIN_CAP_ROWS,
	UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS,
};

impl ScrollSession {
	pub(super) fn classify_sample_motion(&self, frame: &RgbaImage) -> Option<MotionObservation> {
		let effective_motion_rows_hint = self.effective_motion_rows_hint();
		let down_match = self.evaluate_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Down,
			effective_motion_rows_hint,
		);
		let up_match = self.evaluate_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Up,
			effective_motion_rows_hint,
		);

		match (down_match, up_match) {
			(Some(down), Some(up)) => {
				if down.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					< up.mean_abs_diff_x100
				{
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: down.motion_rows,
					})
				} else if up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					<= down.mean_abs_diff_x100
				{
					Some(MotionObservation {
						direction: ScrollDirection::Up,
						motion_rows: up.motion_rows,
					})
				} else {
					None
				}
			},
			(Some(down), None) => Some(MotionObservation {
				direction: ScrollDirection::Down,
				motion_rows: down.motion_rows,
			}),
			(None, Some(up)) => Some(MotionObservation {
				direction: ScrollDirection::Up,
				motion_rows: up.motion_rows,
			}),
			(None, None) => None,
		}
	}

	pub(super) fn classify_downward_sample_motion(
		&self,
		frame: &RgbaImage,
	) -> (DownwardRegistration, Option<&'static str>) {
		let previous = if self.initial_downward_bootstrap_active() {
			&self.last_sample_frame
		} else {
			&self.last_downward_observed_frame
		};

		self.classify_downward_sample_motion_against(previous, frame)
	}

	pub(super) fn classify_downward_sample_motion_with_local_recovery(
		&mut self,
		frame: &RgbaImage,
	) -> DownwardRegistrationWithSource {
		let (primary_raw, primary_reason) = self.classify_downward_sample_motion(frame);
		let primary = primary_raw.map_source(DownwardSampleMatchSource::ObservedSample);

		self.record_registration_diagnostics(
			DownwardSampleMatchSource::ObservedSample,
			primary,
			primary_reason,
		);

		let Some(previous_local) = self.last_preview_only_downward_local_sample.as_ref() else {
			return primary;
		};
		let (local_raw, local_reason) =
			self.classify_preview_only_local_recovery_motion_against(&previous_local.frame, frame);
		let local = local_raw.map_source(DownwardSampleMatchSource::PreviewOnlyLocalSample);

		self.record_registration_diagnostics(
			DownwardSampleMatchSource::PreviewOnlyLocalSample,
			local,
			local_reason,
		);

		match (primary, local) {
			(
				DownwardRegistrationWithSource::Matched(primary),
				DownwardRegistrationWithSource::Matched(local),
			) => {
				let prefer_local =
					self.should_prefer_preview_only_local_recovery_after_extreme_tail_block(
						primary, local,
					) || (!self.should_prefer_observed_sample_over_preview_only_local_recovery(
						primary, local,
					) && (self
						.should_prefer_preview_only_local_recovery_over_observed_sample(
							primary, local,
						) || local.matched.mean_abs_diff_x100
						<= primary.matched.mean_abs_diff_x100));

				if prefer_local {
					DownwardRegistrationWithSource::Matched(local)
				} else {
					DownwardRegistrationWithSource::Matched(primary)
				}
			},
			(DownwardRegistrationWithSource::Matched(primary), _) => {
				DownwardRegistrationWithSource::Matched(primary)
			},
			(_, DownwardRegistrationWithSource::Matched(local)) => {
				DownwardRegistrationWithSource::Matched(local)
			},
			(primary, _) => primary,
		}
	}

	pub(super) fn should_prefer_observed_sample_over_preview_only_local_recovery(
		&self,
		primary: DownwardSampleMatch,
		local: DownwardSampleMatch,
	) -> bool {
		let small_local_recovery_lags_recent_continuity =
			self.last_motion_rows_hint.is_some_and(|last_hint| {
				last_hint >= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
					&& local.matched.motion_rows
						<= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS.saturating_div(2)
					&& local.matched.motion_rows
						< last_hint.saturating_sub(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
			});

		small_local_recovery_lags_recent_continuity
			&& self.transient_burst_motion_hint_exceeds_local_authority(local.matched.motion_rows)
			&& primary.matched.motion_rows
				> local
					.matched
					.motion_rows
					.saturating_add(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
			&& self.last_motion_rows_hint.is_some_and(|last_hint| {
				primary
					.matched
					.motion_rows
					.saturating_add(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
					>= last_hint && primary.matched.motion_rows <= last_hint
			}) && self
			.transient_pending_growth_cap_rows()
			.is_some_and(|cap| primary.matched.motion_rows <= cap)
	}

	pub(super) fn should_prefer_preview_only_local_recovery_after_extreme_tail_block(
		&self,
		primary: DownwardSampleMatch,
		local: DownwardSampleMatch,
	) -> bool {
		let Some(pending_candidate) = self.pending_extreme_preview_only_local_tail_followup else {
			return false;
		};
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		let Some(transient_motion_rows_hint) = self.normalized_transient_motion_rows_hint() else {
			return false;
		};

		primary.source == DownwardSampleMatchSource::ObservedSample
			&& local.source == DownwardSampleMatchSource::PreviewOnlyLocalSample
			&& primary.matched.motion_rows == pending_candidate.motion_rows
			&& local.matched.motion_rows >= last_motion_rows_hint
			&& local.matched.motion_rows < primary.matched.motion_rows
			&& local.matched.motion_rows <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			&& transient_motion_rows_hint
				>= last_motion_rows_hint
					.saturating_mul(EXTREME_TRANSIENT_PREVIEW_LOCAL_TAIL_MULTIPLIER)
	}

	pub(super) fn should_prefer_preview_only_local_recovery_over_observed_sample(
		&self,
		primary: DownwardSampleMatch,
		local: DownwardSampleMatch,
	) -> bool {
		self.transient_burst_search_enabled
			&& self.last_motion_rows_hint.is_some_and(|last_hint| {
				last_hint <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
					&& (local.matched.motion_rows >= last_hint
						|| (local.matched.motion_rows
							<= TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MAX_MOTION_ROWS
							&& self.consecutive_transient_burst_missing_downward_candidate_frames
								>= 2)) && local.matched.motion_rows
					<= last_hint.saturating_add(PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS)
					&& primary.matched.motion_rows
						> local
							.matched
							.motion_rows
							.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS)
			}) || self
			.preview_only_local_slowdown_tail_followup_can_prefer_observed_override(primary, local)
	}

	pub(super) fn preview_only_local_slowdown_tail_followup_can_prefer_observed_override(
		&self,
		primary: DownwardSampleMatch,
		local: DownwardSampleMatch,
	) -> bool {
		self.transient_burst_search_enabled
			&& self.last_preview_only_downward_local_sample.is_some()
			&& local.source == DownwardSampleMatchSource::PreviewOnlyLocalSample
			&& primary.source == DownwardSampleMatchSource::ObservedSample
			&& self.last_motion_rows_hint.is_some_and(|last_hint| {
				let tiny_followup = last_hint <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
					&& local.matched.motion_rows
						<= TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MAX_MOTION_ROWS;
				let near_continuity_followup = last_hint
					<= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
					&& local.matched.motion_rows
						<= last_hint.saturating_add(PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS);

				(tiny_followup || near_continuity_followup)
					&& primary.matched.motion_rows
						> local
							.matched
							.motion_rows
							.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS)
			}) && self.growth_history.last().is_some_and(|commit| {
			commit.decision_source
				== DownwardViewportCandidateSource::PreviewOnlyLocalSample.decision_source()
				&& self
					.last_motion_rows_hint
					.is_some_and(|last_hint| commit.growth_rows <= last_hint)
		})
	}

	pub(super) fn record_registration_diagnostics(
		&mut self,
		source: DownwardSampleMatchSource,
		registration: DownwardRegistrationWithSource,
		reason: Option<&'static str>,
	) {
		let (result, motion_rows, mean_abs_diff_x100) = match registration {
			DownwardRegistrationWithSource::NoMatch => ("no_match", None, None),
			DownwardRegistrationWithSource::Matched(matched) => (
				"matched",
				Some(matched.matched.motion_rows),
				Some(matched.matched.mean_abs_diff_x100),
			),
			DownwardRegistrationWithSource::Ambiguous { best, .. } => {
				("ambiguous", Some(best.matched.motion_rows), Some(best.matched.mean_abs_diff_x100))
			},
		};

		match source {
			DownwardSampleMatchSource::ObservedSample => {
				self.last_observed_sample_registration_result = Some(result);
				self.last_observed_sample_registration_reason = reason;
				self.last_observed_sample_registration_motion_rows = motion_rows;
				self.last_observed_sample_registration_mean_abs_diff_x100 = mean_abs_diff_x100;
			},
			DownwardSampleMatchSource::PreviewOnlyLocalSample => {
				self.last_preview_only_local_registration_result = Some(result);
				self.last_preview_only_local_registration_reason = reason;
				self.last_preview_only_local_registration_motion_rows = motion_rows;
				self.last_preview_only_local_registration_mean_abs_diff_x100 = mean_abs_diff_x100;
			},
		}
	}

	pub(super) fn classify_downward_sample_motion_against(
		&self,
		previous: &RgbaImage,
		frame: &RgbaImage,
	) -> (DownwardRegistration, Option<&'static str>) {
		let config = OverlapSearchConfig::default();
		let preferred_ranges = self.sequential_downward_motion_ranges(previous, frame, config);
		let (registration, reason) = self
			.evaluate_reference_downward_registration_with_preferred_ranges(
				previous,
				frame,
				self.last_motion_rows_hint,
				&preferred_ranges,
				self.transient_burst_search_enabled,
			);

		match registration {
			DownwardRegistration::Matched(matched)
				if self.bootstrap_motion_exceeds_pending_hint(matched.motion_rows)
					&& !self.bootstrap_hint_exceeded_match_can_commit(matched) =>
			{
				(DownwardRegistration::NoMatch, Some("bootstrap_hint_exceeded"))
			},
			other => (other, reason),
		}
	}

	pub(super) fn classify_preview_only_local_recovery_motion_against(
		&self,
		previous: &RgbaImage,
		frame: &RgbaImage,
	) -> (DownwardRegistration, Option<&'static str>) {
		let config = OverlapSearchConfig::default();
		let preferred_range =
			self.preview_only_local_recovery_motion_range(previous, frame, config);
		let preferred_ranges = preferred_range.into_iter().collect::<Vec<_>>();
		let motion_rows_hint =
			self.last_motion_rows_hint.or(self.normalized_transient_motion_rows_hint());
		let (registration, reason) = self
			.evaluate_reference_downward_registration_with_preferred_ranges(
				previous,
				frame,
				motion_rows_hint,
				&preferred_ranges,
				self.transient_burst_search_enabled,
			);

		match registration {
			DownwardRegistration::Matched(matched)
				if self.bootstrap_motion_exceeds_pending_hint(matched.motion_rows)
					&& !self.bootstrap_hint_exceeded_match_can_commit(matched) =>
			{
				(DownwardRegistration::NoMatch, Some("bootstrap_hint_exceeded"))
			},
			other => (other, reason),
		}
	}

	pub(super) fn effective_motion_rows_hint(&self) -> Option<u32> {
		let transient = self.normalized_transient_motion_rows_hint();

		match (self.last_motion_rows_hint, transient) {
			(Some(last), Some(transient)) if self.transient_burst_search_enabled => {
				Some(last.max(transient))
			},
			(Some(last), Some(_transient)) => Some(last),
			(Some(last), None) => Some(last),
			(None, Some(transient)) => Some(transient),
			(None, None) => None,
		}
	}

	pub(super) fn normalized_transient_motion_rows_hint(&self) -> Option<u32> {
		let transient = self.transient_motion_rows_hint?;

		if self.transient_burst_search_enabled {
			return Some(transient);
		}

		match self.last_motion_rows_hint {
			Some(last) => {
				let cap = last
					.saturating_mul(TRANSIENT_MOTION_HINT_MAX_MULTIPLIER)
					.max(TRANSIENT_MOTION_HINT_MIN_CAP_ROWS)
					.max(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS);

				(transient <= cap).then_some(transient)
			},
			None => Some(transient.min(INITIAL_DOWNWARD_MAX_MOTION_ROWS)),
		}
	}

	pub(super) fn transient_burst_motion_hint_exceeds_local_authority(
		&self,
		local_motion_rows: u32,
	) -> bool {
		if !self.transient_burst_search_enabled {
			return false;
		}

		let Some(transient) = self.transient_motion_rows_hint else {
			return false;
		};
		let capped_local_motion_rows =
			local_motion_rows.min(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS);
		let local_authority_rows = self
			.last_motion_rows_hint
			.unwrap_or(capped_local_motion_rows)
			.max(capped_local_motion_rows);
		let local_authority_cap = local_authority_rows
			.saturating_mul(TRANSIENT_MOTION_HINT_MAX_MULTIPLIER)
			.max(TRANSIENT_MOTION_HINT_MIN_CAP_ROWS);

		transient > local_authority_cap
	}

	pub(super) fn preview_only_local_recovery_motion_range(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		config: OverlapSearchConfig,
	) -> Option<RangeInclusive<u32>> {
		let max_motion_rows = support::max_directional_motion_rows(previous, next, config);

		if max_motion_rows == 0 {
			return None;
		}
		if self.initial_downward_bootstrap_active() && self.last_motion_rows_hint.is_none() {
			if let Some(hint) = self.normalized_transient_motion_rows_hint()
				&& hint <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			{
				let tolerance = (hint / 2)
					.clamp(1, PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS)
					.min(max_motion_rows);
				let min_motion_rows = hint.saturating_sub(tolerance).max(1);
				let max_motion_rows = hint
					.saturating_add(tolerance)
					.min(max_motion_rows)
					.min(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS);

				return Some(min_motion_rows..=max_motion_rows);
			}

			return Some(
				1..=PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS.min(max_motion_rows).max(1),
			);
		}

		if let Some(hint) =
			self.last_motion_rows_hint.or(self.normalized_transient_motion_rows_hint())
		{
			let tolerance = (hint / 2)
				.clamp(1, PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS)
				.min(max_motion_rows);
			let min_motion_rows = if self.seeded_preview_only_local_after_observed_burst_commit
				|| self.preview_only_local_tail_followup_can_include_one_pixel_recovery()
			{
				1
			} else {
				hint.saturating_sub(tolerance).max(1)
			};
			let max_motion_rows = hint
				.saturating_add(tolerance)
				.min(max_motion_rows)
				.min(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS);

			return Some(min_motion_rows..=max_motion_rows);
		}

		Some(1..=PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS.min(max_motion_rows).max(1))
	}

	pub(super) fn preview_only_local_tail_followup_can_include_one_pixel_recovery(&self) -> bool {
		self.transient_burst_search_enabled
			&& self.last_preview_only_downward_local_sample.is_some()
			&& self
				.last_motion_rows_hint
				.is_some_and(|last_hint| last_hint <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS)
			&& self.growth_history.last().is_some_and(|commit| {
				commit.decision_source
					== DownwardViewportCandidateSource::PreviewOnlyLocalSample.decision_source()
					&& commit.growth_rows <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
			})
	}

	pub(super) fn refresh_local_downward_sample(&mut self, frame: &RgbaImage) {
		if !self.initial_downward_bootstrap_active() {
			return;
		}

		self.last_unconfirmed_upward_fingerprint = None;

		let fingerprint = support::scroll_capture_fingerprint(frame);

		self.record_last_sample(frame, fingerprint);
	}

	pub(super) fn refresh_preview_only_downward_local_sample(
		&mut self,
		frame: &RgbaImage,
		provisional_viewport_top_y: Option<i32>,
	) {
		let Some(provisional_viewport_top_y) = provisional_viewport_top_y else {
			self.clear_preview_only_downward_local_sample();

			return;
		};

		if !self.should_refresh_preview_only_downward_local_sample(frame) {
			return;
		}

		self.last_unconfirmed_upward_fingerprint = None;

		self.record_preview_only_downward_local_sample(frame, provisional_viewport_top_y);
	}

	pub(super) fn should_refresh_downward_observed_baseline_after_huge_suppressed_jump(
		&self,
	) -> bool {
		self.pending_suppressed_huge_preview_only_local_followup.is_some()
			|| self.blocked_followup_after_suppressed_huge_preview_local_jump
			|| self.blocked_far_committed_only_recovery_after_corroborated_huge_local_jump
	}

	pub(super) fn should_reset_preview_only_local_baseline_after_huge_far_committed_block(
		&self,
	) -> bool {
		self.blocked_far_committed_only_recovery_after_corroborated_huge_local_jump
	}

	pub(super) fn provisional_viewport_top_y_for_downward_sample_match(
		&self,
		observed_match: DownwardSampleMatch,
	) -> Option<i32> {
		let motion_rows = i32::try_from(observed_match.matched.motion_rows).unwrap_or_default();

		match observed_match.source {
			DownwardSampleMatchSource::ObservedSample => {
				Some(self.observed_viewport_top_y.saturating_add(motion_rows))
			},
			DownwardSampleMatchSource::PreviewOnlyLocalSample => self
				.last_preview_only_downward_local_sample
				.as_ref()
				.map(|sample| sample.viewport_top_y.saturating_add(motion_rows)),
		}
	}

	pub(super) fn preview_only_downward_local_viewport_top_y_for_sample_match(
		&self,
		observed_match: DownwardSampleMatch,
	) -> Option<i32> {
		let provisional_viewport_top_y =
			self.provisional_viewport_top_y_for_downward_sample_match(observed_match)?;
		let candidate = DownwardViewportCandidate {
			source: observed_match.source.into(),
			viewport_top_y: provisional_viewport_top_y,
			motion_rows: observed_match.matched.motion_rows,
			mean_abs_diff_x100: observed_match.matched.mean_abs_diff_x100,
		};

		if self.should_suppress_observed_sample_candidate(candidate)
			|| self.should_suppress_preview_only_local_candidate(candidate)
		{
			return self.stable_preview_only_downward_local_viewport_top_y();
		}

		Some(provisional_viewport_top_y)
	}

	pub(super) fn stable_preview_only_downward_local_viewport_top_y(&self) -> Option<i32> {
		self.last_preview_only_downward_local_sample
			.as_ref()
			.map(|sample| sample.viewport_top_y)
			.or(Some(self.observed_viewport_top_y))
	}

	pub(super) fn should_refresh_preview_only_downward_local_sample(
		&self,
		frame: &RgbaImage,
	) -> bool {
		if self.resume_frontier_top_y.is_some() || self.resume_frontier_requires_reacquire {
			return false;
		}
		if self.last_sample_frame != self.last_downward_observed_frame {
			return false;
		}
		if frame == &self.anchor_frame || frame == &self.last_committed_frame {
			return false;
		}
		if self
			.last_preview_only_downward_local_sample
			.as_ref()
			.is_some_and(|previous| *frame == previous.frame)
		{
			return false;
		}

		!self.growth_history.iter().any(|commit| frame == &commit.frame)
	}

	pub(super) fn initial_downward_bootstrap_active(&self) -> bool {
		self.growth_history.is_empty()
			&& self.current_viewport_top_y == 0
			&& self.resume_frontier_top_y.is_none()
			&& !self.resume_frontier_requires_reacquire
	}

	pub(super) fn bootstrap_motion_cap_from_pending_hint(&self) -> Option<u32> {
		if !self.initial_downward_bootstrap_active() || self.last_motion_rows_hint.is_some() {
			return None;
		}

		self.normalized_transient_motion_rows_hint().map(|hint| {
			let tolerance = (hint / 2).clamp(1, PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS);

			hint.saturating_add(tolerance)
		})
	}

	pub(super) fn bootstrap_motion_exceeds_pending_hint(&self, motion_rows: u32) -> bool {
		self.bootstrap_motion_cap_from_pending_hint().is_some_and(|cap| motion_rows > cap)
	}

	pub(super) fn bootstrap_hint_exceeded_match_can_commit(&self, matched: DirectionMatch) -> bool {
		self.initial_downward_bootstrap_active()
			&& self.transient_burst_search_enabled
			&& self.last_motion_rows_hint.is_none()
			&& matched.mean_abs_diff_x100 <= DIRECTION_WARNING_MARGIN_X100.saturating_mul(4)
	}

	pub(super) fn bootstrap_initial_growth_cap_rows(&self) -> Option<u32> {
		if !self.initial_downward_bootstrap_active() || self.last_motion_rows_hint.is_some() {
			return None;
		}
		if self.transient_burst_search_enabled {
			return None;
		}

		self.bootstrap_motion_cap_from_pending_hint()
			.map(|cap| cap.min(BOOTSTRAP_HINTED_INITIAL_GROWTH_MAX_ROWS))
	}
}
