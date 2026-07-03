use color_eyre::eyre::Result;

use crate::scroll_capture::{
	CommittedDownwardViewportCandidateMode, DIRECTION_WARNING_MARGIN_X100,
	DOWNWARD_KEYFRAME_MIN_OVERLAP_DIVISOR, DOWNWARD_KEYFRAME_SEARCH_LIMIT,
	DOWNWARD_KEYFRAME_SEARCH_MAX_TOLERANCE_ROWS, DOWNWARD_KEYFRAME_SEARCH_MOTION_TOLERANCE_ROWS,
	DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS, DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS, DirectionMatch,
	DirectionMatchEval, DownwardRegistration, DownwardSampleMatch, DownwardSampleMatchSource,
	DownwardViewportCandidate, DownwardViewportCandidateSource, DownwardViewportResolution,
	FALLBACK_DOWNWARD_GROWTH_MAX_ROWS, FALLBACK_DOWNWARD_GROWTH_MIN_ROWS, GrowthCommit,
	INITIAL_DOWNWARD_MAX_MOTION_ROWS, LOCAL_DOWNWARD_SEARCH_MAX_TOLERANCE_ROWS,
	LOCAL_DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS, MotionObservation, OverlapSearchConfig,
	OverlapSearchRange, PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS,
	PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS, PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS,
	RangeInclusive, RgbaImage, ScrollDirection, ScrollObserveOutcome, ScrollSession,
	TRANSIENT_BURST_UNDERCONSUMED_HINT_MIN_ROWS, UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS,
	eyre,
};
use crate::scroll_capture::{downward_candidates, support};

impl ScrollSession {
	pub(super) fn evaluate_reference_overlap_direction(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		direction: ScrollDirection,
		motion_rows_hint: Option<u32>,
	) -> Option<DirectionMatch> {
		let config = OverlapSearchConfig::default();
		let preferred_range =
			self.preferred_motion_range_from_hint(previous, next, motion_rows_hint, config)?;

		support::evaluate_overlap_direction(previous, next, direction, preferred_range, config)
	}

	pub(super) fn evaluate_reference_downward_registration(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		allow_full_range_fallback: bool,
	) -> DownwardRegistration {
		let config = OverlapSearchConfig::default();
		let preferred_range = self.preferred_downward_motion_range_from_hint(
			previous,
			next,
			motion_rows_hint,
			config,
		);

		self.evaluate_reference_downward_registration_with_preferred_range(
			previous,
			next,
			motion_rows_hint,
			preferred_range,
			allow_full_range_fallback,
		)
	}

	pub(super) fn evaluate_reference_downward_registration_with_preferred_ranges(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		preferred_ranges: &[RangeInclusive<u32>],
		allow_full_range_fallback: bool,
	) -> (DownwardRegistration, Option<&'static str>) {
		let config = OverlapSearchConfig::default();
		let max_overlap = previous.height().min(next.height());
		let max_motion_rows = support::max_directional_motion_rows(previous, next, config);
		let mut candidates = support::collect_overlap_direction_matches_in_ranges(
			previous,
			next,
			ScrollDirection::Down,
			preferred_ranges,
			config,
		);
		let mut no_match_reason = if candidates.is_empty() { Some("no_candidates") } else { None };

		if candidates.is_empty()
			&& allow_full_range_fallback
			&& (motion_rows_hint.is_none() || self.transient_burst_search_enabled)
		{
			candidates = support::collect_overlap_direction_matches(
				previous,
				next,
				ScrollDirection::Down,
				1..=max_motion_rows,
				config,
			);
			no_match_reason = if candidates.is_empty() { Some("no_candidates") } else { None };
		}

		candidates.retain(|matched| {
			support::downward_registration_has_meaningful_overlap(*matched, max_overlap, config)
		});

		if candidates.is_empty() {
			no_match_reason.get_or_insert("insufficient_overlap");
		}

		let classification = support::classify_downward_registration_candidates(&candidates);
		let upward_veto = self.evaluate_reference_overlap_direction(
			previous,
			next,
			ScrollDirection::Up,
			motion_rows_hint,
		);

		match (classification, upward_veto) {
			(DownwardRegistration::Matched(down), Some(up))
				if down.motion_rows <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
					&& down.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
						>= up.mean_abs_diff_x100 =>
			{
				(DownwardRegistration::NoMatch, Some("direction_ambiguous"))
			},
			(DownwardRegistration::NoMatch, _) => (DownwardRegistration::NoMatch, no_match_reason),
			(other, _) => (other, None),
		}
	}

	pub(super) fn evaluate_reference_downward_registration_with_preferred_range(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		preferred_range: Option<RangeInclusive<u32>>,
		allow_full_range_fallback: bool,
	) -> DownwardRegistration {
		let config = OverlapSearchConfig::default();
		let max_overlap = previous.height().min(next.height());
		let max_motion_rows = support::max_directional_motion_rows(previous, next, config);
		let mut candidates = preferred_range.as_ref().map_or_else(Vec::new, |range| {
			support::collect_overlap_direction_matches(
				previous,
				next,
				ScrollDirection::Down,
				range.clone(),
				config,
			)
		});
		let mut no_match_reason = if candidates.is_empty() { Some("no_candidates") } else { None };

		if candidates.is_empty()
			&& allow_full_range_fallback
			&& (motion_rows_hint.is_none() || self.transient_burst_search_enabled)
		{
			candidates = support::collect_overlap_direction_matches(
				previous,
				next,
				ScrollDirection::Down,
				1..=max_motion_rows,
				config,
			);
			no_match_reason = if candidates.is_empty() { Some("no_candidates") } else { None };
		}

		candidates.retain(|matched| {
			support::downward_registration_has_meaningful_overlap(*matched, max_overlap, config)
		});

		if candidates.is_empty() {
			no_match_reason.get_or_insert("insufficient_overlap");
		}

		let classification = support::classify_downward_registration_candidates(&candidates);
		let upward_veto = self.evaluate_reference_overlap_direction(
			previous,
			next,
			ScrollDirection::Up,
			motion_rows_hint,
		);

		match (classification, upward_veto) {
			(DownwardRegistration::Matched(down), Some(up))
				if down.motion_rows <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
					&& down.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
						>= up.mean_abs_diff_x100 =>
			{
				DownwardRegistration::NoMatch
			},
			(DownwardRegistration::NoMatch, _) => {
				let _ = no_match_reason;

				DownwardRegistration::NoMatch
			},
			(other, _) => other,
		}
	}

	pub(super) fn sequential_downward_motion_ranges(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		config: OverlapSearchConfig,
	) -> Vec<RangeInclusive<u32>> {
		let local_motion_rows_hint = self.last_motion_rows_hint;
		let mut ranges = Vec::new();

		if let Some(local_range) = self.preferred_local_downward_motion_range_from_hint(
			previous,
			next,
			local_motion_rows_hint,
			config,
		) {
			ranges.push(local_range);
		}

		if self.initial_downward_bootstrap_active() && self.last_motion_rows_hint.is_none() {
			return ranges;
		}

		if let Some(transient_range) = self.transient_downward_motion_range(previous, next, config)
			&& !ranges.contains(&transient_range)
		{
			ranges.push(transient_range);
		}

		ranges
	}

	pub(super) fn clear_last_downward_sample_registration(&mut self) {
		self.last_downward_sample_registration_result = None;
		self.last_downward_sample_registration_source = None;
		self.last_downward_sample_registration_motion_rows = None;
		self.last_downward_sample_registration_provisional_viewport_top_y = None;
		self.last_observed_sample_registration_result = None;
		self.last_observed_sample_registration_reason = None;
		self.last_observed_sample_registration_motion_rows = None;
		self.last_observed_sample_registration_mean_abs_diff_x100 = None;
		self.last_preview_only_local_registration_result = None;
		self.last_preview_only_local_registration_reason = None;
		self.last_preview_only_local_registration_motion_rows = None;
		self.last_preview_only_local_registration_mean_abs_diff_x100 = None;
		self.last_downward_viewport_candidate_count = None;
		self.last_downward_viewport_candidates_before_prune = None;
		self.last_downward_viewport_candidates_after_prune = None;
		self.blocked_underconsumed_observed_recovery_in_burst = false;
		self.blocked_lagging_exactly_corroborated_preview_local_tail_in_burst = false;
		self.blocked_followup_after_suppressed_huge_preview_local_jump = false;
		self.blocked_followup_after_extreme_preview_local_tail = false;
		self.blocked_far_committed_only_recovery_after_corroborated_huge_local_jump = false;
	}

	pub(super) fn record_last_downward_sample_registration(
		&mut self,
		result: &'static str,
		source: Option<DownwardSampleMatchSource>,
		motion_rows: Option<u32>,
	) {
		self.last_downward_sample_registration_result = Some(result);
		self.last_downward_sample_registration_source =
			source.map(DownwardSampleMatchSource::label);
		self.last_downward_sample_registration_motion_rows = motion_rows;
	}

	pub(super) fn record_last_sample_eval_context(&mut self) {
		self.last_sample_eval_last_motion_rows_hint = self.last_motion_rows_hint;
		self.last_sample_eval_transient_motion_rows_hint = self.transient_motion_rows_hint;
		self.last_sample_eval_effective_motion_rows_hint = self.effective_motion_rows_hint();
		self.last_sample_eval_transient_burst_search_enabled = self.transient_burst_search_enabled;
	}

	pub(super) fn transient_downward_motion_range(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		config: OverlapSearchConfig,
	) -> Option<RangeInclusive<u32>> {
		let transient_motion_rows_hint = self.normalized_transient_motion_rows_hint()?;
		let max_motion_rows = support::max_directional_motion_rows(previous, next, config);

		if transient_motion_rows_hint == 0 || transient_motion_rows_hint > max_motion_rows {
			return None;
		}

		let tolerance = (transient_motion_rows_hint / 2)
			.clamp(
				LOCAL_DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS,
				LOCAL_DOWNWARD_SEARCH_MAX_TOLERANCE_ROWS,
			)
			.min(max_motion_rows);
		let min_motion_rows = transient_motion_rows_hint.saturating_sub(tolerance).max(1);
		let max_motion_rows =
			transient_motion_rows_hint.saturating_add(tolerance).min(max_motion_rows);

		Some(min_motion_rows..=max_motion_rows)
	}

	pub(super) fn preferred_local_downward_motion_range_from_hint(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		config: OverlapSearchConfig,
	) -> Option<RangeInclusive<u32>> {
		let max_motion_rows = support::max_directional_motion_rows(previous, next, config);

		if let Some(last_growth_rows) = motion_rows_hint {
			let tolerance = (last_growth_rows / 2)
				.clamp(
					LOCAL_DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS,
					LOCAL_DOWNWARD_SEARCH_MAX_TOLERANCE_ROWS,
				)
				.min(max_motion_rows);
			let upper_tolerance =
				if last_growth_rows >= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS {
					last_growth_rows.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS)
				} else {
					tolerance
				}
				.min(max_motion_rows);
			let min_motion_rows = last_growth_rows.saturating_sub(tolerance).max(1);
			let max_motion_rows =
				last_growth_rows.saturating_add(upper_tolerance).min(max_motion_rows);

			return Some(min_motion_rows..=max_motion_rows);
		}

		Some(1..=INITIAL_DOWNWARD_MAX_MOTION_ROWS.min(max_motion_rows).max(1))
	}

	pub(super) fn diagnose_reference_overlap_direction(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		direction: ScrollDirection,
		motion_rows_hint: Option<u32>,
	) -> DirectionMatchEval {
		let config = OverlapSearchConfig::default();
		let preferred_range = self
			.preferred_motion_range_from_hint(previous, next, motion_rows_hint, config)
			.map(OverlapSearchRange::from);

		self.diagnose_reference_overlap_direction_with_preferred_range(
			previous,
			next,
			direction,
			preferred_range,
			false,
		)
	}

	pub(super) fn diagnose_reference_overlap_direction_with_preferred_range(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		direction: ScrollDirection,
		preferred_range: Option<OverlapSearchRange>,
		allow_downward_full_range_fallback: bool,
	) -> DirectionMatchEval {
		let config = OverlapSearchConfig::default();
		let max_motion_rows = support::max_directional_motion_rows(previous, next, config);
		let preferred_only_match = preferred_range.and_then(|range| {
			support::evaluate_overlap_direction(previous, next, direction, range.as_range(), config)
		});
		let mut final_match = preferred_only_match;
		let mut used_full_range_fallback = false;

		if final_match.is_none() && allow_downward_full_range_fallback {
			final_match = support::evaluate_overlap_direction(
				previous,
				next,
				direction,
				1..=max_motion_rows,
				config,
			);
			used_full_range_fallback = final_match.is_some();
		}

		DirectionMatchEval {
			preferred_range,
			max_motion_rows,
			preferred_only_match,
			final_match,
			used_full_range_fallback,
		}
	}

	pub(super) fn evaluate_reference_overlap_direction_preferred_only(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		direction: ScrollDirection,
		motion_rows_hint: Option<u32>,
	) -> Option<DirectionMatch> {
		let config = OverlapSearchConfig::default();
		let preferred_range =
			self.preferred_motion_range_from_hint(previous, next, motion_rows_hint, config)?;

		support::evaluate_overlap_direction(previous, next, direction, preferred_range, config)
	}

	pub(super) fn preferred_motion_range_from_hint(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		config: OverlapSearchConfig,
	) -> Option<RangeInclusive<u32>> {
		let max_motion_rows = support::max_directional_motion_rows(previous, next, config);

		if let Some(last_growth_rows) = motion_rows_hint {
			let tolerance = DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS.min(max_motion_rows);
			let min_motion_rows = last_growth_rows.saturating_sub(tolerance).max(1);
			let max_motion_rows = last_growth_rows.saturating_add(tolerance).min(max_motion_rows);

			return Some(min_motion_rows..=max_motion_rows);
		}

		Some(1..=INITIAL_DOWNWARD_MAX_MOTION_ROWS.min(max_motion_rows).max(1))
	}

	pub(super) fn preferred_downward_motion_range_from_hint(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		config: OverlapSearchConfig,
	) -> Option<RangeInclusive<u32>> {
		let max_motion_rows = support::max_directional_motion_rows(previous, next, config);

		if let Some(last_growth_rows) = motion_rows_hint {
			let tolerance = (last_growth_rows / 2)
				.clamp(
					DOWNWARD_KEYFRAME_SEARCH_MOTION_TOLERANCE_ROWS,
					DOWNWARD_KEYFRAME_SEARCH_MAX_TOLERANCE_ROWS,
				)
				.min(max_motion_rows);
			let upper_tolerance =
				if last_growth_rows >= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS {
					last_growth_rows.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS)
				} else {
					tolerance
				}
				.min(max_motion_rows);
			let min_motion_rows = last_growth_rows.saturating_sub(tolerance).max(1);
			let max_motion_rows =
				last_growth_rows.saturating_add(upper_tolerance).min(max_motion_rows);

			return Some(min_motion_rows..=max_motion_rows);
		}

		Some(1..=INITIAL_DOWNWARD_MAX_MOTION_ROWS.min(max_motion_rows).max(1))
	}

	pub(super) fn resolve_downward_viewport_candidate(
		&mut self,
		frame: &RgbaImage,
		observed_match: DownwardSampleMatch,
	) -> DownwardViewportResolution {
		let pending_suppressed_huge_preview_only_local_followup =
			self.pending_suppressed_huge_preview_only_local_followup.take();
		let pending_suppressed_huge_preview_only_local_followup_remaining_blocks =
			self.pending_suppressed_huge_preview_only_local_followup_remaining_blocks;

		self.pending_suppressed_huge_preview_only_local_followup_remaining_blocks = 0;

		let pending_extreme_preview_only_local_tail_followup =
			self.pending_extreme_preview_only_local_tail_followup.take();
		let pending_extreme_preview_only_local_tail_followup_remaining_blocks =
			self.pending_extreme_preview_only_local_tail_followup_remaining_blocks;

		self.pending_extreme_preview_only_local_tail_followup_remaining_blocks = 0;

		let provisional_viewport_top_y =
			self.provisional_viewport_top_y_for_downward_sample_match(observed_match);
		let mut candidates = Vec::with_capacity(DOWNWARD_KEYFRAME_SEARCH_LIMIT.saturating_add(1));
		let mut suppressed_observed_candidate = None;
		let mut suppressed_preview_only_local_candidate = None;

		self.last_downward_sample_registration_provisional_viewport_top_y =
			provisional_viewport_top_y;

		if let Some(viewport_top_y) = provisional_viewport_top_y {
			let candidate = DownwardViewportCandidate {
				source: observed_match.source.into(),
				viewport_top_y,
				motion_rows: observed_match.matched.motion_rows,
				mean_abs_diff_x100: observed_match.matched.mean_abs_diff_x100,
			};
			let suppress_observed = self.should_suppress_observed_sample_candidate(candidate);
			let suppress_preview_local =
				self.should_suppress_preview_only_local_candidate(candidate);

			if !suppress_observed && !suppress_preview_local {
				candidates.push(candidate);
			} else if suppress_observed
				&& candidate.source == DownwardViewportCandidateSource::ObservedSample
			{
				suppressed_observed_candidate = Some(candidate);
			} else if suppress_preview_local
				&& candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			{
				suppressed_preview_only_local_candidate = Some(candidate);
			}
		}

		self.collect_committed_downward_viewport_candidates(frame, &mut candidates);
		self.apply_pending_preview_local_followup_blocks(
			suppressed_preview_only_local_candidate,
			pending_suppressed_huge_preview_only_local_followup,
			pending_suppressed_huge_preview_only_local_followup_remaining_blocks,
			pending_extreme_preview_only_local_tail_followup,
			pending_extreme_preview_only_local_tail_followup_remaining_blocks,
			&mut candidates,
		);
		self.restore_corroborated_observed_candidate(
			suppressed_observed_candidate,
			&mut candidates,
		);

		let preview_only_local_candidate_before_prune =
			candidates.iter().copied().find(|candidate| {
				candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			});
		let candidates_before_prune = candidates.clone();

		self.last_downward_viewport_candidates_before_prune =
			Some(downward_candidates::format_downward_viewport_candidates(&candidates));

		self.prune_committed_keyframe_candidates_outside_local_continuity(&mut candidates);
		self.restore_repeated_small_preview_only_local_candidate_after_empty_prune(
			preview_only_local_candidate_before_prune,
			&mut candidates,
		);

		if self.should_fail_closed_lagging_exactly_corroborated_preview_local_tail_in_burst(
			&candidates,
		) {
			self.blocked_lagging_exactly_corroborated_preview_local_tail_in_burst = true;

			candidates.clear();
		}
		if self.should_fail_closed_underconsumed_observed_recovery_in_burst(
			&candidates_before_prune,
			&candidates,
		) {
			self.blocked_underconsumed_observed_recovery_in_burst = true;

			candidates.clear();
		}

		self.last_downward_viewport_candidate_count = Some(candidates.len());
		self.last_downward_viewport_candidates_after_prune =
			Some(downward_candidates::format_downward_viewport_candidates(&candidates));

		downward_candidates::select_downward_viewport_candidate(&mut candidates)
	}

	pub(super) fn collect_committed_downward_viewport_candidates(
		&self,
		frame: &RgbaImage,
		candidates: &mut Vec<DownwardViewportCandidate>,
	) {
		self.collect_committed_downward_viewport_candidates_with_mode(
			frame,
			candidates,
			CommittedDownwardViewportCandidateMode::IncludeRecentHistory,
		);
	}

	pub(super) fn collect_fallback_downward_viewport_candidates(
		&self,
		frame: &RgbaImage,
		candidates: &mut Vec<DownwardViewportCandidate>,
	) {
		self.collect_committed_downward_viewport_candidates_with_mode(
			frame,
			candidates,
			CommittedDownwardViewportCandidateMode::LastCommittedOnly,
		);
	}

	pub(super) fn collect_committed_downward_viewport_candidates_with_mode(
		&self,
		frame: &RgbaImage,
		candidates: &mut Vec<DownwardViewportCandidate>,
		mode: CommittedDownwardViewportCandidateMode,
	) {
		self.push_downward_viewport_candidate(
			&self.last_committed_frame,
			self.current_viewport_top_y,
			frame,
			DownwardViewportCandidateSource::CommittedKeyframe,
			candidates,
		);

		if mode == CommittedDownwardViewportCandidateMode::LastCommittedOnly
			|| DOWNWARD_KEYFRAME_SEARCH_LIMIT <= 1
		{
			return;
		}

		for commit in self
			.growth_history
			.iter()
			.rev()
			.skip(1)
			.take(DOWNWARD_KEYFRAME_SEARCH_LIMIT.saturating_sub(1))
		{
			self.push_downward_viewport_candidate(
				&commit.frame,
				commit.viewport_top_y,
				frame,
				DownwardViewportCandidateSource::CommittedKeyframe,
				candidates,
			);
		}
	}

	pub(super) fn push_downward_viewport_candidate(
		&self,
		reference: &RgbaImage,
		reference_viewport_top_y: i32,
		frame: &RgbaImage,
		source: DownwardViewportCandidateSource,
		candidates: &mut Vec<DownwardViewportCandidate>,
	) {
		let predicted_motion_rows = self.downward_keyframe_motion_hint(reference_viewport_top_y);
		let allow_full_range_fallback =
			!(self.initial_downward_bootstrap_active() && predicted_motion_rows.is_none());
		let mut registration = self.evaluate_reference_downward_registration(
			reference,
			frame,
			predicted_motion_rows,
			allow_full_range_fallback,
		);

		if source == DownwardViewportCandidateSource::CommittedKeyframe
			&& self.should_retry_committed_keyframe_registration_across_full_range(registration)
		{
			let full_range_registration = self
				.evaluate_reference_downward_registration_with_preferred_range(
					reference,
					frame,
					predicted_motion_rows,
					None,
					true,
				);

			registration = self.prefer_full_range_committed_keyframe_registration(
				registration,
				full_range_registration,
			);
		}

		if let DownwardRegistration::Matched(matched) = registration {
			if self.bootstrap_motion_exceeds_pending_hint(matched.motion_rows) {
				return;
			}

			let max_overlap = reference.height().min(frame.height());
			let min_keyframe_overlap_rows = OverlapSearchConfig::default()
				.min_overlap_rows
				.max(max_overlap / DOWNWARD_KEYFRAME_MIN_OVERLAP_DIVISOR)
				.max(1);
			let overlap_rows = max_overlap.saturating_sub(matched.motion_rows);

			if overlap_rows < min_keyframe_overlap_rows {
				return;
			}

			let viewport_top_y = reference_viewport_top_y
				.saturating_add(i32::try_from(matched.motion_rows).unwrap_or_default());

			if viewport_top_y <= self.current_viewport_top_y {
				return;
			}

			candidates.push(DownwardViewportCandidate {
				source,
				viewport_top_y,
				motion_rows: matched.motion_rows,
				mean_abs_diff_x100: matched.mean_abs_diff_x100,
			});
		}
	}

	pub(super) fn should_retry_committed_keyframe_registration_across_full_range(
		&self,
		registration: DownwardRegistration,
	) -> bool {
		let DownwardRegistration::Matched(matched) = registration else {
			return false;
		};
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		let low_confidence_match =
			matched.mean_abs_diff_x100 > DIRECTION_WARNING_MARGIN_X100.saturating_mul(4);
		let tiny_underconsumed_match = self
			.transient_burst_motion_hint_exceeds_local_authority(matched.motion_rows)
			&& matched.mean_abs_diff_x100 > DIRECTION_WARNING_MARGIN_X100.saturating_mul(4)
			&& matched.motion_rows
				<= last_motion_rows_hint.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS);
		let large_overshot_match = matched.motion_rows > last_motion_rows_hint.saturating_mul(2);

		low_confidence_match && (tiny_underconsumed_match || large_overshot_match)
	}

	pub(super) fn prefer_full_range_committed_keyframe_registration(
		&self,
		preferred_range_registration: DownwardRegistration,
		full_range_registration: DownwardRegistration,
	) -> DownwardRegistration {
		match (preferred_range_registration, full_range_registration) {
			(DownwardRegistration::Matched(preferred), DownwardRegistration::Matched(full))
				if full.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					< preferred.mean_abs_diff_x100
					&& preferred.motion_rows.abs_diff(full.motion_rows)
						> UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS =>
			{
				DownwardRegistration::Matched(full)
			},
			(preferred, _) => preferred,
		}
	}

	pub(super) fn downward_keyframe_motion_hint(
		&self,
		reference_viewport_top_y: i32,
	) -> Option<u32> {
		let last_motion_rows = self.last_motion_rows_hint?;
		let already_traversed_rows = u32::try_from(
			self.current_viewport_top_y.saturating_sub(reference_viewport_top_y).max(0),
		)
		.unwrap_or_default();

		Some(already_traversed_rows.saturating_add(last_motion_rows))
	}

	pub(super) fn fallback_downward_growth_blocked_while_resume_frontier_active(
		&mut self,
		candidate_viewport_top_y: i32,
		motion_rows: u32,
		preview_changed: bool,
		decision_source: &'static str,
	) -> Option<ScrollObserveOutcome> {
		let resume_frontier_top_y = self.resume_frontier_top_y?;
		let growth_rows = if candidate_viewport_top_y <= resume_frontier_top_y {
			0
		} else {
			u32::try_from(candidate_viewport_top_y - resume_frontier_top_y).unwrap_or_default()
		};

		self.log_decision(
			"scroll_capture.fallback_downward_blocked_while_resume_frontier_active",
			ScrollDirection::Down,
			Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
			Some(candidate_viewport_top_y),
			Some(growth_rows),
			Some(decision_source),
		);

		Some(support::preview_update_outcome(preview_changed))
	}

	pub(super) fn fallback_downward_growth_exceeds_continuity_budget(
		&self,
		candidate_viewport_top_y: i32,
	) -> bool {
		let growth_rows = self.growth_rows_for_candidate_viewport_top_y(candidate_viewport_top_y);
		let Some(base_continuity_rows) = self.last_motion_rows_hint else {
			return false;
		};
		let local_overrun_rows = base_continuity_rows
			.saturating_mul(2)
			.clamp(FALLBACK_DOWNWARD_GROWTH_MIN_ROWS, FALLBACK_DOWNWARD_GROWTH_MAX_ROWS);
		let preview_local_rows = self
			.last_preview_only_downward_local_sample
			.as_ref()
			.map(|sample| {
				u32::try_from(
					sample.viewport_top_y.saturating_sub(self.current_viewport_top_y).max(0),
				)
				.unwrap_or_default()
			})
			.unwrap_or_default();
		let max_growth_rows = preview_local_rows.saturating_add(local_overrun_rows);

		growth_rows > max_growth_rows
	}

	pub(super) fn record_transient_burst_missing_downward_candidate_frame(
		&mut self,
		preview_changed: bool,
	) {
		if self.transient_burst_search_enabled && preview_changed {
			self.consecutive_transient_burst_missing_downward_candidate_frames = self
				.consecutive_transient_burst_missing_downward_candidate_frames
				.saturating_add(1);
		} else {
			self.consecutive_transient_burst_missing_downward_candidate_frames = 0;
		}
	}

	pub(super) fn transient_burst_candidate_underconsumes_input_hint(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		if !self.transient_burst_search_enabled || self.resume_frontier_top_y.is_some() {
			return false;
		}

		let Some(hint) = self.normalized_transient_motion_rows_hint() else {
			return false;
		};

		if hint < TRANSIENT_BURST_UNDERCONSUMED_HINT_MIN_ROWS {
			return false;
		}

		let growth_rows = self.growth_rows_for_candidate_viewport_top_y(candidate.viewport_top_y);
		let visibly_underconsumed =
			growth_rows.saturating_add(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS) < hint;
		let severely_underconsumed = growth_rows <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
			|| growth_rows.saturating_mul(3) < hint;

		visibly_underconsumed
			&& (severely_underconsumed
				|| self.transient_burst_motion_hint_exceeds_local_authority(
					growth_rows.max(candidate.motion_rows),
				))
	}

	pub(super) fn observe_fallback_downward_growth(
		&mut self,
		frame: RgbaImage,
		preview_changed: bool,
	) -> Result<ScrollObserveOutcome> {
		let mut candidates = Vec::with_capacity(DOWNWARD_KEYFRAME_SEARCH_LIMIT);

		self.collect_fallback_downward_viewport_candidates(&frame, &mut candidates);

		match downward_candidates::select_downward_viewport_candidate(&mut candidates) {
			DownwardViewportResolution::NoMatch => {
				self.record_transient_burst_missing_downward_candidate_frame(preview_changed);
				self.refresh_preview_only_downward_local_sample(
					&frame,
					self.stable_preview_only_downward_local_viewport_top_y(),
				);
				self.log_decision(
					"scroll_capture.fallback_downward_no_match",
					ScrollDirection::Down,
					None,
					None,
					Some(0),
					Some("no_committed_keyframe_match"),
				);

				Ok(support::preview_update_outcome(preview_changed))
			},
			DownwardViewportResolution::Selected(candidate) => {
				if self.transient_burst_candidate_underconsumes_input_hint(candidate) {
					return self.block_downward_growth_candidate(
						&frame,
						candidate.motion_rows,
						candidate,
						preview_changed,
						"visual_motion_underconsumed_input_hint",
					);
				}
				if self.fallback_downward_growth_exceeds_continuity_budget(candidate.viewport_top_y)
				{
					self.refresh_preview_only_downward_local_sample(
						&frame,
						self.stable_preview_only_downward_local_viewport_top_y(),
					);
					self.log_decision(
						"scroll_capture.fallback_downward_growth_blocked",
						ScrollDirection::Down,
						Some(MotionObservation {
							direction: ScrollDirection::Down,
							motion_rows: candidate.motion_rows,
						}),
						Some(candidate.viewport_top_y),
						Some(
							self.growth_rows_for_candidate_viewport_top_y(candidate.viewport_top_y),
						),
						Some("fallback_committed_candidate_exceeded_local_continuity_budget"),
					);

					return Ok(support::preview_update_outcome(preview_changed));
				}

				if let Some(outcome) = self
					.fallback_downward_growth_blocked_while_resume_frontier_active(
						candidate.viewport_top_y,
						candidate.motion_rows,
						preview_changed,
						"resume_frontier_active_blocks_keyframe_fallback_downward_match",
					) {
					return Ok(outcome);
				}

				self.observe_downward_growth_to_viewport(
					frame,
					candidate.viewport_top_y,
					preview_changed,
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: candidate.motion_rows,
					}),
					candidate.source.fallback_decision_source(),
				)
			},
			DownwardViewportResolution::Ambiguous { preferred, competing } => {
				self.refresh_preview_only_downward_local_sample(
					&frame,
					self.stable_preview_only_downward_local_viewport_top_y(),
				);
				self.log_decision(
					"scroll_capture.fallback_ambiguous_downward_registration",
					ScrollDirection::Down,
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: preferred.motion_rows,
					}),
					Some(preferred.viewport_top_y),
					Some(0),
					Some(preferred.competing_block_reason(competing)),
				);

				Ok(support::preview_update_outcome(preview_changed))
			},
		}
	}

	#[allow(clippy::too_many_arguments)]
	pub(super) fn apply_growth(
		&mut self,
		frame: RgbaImage,
		growth_rows: u32,
		viewport_top_y: i32,
		decision_source: &'static str,
		detected_motion_rows: Option<u32>,
		effective_motion_rows_hint: Option<u32>,
		previous_motion_rows_hint: Option<u32>,
	) -> Result<ScrollObserveOutcome> {
		let fingerprint = support::scroll_capture_fingerprint(&frame);
		let strip = support::crop_bottom_rows(&frame, growth_rows)
			.ok_or_else(|| eyre::eyre!("failed to extract growth strip"))?;
		let preview_strip = support::resize_strip_to_preview_width(&strip, self.preview_width_px);

		self.export_image = support::append_vertical_image(&self.export_image, &strip)?;
		self.preview_image = support::append_vertical_image(&self.preview_image, &preview_strip)?;

		self.bottom_segments.push(strip);
		self.bottom_preview_segments.push(preview_strip);

		self.current_viewport_top_y = viewport_top_y;
		self.observed_viewport_top_y = viewport_top_y;

		self.record_last_sample(&frame, fingerprint);
		self.record_last_downward_observed_sample(
			&frame,
			support::scroll_capture_fingerprint(&frame),
		);

		if self.should_seed_preview_only_local_after_observed_burst_commit(
			decision_source,
			growth_rows,
			previous_motion_rows_hint,
		) {
			self.record_preview_only_downward_local_sample(&frame, viewport_top_y);

			self.seeded_preview_only_local_after_observed_burst_commit = true;
		} else if self.should_preserve_preview_only_local_after_preview_only_burst_commit(
			decision_source,
			growth_rows,
			previous_motion_rows_hint,
		) {
			self.record_preview_only_downward_local_sample(&frame, viewport_top_y);

			self.seeded_preview_only_local_after_observed_burst_commit = false;
			self.last_blocked_preview_only_local_candidate = None;
		} else {
			self.clear_preview_only_downward_local_sample();
		}

		self.last_unconfirmed_upward_fingerprint = None;
		self.last_committed_frame = frame.clone();
		self.resume_frontier_top_y = None;
		self.resume_frontier_requires_reacquire = false;

		self.growth_history.push(GrowthCommit {
			frame,
			growth_rows,
			viewport_top_y,
			decision_source,
			detected_motion_rows,
			effective_motion_rows_hint,
		});

		Ok(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows })
	}

	pub(super) fn should_seed_preview_only_local_after_observed_burst_commit(
		&self,
		decision_source: &'static str,
		growth_rows: u32,
		previous_motion_rows_hint: Option<u32>,
	) -> bool {
		decision_source == DownwardViewportCandidateSource::ObservedSample.decision_source()
			&& self.transient_burst_search_enabled
			&& previous_motion_rows_hint.is_some_and(|previous| {
				previous >= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS && growth_rows < previous
			})
	}

	pub(super) fn should_preserve_preview_only_local_after_preview_only_burst_commit(
		&self,
		decision_source: &'static str,
		growth_rows: u32,
		previous_motion_rows_hint: Option<u32>,
	) -> bool {
		decision_source == DownwardViewportCandidateSource::PreviewOnlyLocalSample.decision_source()
			&& previous_motion_rows_hint.is_some_and(|previous| {
				if self.transient_burst_search_enabled {
					growth_rows >= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
						&& growth_rows
							>= previous.saturating_sub(PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS)
						&& growth_rows
							<= previous
								.saturating_add(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS)
				} else {
					previous <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
						&& growth_rows > 1 && growth_rows <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
						&& growth_rows <= previous
				}
			})
	}

	pub(super) fn rebuild_export_image(&self) -> Result<RgbaImage> {
		let mut ordered = Vec::with_capacity(self.bottom_segments.len().saturating_add(1));

		ordered.push(&self.anchor_frame);

		for strip in &self.bottom_segments {
			ordered.push(strip);
		}

		support::stack_vertical_images(&ordered)
	}

	pub(super) fn rebuild_preview_image(&self) -> Result<RgbaImage> {
		let mut ordered = Vec::with_capacity(self.bottom_preview_segments.len().saturating_add(1));

		ordered.push(&self.anchor_preview);

		for strip in &self.bottom_preview_segments {
			ordered.push(strip);
		}

		support::stack_vertical_images(&ordered)
	}
}
