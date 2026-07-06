use std::ops::RangeInclusive;

use crate::scroll_capture::support;
use crate::scroll_capture::{
	DIRECTION_WARNING_MARGIN_X100, DOWNWARD_KEYFRAME_SEARCH_MAX_TOLERANCE_ROWS,
	DOWNWARD_KEYFRAME_SEARCH_MOTION_TOLERANCE_ROWS, DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS,
	DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS, DirectionMatch, DirectionMatchEval, DownwardRegistration,
	INITIAL_DOWNWARD_MAX_MOTION_ROWS, LOCAL_DOWNWARD_SEARCH_MAX_TOLERANCE_ROWS,
	LOCAL_DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS, OverlapSearchConfig, OverlapSearchRange,
	PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS, RgbaImage, ScrollDirection, ScrollSession,
};

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
}
