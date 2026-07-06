use image::RgbaImage;

use crate::scroll_capture::{
	DIRECTION_WARNING_MARGIN_X100, DirectionMatch, DownwardRegistration, OverlapSearchConfig,
	ScrollDirection, support,
};

pub(super) fn classify_pairwise_downward_sample_motion_against(
	previous: &RgbaImage,
	next: &RgbaImage,
) -> Option<DirectionMatch> {
	let motion_rows = estimate_pairwise_downward_shift_rows(previous, next)?;
	let config = worker_pairwise_overlap_search_config();
	let matched = support::evaluate_overlap_direction(
		previous,
		next,
		ScrollDirection::Down,
		motion_rows..=motion_rows,
		config,
	)?;
	let max_overlap = previous.height().min(next.height());

	support::downward_registration_has_meaningful_overlap(matched, max_overlap, config)
		.then_some(matched)
}

pub(super) fn estimate_pairwise_downward_shift_rows(
	previous: &RgbaImage,
	current: &RgbaImage,
) -> Option<u32> {
	if previous.dimensions() != current.dimensions() {
		return None;
	}

	let (_width, height) = previous.dimensions();

	if height < 3 {
		return None;
	}

	let max_shift = height.saturating_sub(1);

	support::evaluate_overlap_direction(
		previous,
		current,
		ScrollDirection::Down,
		1..=max_shift,
		worker_pairwise_overlap_search_config(),
	)
	.map(|matched| matched.motion_rows)
}

pub(super) fn trusted_pairwise_downward_shift_rows_near_motion(
	previous: &RgbaImage,
	current: &RgbaImage,
	motion_rows: u32,
	tolerance_rows: u32,
) -> Option<u32> {
	match classify_pairwise_downward_shift_near_motion(
		previous,
		current,
		motion_rows,
		tolerance_rows,
	) {
		DownwardRegistration::Matched(matched) => Some(matched.motion_rows),
		DownwardRegistration::Ambiguous { .. } | DownwardRegistration::NoMatch => None,
	}
}

pub(super) fn trusted_pairwise_downward_shift_match(
	previous: &RgbaImage,
	current: &RgbaImage,
) -> Option<DirectionMatch> {
	trusted_pairwise_shift_match(previous, current, ScrollDirection::Down)
}

pub(super) fn trusted_pairwise_upward_shift_rows(
	previous: &RgbaImage,
	current: &RgbaImage,
) -> Option<u32> {
	let up_match = trusted_pairwise_shift_match(previous, current, ScrollDirection::Up)?;
	let down_match = trusted_pairwise_shift_match(previous, current, ScrollDirection::Down);

	if down_match.is_some_and(|down_match| {
		down_match.mean_abs_diff_x100
			<= up_match.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
	}) {
		return None;
	}

	Some(up_match.motion_rows)
}

fn classify_pairwise_downward_shift_near_motion(
	previous: &RgbaImage,
	current: &RgbaImage,
	motion_rows: u32,
	tolerance_rows: u32,
) -> DownwardRegistration {
	if previous.dimensions() != current.dimensions() {
		return DownwardRegistration::NoMatch;
	}

	let (_width, height) = previous.dimensions();

	if height < 3 {
		return DownwardRegistration::NoMatch;
	}

	let max_shift = height.saturating_sub(1);
	let start = motion_rows.saturating_sub(tolerance_rows).max(1);
	let end = motion_rows.saturating_add(tolerance_rows).min(max_shift).max(start);
	let candidates = support::collect_overlap_direction_matches(
		previous,
		current,
		ScrollDirection::Down,
		start..=end,
		worker_pairwise_overlap_search_config(),
	);

	support::classify_downward_registration_candidates(&candidates)
}

fn trusted_pairwise_shift_match(
	previous: &RgbaImage,
	current: &RgbaImage,
	direction: ScrollDirection,
) -> Option<DirectionMatch> {
	if previous.dimensions() != current.dimensions() {
		return None;
	}

	let (_width, height) = previous.dimensions();

	if height < 3 {
		return None;
	}

	let config = worker_pairwise_overlap_search_config();
	let max_shift = support::max_directional_motion_rows(previous, current, config);
	let candidates = support::collect_overlap_direction_matches(
		previous,
		current,
		direction,
		1..=max_shift,
		config,
	);

	match support::classify_downward_registration_candidates(&candidates) {
		DownwardRegistration::Matched(matched) => Some(matched),
		DownwardRegistration::Ambiguous { .. } | DownwardRegistration::NoMatch => None,
	}
}

fn worker_pairwise_overlap_search_config() -> OverlapSearchConfig {
	OverlapSearchConfig {
		min_overlap_rows: 24,
		max_column_samples: 240,
		max_row_samples: 128,
		max_mean_abs_diff_x100: 850,
	}
}
