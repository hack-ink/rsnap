use std::ops::RangeInclusive;
#[cfg(target_os = "macos")]
use std::ptr;

use color_eyre::eyre::{self, Result};
use image::{
	RgbaImage,
	imageops::{self, FilterType},
};
#[cfg(target_os = "macos")]
use objc2::{AnyThread, runtime::AnyObject};
#[cfg(target_os = "macos")]
use objc2_core_foundation::CFData;
#[cfg(target_os = "macos")]
use objc2_core_foundation::CFRetained;
#[cfg(target_os = "macos")]
use objc2_core_graphics::{
	CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
	CGImageByteOrderInfo,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSArray, NSDictionary};
#[cfg(target_os = "macos")]
use objc2_vision::{VNImageOption, VNImageRequestHandler, VNTranslationalImageRegistrationRequest};

#[cfg(test)]
use crate::scroll_capture::OverlapMatch;
use crate::scroll_capture::{
	DIRECTION_WARNING_MARGIN_X100, DOWNWARD_REGISTRATION_AMBIGUOUS_GAP_ROWS,
	DOWNWARD_REGISTRATION_MIN_OVERLAP_DIVISOR, DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS,
	DirectionMatch, DownwardRegistration, DownwardViewportCandidate,
	DownwardViewportCandidateSource, DownwardViewportResolution,
	INFORMATIVE_SPAN_HORIZONTAL_PADDING_PX, INFORMATIVE_SPAN_ROW_SAMPLES,
	INFORMATIVE_SPAN_SCORE_FLOOR_X100, InformativeSpan, OverlapSearchConfig,
	RESUME_DIRECT_PROOF_MAX_MEAN_ABS_DIFF_X100, ScrollDirection, ScrollFrameFingerprint,
	ScrollObserveOutcome,
};

const MOTION_COVERAGE_MIN_PERCENT: u32 = 20;
const MOTION_COVERAGE_MIN_INFORMATIVE_COLUMNS: u32 = 1;
const MOTION_COVERAGE_STATIC_EDGE_MAX_LEADING_COLUMNS: u32 = 48;
const MOTION_COVERAGE_STATIC_EDGE_MIN_COLUMNS: u32 = 64;
const MOTION_COVERAGE_STATIC_BAND_MIN_COLUMNS: usize = 64;
const MOTION_COVERAGE_STATIC_BAND_MIN_PERCENT: u32 = 65;
const MOTION_COVERAGE_STATIC_BAND_STRUCTURE_DIVISOR: u32 = 64;
const MOTION_COVERAGE_STATIC_BAND_MOTION_DIVISOR: u32 = 16;
const MOTION_OVERLAP_MIN_MATCHING_COLUMN_PERCENT: u32 = 80;
const MOTION_OVERLAP_BAD_EDGE_SAMPLE_DIVISOR: usize = 10;
const MOTION_OVERLAP_BAD_EDGE_MIN_SAMPLES: usize = 8;

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
		overlap_global_informative_span(previous, next),
	)
}

pub(crate) fn compose_provisional_preview_image(
	base_preview: &RgbaImage,
	latest_frame: Option<&RgbaImage>,
	motion_rows_hint: Option<u32>,
	preview_width_px: u32,
) -> RgbaImage {
	let Some(frame) = latest_frame else {
		return base_preview.clone();
	};
	let Some(motion_rows_hint) = motion_rows_hint else {
		return base_preview.clone();
	};
	let hinted_growth_rows = motion_rows_hint.min(frame.height());

	if hinted_growth_rows == 0 {
		return base_preview.clone();
	}

	let Some(strip) = crop_bottom_rows(frame, hinted_growth_rows) else {
		return base_preview.clone();
	};
	let preview_strip = resize_strip_to_preview_width(&strip, preview_width_px);

	append_vertical_image(base_preview, &preview_strip).unwrap_or_else(|_| base_preview.clone())
}

#[cfg(target_os = "macos")]
pub(super) fn classify_vision_downward_sample_motion_against(
	previous: &RgbaImage,
	next: &RgbaImage,
) -> Option<DirectionMatch> {
	let previous_cg = cg_image_from_rgba_image(previous).ok()?;
	let next_cg = cg_image_from_rgba_image(next).ok()?;
	let options = NSDictionary::<VNImageOption, AnyObject>::new();
	let request = unsafe {
		VNTranslationalImageRegistrationRequest::initWithTargetedCGImage_options(
			VNTranslationalImageRegistrationRequest::alloc(),
			previous_cg.as_ref(),
			options.as_ref(),
		)
	};
	let request_array = NSArray::from_retained_slice(&[request
		.clone()
		.into_super()
		.into_super()
		.into_super()
		.into_super()]);
	let handler = unsafe {
		VNImageRequestHandler::initWithCGImage_options(
			VNImageRequestHandler::alloc(),
			next_cg.as_ref(),
			options.as_ref(),
		)
	};

	handler.performRequests_error(request_array.as_ref()).ok()?;

	let results = unsafe { request.results() }?;

	if results.count() == 0 {
		return None;
	}

	let translation = unsafe { results.objectAtIndex(0).alignmentTransform() };
	let motion_rows = translation.ty.round();

	if !motion_rows.is_finite() || motion_rows <= 0.0 {
		return None;
	}

	let motion_rows = motion_rows as u32;
	let config = OverlapSearchConfig::default();
	let matched = evaluate_overlap_direction(
		previous,
		next,
		ScrollDirection::Down,
		motion_rows..=motion_rows,
		config,
	)?;
	let max_overlap = previous.height().min(next.height());

	downward_registration_has_meaningful_overlap(matched, max_overlap, config).then_some(matched)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn classify_vision_downward_sample_motion_against(
	_previous: &RgbaImage,
	_next: &RgbaImage,
) -> Option<DirectionMatch> {
	None
}

#[cfg(test)]
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

	evaluate_overlap_direction(
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

pub(super) fn select_downward_viewport_candidate(
	candidates: &mut [DownwardViewportCandidate],
) -> DownwardViewportResolution {
	if candidates.is_empty() {
		return DownwardViewportResolution::NoMatch;
	}

	if let Some(preferred_local) = prefer_local_downward_viewport_candidate(candidates) {
		let ambiguity_margin = downward_viewport_competing_margin(candidates, preferred_local);
		let competing = candidates.iter().copied().find(|candidate| {
			candidate != &preferred_local
				&& candidate.viewport_top_y.abs_diff(preferred_local.viewport_top_y)
					>= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
				&& candidate.mean_abs_diff_x100
					<= preferred_local.mean_abs_diff_x100.saturating_add(ambiguity_margin)
		});

		return match competing {
			Some(competing) => {
				DownwardViewportResolution::Ambiguous { preferred: preferred_local, competing }
			},
			None => DownwardViewportResolution::Selected(preferred_local),
		};
	}

	candidates.sort_by(|left, right| {
		left.mean_abs_diff_x100
			.cmp(&right.mean_abs_diff_x100)
			.then(left.source.priority().cmp(&right.source.priority()))
			.then(left.motion_rows.cmp(&right.motion_rows))
	});

	let preferred = candidates[0];
	let ambiguity_margin = downward_viewport_competing_margin(candidates, preferred);
	let competing = candidates.iter().copied().skip(1).find(|candidate| {
		candidate.viewport_top_y.abs_diff(preferred.viewport_top_y)
			>= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
			&& candidate.mean_abs_diff_x100
				<= preferred.mean_abs_diff_x100.saturating_add(ambiguity_margin)
	});

	match competing {
		Some(competing) => DownwardViewportResolution::Ambiguous { preferred, competing },
		None => DownwardViewportResolution::Selected(preferred),
	}
}

pub(super) fn format_downward_viewport_candidates(
	candidates: &[DownwardViewportCandidate],
) -> String {
	candidates
		.iter()
		.map(|candidate| {
			format!(
				"{:?}@{}/{}:{}",
				candidate.source,
				candidate.viewport_top_y,
				candidate.motion_rows,
				candidate.mean_abs_diff_x100
			)
		})
		.collect::<Vec<_>>()
		.join(",")
}

pub(super) fn best_local_downward_viewport_candidate(
	candidates: &[DownwardViewportCandidate],
) -> Option<DownwardViewportCandidate> {
	candidates
		.iter()
		.copied()
		.filter(|candidate| candidate.source != DownwardViewportCandidateSource::CommittedKeyframe)
		.min_by(|left, right| {
			left.mean_abs_diff_x100
				.cmp(&right.mean_abs_diff_x100)
				.then(left.source.priority().cmp(&right.source.priority()))
				.then(left.motion_rows.cmp(&right.motion_rows))
		})
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
	let Some(informative_span) = overlap_global_informative_span(previous, next) else {
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
	let sample_columns = motion_sample_columns_for_span(previous, next, informative_span, config);

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

pub(super) fn resize_strip_to_preview_width(strip: &RgbaImage, preview_width_px: u32) -> RgbaImage {
	if strip.width() <= preview_width_px {
		return strip.clone();
	}

	let preview_height = ((strip.height() as f32 / strip.width() as f32) * preview_width_px as f32)
		.round()
		.max(1.0) as u32;

	imageops::resize(strip, preview_width_px, preview_height, FilterType::Triangle)
}

pub(super) fn crop_bottom_rows(frame: &RgbaImage, rows: u32) -> Option<RgbaImage> {
	let rows = rows.min(frame.height());

	if rows == 0 {
		return None;
	}

	let start_y = frame.height().saturating_sub(rows);

	Some(imageops::crop_imm(frame, 0, start_y, frame.width(), rows).to_image())
}

pub(super) fn stack_vertical_images(images: &[&RgbaImage]) -> Result<RgbaImage> {
	let Some(first) = images.first() else {
		return Err(eyre::eyre!("cannot stack an empty image list"));
	};
	let width = first.width();
	let total_height = images.iter().try_fold(0_u32, |acc, image| {
		if image.width() != width {
			return Err(eyre::eyre!(
				"image width mismatch while stacking: expected {} got {}",
				width,
				image.width()
			));
		}

		acc.checked_add(image.height()).ok_or_else(|| eyre::eyre!("stacked image height overflow"))
	})?;
	let total_bytes = images.iter().try_fold(0_usize, |acc, image| {
		acc.checked_add(image.as_raw().len())
			.ok_or_else(|| eyre::eyre!("stacked image byte length overflow"))
	})?;
	let mut raw = Vec::with_capacity(total_bytes);

	for image in images {
		raw.extend_from_slice(image.as_raw());
	}

	RgbaImage::from_raw(width, total_height, raw)
		.ok_or_else(|| eyre::eyre!("failed to construct stacked image buffer"))
}

pub(super) fn append_vertical_image(base: &RgbaImage, strip: &RgbaImage) -> Result<RgbaImage> {
	if base.width() != strip.width() {
		return Err(eyre::eyre!(
			"image width mismatch while appending: expected {} got {}",
			base.width(),
			strip.width()
		));
	}

	stack_vertical_images(&[base, strip])
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

fn downward_viewport_competing_margin(
	candidates: &[DownwardViewportCandidate],
	preferred: DownwardViewportCandidate,
) -> u32 {
	let exact_corroborated = candidates.iter().any(|candidate| {
		candidate != &preferred
			&& candidate.viewport_top_y == preferred.viewport_top_y
			&& candidate.motion_rows == preferred.motion_rows
			&& candidate.mean_abs_diff_x100
				<= preferred.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
	});

	if exact_corroborated { 0 } else { DIRECTION_WARNING_MARGIN_X100 }
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
	let candidates = collect_overlap_direction_matches(
		previous,
		current,
		ScrollDirection::Down,
		start..=end,
		worker_pairwise_overlap_search_config(),
	);

	classify_downward_registration_candidates(&candidates)
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
	let max_shift = max_directional_motion_rows(previous, current, config);
	let candidates =
		collect_overlap_direction_matches(previous, current, direction, 1..=max_shift, config);

	match classify_downward_registration_candidates(&candidates) {
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

#[cfg(target_os = "macos")]
fn cg_image_from_rgba_image(image: &RgbaImage) -> Result<CFRetained<CGImage>> {
	let width = image.width() as usize;
	let height = image.height() as usize;

	if width == 0 || height == 0 {
		return Err(eyre::eyre!("vision registration image has zero dimensions"));
	}

	let bytes = CFData::from_bytes(image.as_raw());
	let provider = CGDataProvider::with_cf_data(Some(bytes.as_ref()))
		.ok_or_else(|| eyre::eyre!("failed to create CGDataProvider for Vision registration"))?;
	let color_space = CGColorSpace::new_device_rgb()
		.ok_or_else(|| eyre::eyre!("failed to create RGB colorspace for Vision registration"))?;
	let bitmap_info = CGBitmapInfo(CGImageAlphaInfo::Last.0 | CGImageByteOrderInfo::Order32Big.0);

	unsafe {
		CGImage::new(
			width,
			height,
			8,
			32,
			width.saturating_mul(4),
			Some(color_space.as_ref()),
			bitmap_info,
			Some(provider.as_ref()),
			ptr::null(),
			false,
			CGColorRenderingIntent::RenderingIntentDefault,
		)
	}
	.ok_or_else(|| eyre::eyre!("failed to create CGImage for Vision registration"))
}

fn prefer_local_downward_viewport_candidate(
	candidates: &[DownwardViewportCandidate],
) -> Option<DownwardViewportCandidate> {
	let local = best_local_downward_viewport_candidate(candidates)?;
	let committed = candidates
		.iter()
		.copied()
		.filter(|candidate| candidate.source == DownwardViewportCandidateSource::CommittedKeyframe)
		.min_by(|left, right| {
			left.mean_abs_diff_x100
				.cmp(&right.mean_abs_diff_x100)
				.then(left.motion_rows.cmp(&right.motion_rows))
		});
	let Some(committed) = committed else {
		return Some(local);
	};
	let committed_is_nearby = committed.viewport_top_y.abs_diff(local.viewport_top_y)
		< DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS;
	let committed_is_only_modestly_better =
		committed.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
			>= local.mean_abs_diff_x100;

	if committed_is_nearby && committed_is_only_modestly_better { Some(local) } else { None }
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
	let sample_columns = motion_sample_columns_for_span(previous, next, informative_span, config);
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
		let local_y = evenly_spaced_sample(0, overlap_rows, row, row_samples);
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

fn motion_sample_columns_for_span(
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

fn evenly_sampled_columns(x_start: u32, x_end: u32, max_column_samples: u32) -> Vec<u32> {
	let effective_width = x_end.saturating_sub(x_start).max(1);
	let column_samples = effective_width.min(max_column_samples).max(1);
	let mut columns = Vec::with_capacity(column_samples as usize);

	for column in 0..column_samples {
		columns.push(evenly_spaced_sample(x_start, x_end, column, column_samples));
	}

	columns
}

fn overlap_global_informative_span(left: &RgbaImage, right: &RgbaImage) -> Option<InformativeSpan> {
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
