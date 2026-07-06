use std::ops::RangeInclusive;

use image::RgbaImage;

#[cfg(test)]
use crate::scroll_capture::InformativeSpan;
#[cfg(test)]
use crate::scroll_capture::OverlapMatch;
use crate::scroll_capture::informative_span;
use crate::scroll_capture::{
	DIRECTION_WARNING_MARGIN_X100, DOWNWARD_REGISTRATION_AMBIGUOUS_GAP_ROWS,
	DOWNWARD_REGISTRATION_MIN_OVERLAP_DIVISOR, DirectionMatch, DownwardRegistration,
	OverlapSearchConfig, RESUME_DIRECT_PROOF_MAX_MEAN_ABS_DIFF_X100, ScrollDirection,
	ScrollFrameFingerprint, ScrollObserveOutcome,
};

const MOTION_OVERLAP_MIN_MATCHING_COLUMN_PERCENT: u32 = 80;
const MOTION_OVERLAP_BAD_EDGE_SAMPLE_DIVISOR: usize = 10;
const MOTION_OVERLAP_BAD_EDGE_MIN_SAMPLES: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OverlapOrientation {
	PreviousBottomToNextTop,
	PreviousTopToNextBottom,
}

#[must_use]
pub(crate) fn scroll_capture_fingerprint(image: &RgbaImage) -> Vec<u8> {
	ScrollFrameFingerprint::from_image(image).into_bytes()
}

#[must_use]
pub(crate) fn scroll_capture_fingerprint_delta(left: &[u8], right: &[u8]) -> u32 {
	if left.len() != right.len() || left.is_empty() || !left.len().is_multiple_of(4) {
		return u32::MAX;
	}

	let mut total_abs_diff = 0_u64;
	let mut comparisons = 0_u64;

	for (left_pixel, right_pixel) in left.chunks_exact(4).zip(right.chunks_exact(4)) {
		total_abs_diff = total_abs_diff
			.saturating_add(u64::from(left_pixel[0].abs_diff(right_pixel[0])))
			.saturating_add(u64::from(left_pixel[1].abs_diff(right_pixel[1])))
			.saturating_add(u64::from(left_pixel[2].abs_diff(right_pixel[2])))
			.saturating_add(u64::from(left_pixel[3].abs_diff(right_pixel[3])));
		comparisons = comparisons.saturating_add(4);
	}

	total_abs_diff.checked_div(comparisons).map_or(u32::MAX, |mean_abs_diff| mean_abs_diff as u32)
}

#[cfg(test)]
#[must_use]
pub(crate) fn detect_vertical_overlap(
	previous: &RgbaImage,
	next: &RgbaImage,
	config: OverlapSearchConfig,
) -> OverlapMatch {
	detect_vertical_overlap_in_range(
		previous,
		next,
		1..=previous.height().min(next.height()),
		ScrollDirection::Down,
		config,
		informative_span::overlap_global_informative_span(previous, next),
	)
}

pub(super) fn evaluate_overlap_direction(
	previous: &RgbaImage,
	next: &RgbaImage,
	direction: ScrollDirection,
	range: RangeInclusive<u32>,
	config: OverlapSearchConfig,
) -> Option<DirectionMatch> {
	collect_overlap_direction_matches(previous, next, direction, range, config).into_iter().next()
}

pub(super) fn collect_overlap_direction_matches(
	previous: &RgbaImage,
	next: &RgbaImage,
	direction: ScrollDirection,
	range: RangeInclusive<u32>,
	config: OverlapSearchConfig,
) -> Vec<DirectionMatch> {
	let Some(informative_span) = informative_span::overlap_global_informative_span(previous, next)
	else {
		return Vec::new();
	};
	let max_overlap = previous.height().min(next.height());
	let effective_min_overlap =
		if max_overlap <= config.min_overlap_rows { 1 } else { config.min_overlap_rows.max(1) };
	let max_motion_rows = max_overlap.saturating_sub(effective_min_overlap).max(1);
	let search_start = (*range.start()).max(1).min(max_motion_rows);
	let search_end = (*range.end()).max(search_start).min(max_motion_rows);
	let orientation = match direction {
		ScrollDirection::Down => OverlapOrientation::PreviousBottomToNextTop,
		ScrollDirection::Up => OverlapOrientation::PreviousTopToNextBottom,
	};
	let sample_columns =
		informative_span::motion_sample_columns_for_span(previous, next, informative_span, config);

	if sample_columns.is_empty() {
		return Vec::new();
	}

	let mut matches = Vec::with_capacity(search_end.saturating_sub(search_start) as usize + 1);

	for motion_rows in search_start..=search_end {
		let overlap_rows = max_overlap.saturating_sub(motion_rows);

		if overlap_rows < effective_min_overlap {
			continue;
		}

		let diff = motion_mean_abs_diff_x100(
			previous,
			next,
			motion_rows,
			config,
			orientation,
			&sample_columns,
		);

		if diff > config.max_mean_abs_diff_x100 {
			continue;
		}

		matches.push(DirectionMatch { mean_abs_diff_x100: diff, motion_rows });
	}

	matches.sort_by(|left, right| {
		left.mean_abs_diff_x100
			.cmp(&right.mean_abs_diff_x100)
			.then(left.motion_rows.cmp(&right.motion_rows))
	});

	matches
}

pub(super) fn collect_overlap_direction_matches_in_ranges(
	previous: &RgbaImage,
	next: &RgbaImage,
	direction: ScrollDirection,
	ranges: &[RangeInclusive<u32>],
	config: OverlapSearchConfig,
) -> Vec<DirectionMatch> {
	let mut matches = Vec::new();

	for range in ranges {
		matches.extend(collect_overlap_direction_matches(
			previous,
			next,
			direction,
			range.clone(),
			config,
		));
	}

	if matches.len() <= 1 {
		return matches;
	}

	matches.sort_by(|left, right| {
		left.motion_rows
			.cmp(&right.motion_rows)
			.then(left.mean_abs_diff_x100.cmp(&right.mean_abs_diff_x100))
	});

	let mut deduped: Vec<DirectionMatch> = Vec::with_capacity(matches.len());

	for matched in matches {
		if let Some(previous) = deduped.last_mut()
			&& previous.motion_rows == matched.motion_rows
		{
			if matched.mean_abs_diff_x100 < previous.mean_abs_diff_x100 {
				*previous = matched;
			}

			continue;
		}

		deduped.push(matched);
	}

	deduped.sort_by(|left, right| {
		left.mean_abs_diff_x100
			.cmp(&right.mean_abs_diff_x100)
			.then(left.motion_rows.cmp(&right.motion_rows))
	});

	deduped
}

pub(super) fn classify_downward_registration_candidates(
	candidates: &[DirectionMatch],
) -> DownwardRegistration {
	let Some(best) = candidates.first().copied() else {
		return DownwardRegistration::NoMatch;
	};
	let competing = candidates.iter().copied().skip(1).find(|candidate| {
		candidate.motion_rows.abs_diff(best.motion_rows) >= DOWNWARD_REGISTRATION_AMBIGUOUS_GAP_ROWS
	});

	match competing {
		Some(competing)
			if best.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
				>= competing.mean_abs_diff_x100 =>
		{
			DownwardRegistration::Ambiguous { best, competing }
		},
		_ => DownwardRegistration::Matched(best),
	}
}

pub(super) fn downward_registration_has_meaningful_overlap(
	matched: DirectionMatch,
	max_overlap: u32,
	config: OverlapSearchConfig,
) -> bool {
	let overlap_rows = max_overlap.saturating_sub(matched.motion_rows);
	let effective_min_overlap =
		if max_overlap <= config.min_overlap_rows { 1 } else { config.min_overlap_rows.max(1) };
	let min_overlap_rows =
		effective_min_overlap.max(max_overlap / DOWNWARD_REGISTRATION_MIN_OVERLAP_DIVISOR).max(1);

	overlap_rows >= min_overlap_rows
}

pub(super) fn preview_update_outcome(preview_changed: bool) -> ScrollObserveOutcome {
	if preview_changed {
		ScrollObserveOutcome::PreviewUpdated
	} else {
		ScrollObserveOutcome::NoChange
	}
}

pub(super) fn resume_direct_match_is_trustworthy(matched: DirectionMatch) -> bool {
	matched.mean_abs_diff_x100 <= RESUME_DIRECT_PROOF_MAX_MEAN_ABS_DIFF_X100
}

pub(super) fn preferred_upward_override_match(
	up_match: Option<DirectionMatch>,
	down_match: Option<DirectionMatch>,
) -> Option<DirectionMatch> {
	match (up_match, down_match) {
		(Some(up), Some(_down)) if resume_direct_match_is_trustworthy(up) => Some(up),
		(Some(up), None) if resume_direct_match_is_trustworthy(up) => Some(up),
		_ => None,
	}
}

pub(super) fn preferred_upward_input_override_match(
	sample_match: Option<DirectionMatch>,
	committed_match: Option<DirectionMatch>,
) -> Option<(DirectionMatch, bool)> {
	match (sample_match, committed_match) {
		(Some(sample), Some(committed))
			if committed.motion_rows <= sample.motion_rows
				&& committed.mean_abs_diff_x100
					<= sample.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100) =>
		{
			Some((committed, true))
		},
		(Some(sample), Some(_committed)) => Some((sample, false)),
		(Some(sample), None) => Some((sample, false)),
		(None, Some(committed)) => Some((committed, true)),
		(None, None) => None,
	}
}

pub(super) fn upward_confirmation_match_for_downward_input(
	up_match: Option<DirectionMatch>,
	down_match: Option<DirectionMatch>,
	has_committed_growth: bool,
) -> Option<DirectionMatch> {
	if !has_committed_growth {
		return None;
	}

	match (up_match, down_match) {
		(Some(up), Some(down))
			if resume_direct_match_is_trustworthy(up)
				&& up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					<= down.mean_abs_diff_x100 =>
		{
			Some(up)
		},
		(Some(up), None) if resume_direct_match_is_trustworthy(up) => Some(up),
		_ => None,
	}
}

pub(super) fn rewind_active_upward_override_match(
	sample_match: Option<DirectionMatch>,
	committed_match: Option<DirectionMatch>,
	rewind_active: bool,
) -> Option<(DirectionMatch, bool)> {
	if !rewind_active {
		return None;
	}

	match (sample_match, committed_match) {
		(Some(sample), Some(committed))
			if committed.motion_rows < sample.motion_rows
				&& committed.mean_abs_diff_x100
					<= sample.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100) =>
		{
			Some((committed, true))
		},
		(Some(sample), _) => Some((sample, false)),
		(None, Some(committed)) => Some((committed, true)),
		(None, None) => None,
	}
}

pub(super) fn rewind_active_upward_motion_should_fail_closed(
	sample_up_match: Option<DirectionMatch>,
	committed_up_match: Option<DirectionMatch>,
	committed_down_match: Option<DirectionMatch>,
	rewind_active: bool,
) -> bool {
	if !rewind_active {
		return false;
	}
	if committed_up_match.is_some() {
		return false;
	}

	matches!(
		(sample_up_match, committed_down_match),
		(Some(sample_up), Some(committed_down))
			if committed_down.mean_abs_diff_x100
				<= sample_up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
				&& committed_down.motion_rows >= sample_up.motion_rows
	)
}

pub(super) fn max_directional_motion_rows(
	previous: &RgbaImage,
	next: &RgbaImage,
	config: OverlapSearchConfig,
) -> u32 {
	let max_overlap = previous.height().min(next.height());
	let effective_min_overlap =
		if max_overlap <= config.min_overlap_rows { 1 } else { config.min_overlap_rows.max(1) };

	max_overlap.saturating_sub(effective_min_overlap).max(1)
}

#[cfg(test)]
fn detect_vertical_overlap_in_range(
	previous: &RgbaImage,
	next: &RgbaImage,
	range: RangeInclusive<u32>,
	direction: ScrollDirection,
	config: OverlapSearchConfig,
	informative_span: Option<InformativeSpan>,
) -> OverlapMatch {
	if previous.width() == 0 || next.width() == 0 || previous.height() == 0 || next.height() == 0 {
		return OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };
	}

	let Some(informative_span) = informative_span else {
		return OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };
	};
	let max_overlap = previous.height().min(next.height());
	let effective_min_overlap =
		if max_overlap <= config.min_overlap_rows { 1 } else { config.min_overlap_rows.max(1) };
	let max_motion_rows = max_overlap.saturating_sub(effective_min_overlap).max(1);
	let search_start = (*range.start()).max(1).min(max_motion_rows);
	let search_end = (*range.end()).max(search_start).min(max_motion_rows);
	let orientation = match direction {
		ScrollDirection::Down => OverlapOrientation::PreviousBottomToNextTop,
		ScrollDirection::Up => OverlapOrientation::PreviousTopToNextBottom,
	};
	let sample_columns =
		informative_span::motion_sample_columns_for_span(previous, next, informative_span, config);
	let mut best = OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };

	if sample_columns.is_empty() {
		return best;
	}

	for motion_rows in search_start..=search_end {
		let overlap_rows = max_overlap.saturating_sub(motion_rows);

		if overlap_rows < effective_min_overlap {
			continue;
		}

		let diff = motion_mean_abs_diff_x100(
			previous,
			next,
			motion_rows,
			config,
			orientation,
			&sample_columns,
		);

		if diff > config.max_mean_abs_diff_x100 {
			continue;
		}
		if !best.matched
			|| diff < best.mean_abs_diff_x100
			|| (diff == best.mean_abs_diff_x100 && overlap_rows > best.rows)
		{
			best = OverlapMatch { rows: overlap_rows, matched: true, mean_abs_diff_x100: diff };
		}
	}

	best
}

fn motion_mean_abs_diff_x100(
	previous: &RgbaImage,
	next: &RgbaImage,
	motion_rows: u32,
	config: OverlapSearchConfig,
	orientation: OverlapOrientation,
	sample_columns: &[u32],
) -> u32 {
	let max_overlap = previous.height().min(next.height());
	let overlap_rows = max_overlap.saturating_sub(motion_rows);

	if overlap_rows == 0 {
		return u32::MAX;
	}

	let row_samples = overlap_rows.min(config.max_row_samples).max(1);
	let previous_overlap_start_y = previous.height().saturating_sub(overlap_rows);
	let next_overlap_start_y = next.height().saturating_sub(overlap_rows);
	let previous_start_y = match orientation {
		OverlapOrientation::PreviousBottomToNextTop => previous_overlap_start_y,
		OverlapOrientation::PreviousTopToNextBottom => 0,
	};
	let next_start_y = match orientation {
		OverlapOrientation::PreviousBottomToNextTop => 0,
		OverlapOrientation::PreviousTopToNextBottom => next_overlap_start_y,
	};
	let mut total_abs_diff = 0_u64;
	let mut comparisons = 0_u64;
	let mut column_abs_diff = vec![0_u64; sample_columns.len()];
	let mut column_comparisons = 0_u64;

	for row in 0..row_samples {
		let local_y = informative_span::evenly_spaced_sample(0, overlap_rows, row, row_samples);
		let previous_y =
			previous_start_y.saturating_add(local_y).min(previous.height().saturating_sub(1));
		let next_y = next_start_y.saturating_add(local_y).min(next.height().saturating_sub(1));

		for (column_index, x) in sample_columns.iter().enumerate() {
			let previous_pixel = previous.get_pixel(*x, previous_y).0;
			let next_pixel = next.get_pixel(*x, next_y).0;
			let pixel_abs_diff = u64::from(previous_pixel[0].abs_diff(next_pixel[0]))
				.saturating_add(u64::from(previous_pixel[1].abs_diff(next_pixel[1])))
				.saturating_add(u64::from(previous_pixel[2].abs_diff(next_pixel[2])));

			total_abs_diff = total_abs_diff.saturating_add(pixel_abs_diff);
			column_abs_diff[column_index] =
				column_abs_diff[column_index].saturating_add(pixel_abs_diff);
			comparisons = comparisons.saturating_add(3);
		}

		column_comparisons = column_comparisons.saturating_add(3);
	}

	if comparisons == 0 {
		return u32::MAX;
	}
	if !motion_overlap_columns_support_span(&column_abs_diff, column_comparisons, config) {
		return u32::MAX;
	}

	((total_abs_diff.saturating_mul(100)) / comparisons) as u32
}

fn motion_overlap_columns_support_span(
	column_abs_diff: &[u64],
	column_comparisons: u64,
	config: OverlapSearchConfig,
) -> bool {
	if column_abs_diff.is_empty() || column_comparisons == 0 {
		return false;
	}

	let bad_column_threshold = config
		.max_mean_abs_diff_x100
		.saturating_mul(4)
		.max(config.max_mean_abs_diff_x100.saturating_add(1));
	let mut matching_columns = 0_u32;
	let mut bad_columns = Vec::with_capacity(column_abs_diff.len());

	for total in column_abs_diff {
		let column_mean_x100 = ((total.saturating_mul(100)) / column_comparisons) as u32;
		let column_matches = column_mean_x100 <= bad_column_threshold;

		if column_matches {
			matching_columns = matching_columns.saturating_add(1);
		}

		bad_columns.push(!column_matches);
	}

	let total_columns = column_abs_diff.len() as u32;
	let enough_matching_columns = matching_columns.saturating_mul(100)
		>= total_columns.saturating_mul(MOTION_OVERLAP_MIN_MATCHING_COLUMN_PERCENT);
	let min_bad_edge_columns = (column_abs_diff.len() / MOTION_OVERLAP_BAD_EDGE_SAMPLE_DIVISOR)
		.max(MOTION_OVERLAP_BAD_EDGE_MIN_SAMPLES)
		.min(column_abs_diff.len());

	enough_matching_columns
		&& leading_true_run_len(bad_columns.iter().copied()) < min_bad_edge_columns
		&& leading_true_run_len(bad_columns.iter().rev().copied()) < min_bad_edge_columns
}

fn leading_true_run_len<I>(iter: I) -> usize
where
	I: IntoIterator<Item = bool>,
{
	let mut len = 0_usize;

	for value in iter {
		if !value {
			break;
		}

		len = len.saturating_add(1);
	}

	len
}
