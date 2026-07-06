use image::RgbaImage;

use crate::scroll_capture::{InformativeSpan, OverlapSearchConfig};

const INFORMATIVE_SPAN_ROW_SAMPLES: u32 = 24;
const INFORMATIVE_SPAN_SCORE_FLOOR_X100: u32 = 24;
const INFORMATIVE_SPAN_HORIZONTAL_PADDING_PX: u32 = 16;
const MOTION_COVERAGE_MIN_PERCENT: u32 = 20;
const MOTION_COVERAGE_MIN_INFORMATIVE_COLUMNS: u32 = 1;
const MOTION_COVERAGE_STATIC_EDGE_MAX_LEADING_COLUMNS: u32 = 48;
const MOTION_COVERAGE_STATIC_EDGE_MIN_COLUMNS: u32 = 64;
const MOTION_COVERAGE_STATIC_BAND_MIN_COLUMNS: usize = 64;
const MOTION_COVERAGE_STATIC_BAND_MIN_PERCENT: u32 = 65;
const MOTION_COVERAGE_STATIC_BAND_STRUCTURE_DIVISOR: u32 = 64;
const MOTION_COVERAGE_STATIC_BAND_MOTION_DIVISOR: u32 = 16;

#[derive(Clone, Copy, Debug)]
struct MotionCoverageColumnScore {
	structure_score: u32,
	motion_score: u32,
}
impl MotionCoverageColumnScore {
	fn has_structure(self, threshold: u32) -> bool {
		self.structure_score >= threshold
	}

	fn has_motion(self, threshold: u32) -> bool {
		self.motion_score >= threshold
	}

	fn is_static(self, structure_threshold: u32, motion_threshold: u32) -> bool {
		self.has_structure(structure_threshold) && self.motion_score <= motion_threshold
	}
}

pub(super) fn informative_column_span(
	image: &RgbaImage,
	start_y: u32,
	rows: u32,
) -> Option<InformativeSpan> {
	if image.width() == 0 || image.height() == 0 || rows == 0 {
		return None;
	}

	let clamped_rows = rows.min(image.height().saturating_sub(start_y)).max(1);
	let row_samples = clamped_rows.min(INFORMATIVE_SPAN_ROW_SAMPLES.max(2)).max(2);
	let mut scores = vec![0_u32; image.width() as usize];
	let mut max_score = 0_u32;

	for row in 0..row_samples.saturating_sub(1) {
		let local_y = evenly_spaced_sample(0, clamped_rows, row, row_samples);
		let next_local_y = (local_y.saturating_add(1)).min(clamped_rows.saturating_sub(1));
		let y = start_y.saturating_add(local_y).min(image.height().saturating_sub(1));
		let next_y = start_y.saturating_add(next_local_y).min(image.height().saturating_sub(1));

		for x in 0..image.width() {
			let pixel = image.get_pixel(x, y).0;
			let next_pixel = image.get_pixel(x, next_y).0;
			let score = u32::from(pixel[0].abs_diff(next_pixel[0]))
				.saturating_add(u32::from(pixel[1].abs_diff(next_pixel[1])))
				.saturating_add(u32::from(pixel[2].abs_diff(next_pixel[2])));
			let slot = &mut scores[x as usize];

			*slot = slot.saturating_add(score);
			max_score = max_score.max(*slot);
		}
	}

	if max_score == 0 {
		return None;
	}

	let threshold = (max_score / 6).max(INFORMATIVE_SPAN_SCORE_FLOOR_X100);
	let mut start_x = None;
	let mut end_x = None;

	for (x, score) in scores.iter().enumerate() {
		if *score >= threshold {
			start_x.get_or_insert(x as u32);

			end_x = Some((x as u32).saturating_add(1));
		}
	}

	let start_x = start_x?;
	let end_exclusive_x = end_x?;
	let padding = INFORMATIVE_SPAN_HORIZONTAL_PADDING_PX.min(image.width() / 8);
	let start_x = start_x.saturating_sub(padding);
	let end_exclusive_x =
		end_exclusive_x.saturating_add(padding).min(image.width()).max(start_x.saturating_add(1));

	Some(InformativeSpan { start_x, end_exclusive_x })
}

pub(super) fn evenly_spaced_sample(start: u32, end_exclusive: u32, index: u32, count: u32) -> u32 {
	let span = end_exclusive.saturating_sub(start).max(1);

	if count <= 1 {
		return start.min(end_exclusive.saturating_sub(1));
	}

	let numerator =
		(u64::from(index) * u64::from(span.saturating_sub(1))) / u64::from(count.saturating_sub(1));

	start.saturating_add(numerator as u32).min(end_exclusive.saturating_sub(1))
}

pub(super) fn motion_sample_columns_for_span(
	previous: &RgbaImage,
	next: &RgbaImage,
	informative_span: InformativeSpan,
	config: OverlapSearchConfig,
) -> Vec<u32> {
	let width = previous.width().min(next.width());

	if width == 0 {
		return Vec::new();
	}

	let x_start = informative_span.start_x.min(width.saturating_sub(1));
	let x_end = informative_span.end_exclusive_x.min(width).max(x_start + 1);
	let column_samples = width.min(config.max_column_samples).max(1);

	evenly_sampled_columns(x_start, x_end, column_samples)
}

pub(super) fn overlap_global_informative_span(
	left: &RgbaImage,
	right: &RgbaImage,
) -> Option<InformativeSpan> {
	let left_span = informative_column_span(left, 0, left.height());
	let right_span = informative_column_span(right, 0, right.height());
	let width = left.width().min(right.width());
	let structural_span = match (left_span, right_span) {
		(Some(left_span), Some(right_span)) => {
			let start_x = left_span.start_x.max(right_span.start_x);
			let end_exclusive_x =
				left_span.end_exclusive_x.min(right_span.end_exclusive_x).min(width);

			(end_exclusive_x > start_x).then_some(InformativeSpan { start_x, end_exclusive_x })?
		},
		(Some(span), None) | (None, Some(span)) => {
			let end_exclusive_x = span.end_exclusive_x.min(width).max(span.start_x + 1);

			(end_exclusive_x > span.start_x)
				.then_some(InformativeSpan { start_x: span.start_x, end_exclusive_x })?
		},
		(None, None) => return None,
	};

	motion_coverage_supports_structural_span(left, right, structural_span)
		.then_some(structural_span)
}

fn evenly_sampled_columns(x_start: u32, x_end: u32, max_column_samples: u32) -> Vec<u32> {
	let effective_width = x_end.saturating_sub(x_start).max(1);
	let column_samples = effective_width.min(max_column_samples).max(1);
	let mut columns = Vec::with_capacity(column_samples as usize);

	for column in 0..column_samples {
		columns.push(evenly_spaced_sample(x_start, x_end, column, column_samples));
	}

	columns
}

fn motion_coverage_supports_structural_span(
	left: &RgbaImage,
	right: &RgbaImage,
	structural_span: InformativeSpan,
) -> bool {
	let width = left.width().min(right.width());
	let height = left.height().min(right.height());
	let x_start = structural_span.start_x.min(width.saturating_sub(1));
	let x_end = structural_span.end_exclusive_x.min(width).max(x_start.saturating_add(1));

	if width == 0 || height == 0 {
		return false;
	}

	let row_samples = height.min(INFORMATIVE_SPAN_ROW_SAMPLES.max(2)).max(2);
	let mut scores = Vec::with_capacity(width as usize);
	let mut max_structure_score = 0_u32;
	let mut max_motion_score = 0_u32;

	for x in 0..width {
		let mut structure_score = 0_u32;
		let mut motion_score = 0_u32;

		for row in 0..row_samples {
			let y = evenly_spaced_sample(0, height, row, row_samples);
			let next_y = y.saturating_add(1).min(height.saturating_sub(1));
			let left_pixel = left.get_pixel(x, y).0;
			let right_pixel = right.get_pixel(x, y).0;
			let left_next_pixel = left.get_pixel(x, next_y).0;
			let right_next_pixel = right.get_pixel(x, next_y).0;

			motion_score = motion_score
				.saturating_add(u32::from(left_pixel[0].abs_diff(right_pixel[0])))
				.saturating_add(u32::from(left_pixel[1].abs_diff(right_pixel[1])))
				.saturating_add(u32::from(left_pixel[2].abs_diff(right_pixel[2])));
			structure_score = structure_score
				.saturating_add(u32::from(left_pixel[0].abs_diff(left_next_pixel[0])))
				.saturating_add(u32::from(left_pixel[1].abs_diff(left_next_pixel[1])))
				.saturating_add(u32::from(left_pixel[2].abs_diff(left_next_pixel[2])))
				.saturating_add(u32::from(right_pixel[0].abs_diff(right_next_pixel[0])))
				.saturating_add(u32::from(right_pixel[1].abs_diff(right_next_pixel[1])))
				.saturating_add(u32::from(right_pixel[2].abs_diff(right_next_pixel[2])));
		}

		max_structure_score = max_structure_score.max(structure_score);
		max_motion_score = max_motion_score.max(motion_score);

		scores.push(MotionCoverageColumnScore { structure_score, motion_score });
	}

	if max_structure_score == 0 || max_motion_score == 0 {
		return false;
	}
	if raw_frame_pair_has_static_informative_band(&scores, max_structure_score, max_motion_score) {
		return false;
	}

	let structure_threshold = (max_structure_score / 8).max(1);
	let motion_threshold = (max_motion_score / 8).max(1);
	let span_scores = &scores[x_start as usize..x_end as usize];
	let mut informative_columns = 0_u32;
	let mut moving_informative_columns = 0_u32;

	if raw_frame_pair_has_static_informative_edge(
		span_scores,
		structure_threshold,
		motion_threshold,
		x_start,
		width.saturating_sub(x_end),
	) {
		return false;
	}

	for &score in span_scores {
		if !score.has_structure(structure_threshold) {
			continue;
		}

		informative_columns = informative_columns.saturating_add(1);

		if score.has_motion(motion_threshold) {
			moving_informative_columns = moving_informative_columns.saturating_add(1);
		}
	}

	informative_columns >= MOTION_COVERAGE_MIN_INFORMATIVE_COLUMNS
		&& moving_informative_columns.saturating_mul(100)
			>= informative_columns.saturating_mul(MOTION_COVERAGE_MIN_PERCENT)
}

fn raw_frame_pair_has_static_informative_band(
	scores: &[MotionCoverageColumnScore],
	max_structure_score: u32,
	max_motion_score: u32,
) -> bool {
	if scores.len() < MOTION_COVERAGE_STATIC_BAND_MIN_COLUMNS
		|| max_structure_score == 0
		|| max_motion_score == 0
	{
		return false;
	}

	let structure_threshold =
		(max_structure_score / MOTION_COVERAGE_STATIC_BAND_STRUCTURE_DIVISOR).max(1);
	let motion_threshold = (max_motion_score / MOTION_COVERAGE_STATIC_BAND_MOTION_DIVISOR).max(1);
	let moving_motion_threshold = motion_threshold.saturating_add(1);
	let mut moving_start = None;
	let mut moving_end = None;
	let mut static_flags = Vec::with_capacity(scores.len());

	for (column, score) in scores.iter().enumerate() {
		if score.has_structure(structure_threshold) && score.has_motion(moving_motion_threshold) {
			moving_start.get_or_insert(column);

			moving_end = Some(column.saturating_add(1));
		}

		static_flags.push(score.is_static(structure_threshold, motion_threshold));
	}

	let Some(moving_start) = moving_start else {
		return false;
	};
	let Some(moving_end) = moving_end else {
		return false;
	};
	let mut static_columns = static_flags[..MOTION_COVERAGE_STATIC_BAND_MIN_COLUMNS]
		.iter()
		.filter(|is_static| **is_static)
		.count();

	if static_side_band_has_enough_columns(static_columns, 0, moving_start, moving_end) {
		return true;
	}

	for end in MOTION_COVERAGE_STATIC_BAND_MIN_COLUMNS..static_flags.len() {
		if static_flags[end - MOTION_COVERAGE_STATIC_BAND_MIN_COLUMNS] {
			static_columns = static_columns.saturating_sub(1);
		}
		if static_flags[end] {
			static_columns = static_columns.saturating_add(1);
		}

		let start = end.saturating_add(1).saturating_sub(MOTION_COVERAGE_STATIC_BAND_MIN_COLUMNS);

		if static_side_band_has_enough_columns(static_columns, start, moving_start, moving_end) {
			return true;
		}
	}

	false
}

fn static_side_band_has_enough_columns(
	static_columns: usize,
	start: usize,
	moving_start: usize,
	moving_end: usize,
) -> bool {
	let end = start.saturating_add(MOTION_COVERAGE_STATIC_BAND_MIN_COLUMNS);
	let enough_static_columns = (static_columns as u32).saturating_mul(100)
		>= (MOTION_COVERAGE_STATIC_BAND_MIN_COLUMNS as u32)
			.saturating_mul(MOTION_COVERAGE_STATIC_BAND_MIN_PERCENT);
	let side_of_moving_span = end <= moving_start || start >= moving_end;

	enough_static_columns && side_of_moving_span
}

fn raw_frame_pair_has_static_informative_edge(
	scores: &[MotionCoverageColumnScore],
	structure_threshold: u32,
	motion_threshold: u32,
	left_leading_columns: u32,
	right_leading_columns: u32,
) -> bool {
	raw_static_edge_run_len(
		scores.iter().copied(),
		structure_threshold,
		motion_threshold,
		left_leading_columns,
	) >= MOTION_COVERAGE_STATIC_EDGE_MIN_COLUMNS
		|| raw_static_edge_run_len(
			scores.iter().rev().copied(),
			structure_threshold,
			motion_threshold,
			right_leading_columns,
		) >= MOTION_COVERAGE_STATIC_EDGE_MIN_COLUMNS
}

fn raw_static_edge_run_len<I>(
	iter: I,
	structure_threshold: u32,
	motion_threshold: u32,
	leading_columns: u32,
) -> u32
where
	I: IntoIterator<Item = MotionCoverageColumnScore>,
{
	let mut skipped_columns = leading_columns;
	let mut static_columns = 0_u32;
	let mut seen_informative = false;

	for score in iter {
		if !score.has_structure(structure_threshold) {
			if seen_informative {
				break;
			}

			skipped_columns = skipped_columns.saturating_add(1);

			if skipped_columns > MOTION_COVERAGE_STATIC_EDGE_MAX_LEADING_COLUMNS {
				return 0;
			}

			continue;
		}
		if skipped_columns > MOTION_COVERAGE_STATIC_EDGE_MAX_LEADING_COLUMNS {
			return 0;
		}

		seen_informative = true;

		if score.has_motion(motion_threshold) {
			break;
		}

		static_columns = static_columns.saturating_add(1);
	}

	static_columns
}
