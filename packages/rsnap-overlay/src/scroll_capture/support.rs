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

	total_abs_diff.checked_div(comparisons).map_or(u32::MAX, |average| average as u32)
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

pub(super) fn select_downward_viewport_candidate(
	candidates: &mut [DownwardViewportCandidate],
) -> DownwardViewportResolution {
	if candidates.is_empty() {
		return DownwardViewportResolution::NoMatch;
	}

	if let Some(preferred_local) = prefer_local_downward_viewport_candidate(candidates) {
		let competing = candidates.iter().copied().find(|candidate| {
			candidate != &preferred_local
				&& candidate.viewport_top_y.abs_diff(preferred_local.viewport_top_y)
					>= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
				&& candidate.mean_abs_diff_x100
					<= preferred_local
						.mean_abs_diff_x100
						.saturating_add(DIRECTION_WARNING_MARGIN_X100)
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
	let competing = candidates.iter().copied().skip(1).find(|candidate| {
		candidate.viewport_top_y.abs_diff(preferred.viewport_top_y)
			>= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
			&& candidate.mean_abs_diff_x100
				<= preferred.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
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
			informative_span,
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

fn worker_pairwise_overlap_search_config() -> OverlapSearchConfig {
	OverlapSearchConfig {
		min_overlap_rows: 24,
		max_column_samples: 96,
		max_row_samples: 96,
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
	let mut best = OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };

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
			informative_span,
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
	informative_span: InformativeSpan,
) -> u32 {
	let width = previous.width().min(next.width());
	let max_overlap = previous.height().min(next.height());
	let overlap_rows = max_overlap.saturating_sub(motion_rows);

	if overlap_rows == 0 {
		return u32::MAX;
	}

	let column_samples = width.min(config.max_column_samples).max(1);
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
	let x_start = informative_span.start_x.min(width.saturating_sub(1));
	let x_end = informative_span.end_exclusive_x.min(width).max(x_start + 1);
	let effective_width = x_end.saturating_sub(x_start).max(1);
	let column_samples = effective_width.min(column_samples).max(1);
	let mut total_abs_diff = 0_u64;
	let mut comparisons = 0_u64;

	for row in 0..row_samples {
		let local_y = evenly_spaced_sample(0, overlap_rows, row, row_samples);
		let previous_y =
			previous_start_y.saturating_add(local_y).min(previous.height().saturating_sub(1));
		let next_y = next_start_y.saturating_add(local_y).min(next.height().saturating_sub(1));

		for column in 0..column_samples {
			let x = evenly_spaced_sample(x_start, x_end, column, column_samples);
			let previous_pixel = previous.get_pixel(x, previous_y).0;
			let next_pixel = next.get_pixel(x, next_y).0;

			total_abs_diff = total_abs_diff
				.saturating_add(u64::from(previous_pixel[0].abs_diff(next_pixel[0])))
				.saturating_add(u64::from(previous_pixel[1].abs_diff(next_pixel[1])))
				.saturating_add(u64::from(previous_pixel[2].abs_diff(next_pixel[2])));
			comparisons = comparisons.saturating_add(3);
		}
	}

	if comparisons == 0 {
		return u32::MAX;
	}

	((total_abs_diff.saturating_mul(100)) / comparisons) as u32
}

fn overlap_global_informative_span(left: &RgbaImage, right: &RgbaImage) -> Option<InformativeSpan> {
	let left_span = informative_column_span(left, 0, left.height());
	let right_span = informative_column_span(right, 0, right.height());
	let width = left.width().min(right.width());

	match (left_span, right_span) {
		(Some(left_span), Some(right_span)) => {
			let start_x = left_span.start_x.max(right_span.start_x);
			let end_exclusive_x =
				left_span.end_exclusive_x.min(right_span.end_exclusive_x).min(width);

			(end_exclusive_x > start_x).then_some(InformativeSpan { start_x, end_exclusive_x })
		},
		(Some(span), None) | (None, Some(span)) => {
			let end_exclusive_x = span.end_exclusive_x.min(width).max(span.start_x + 1);

			(end_exclusive_x > span.start_x)
				.then_some(InformativeSpan { start_x: span.start_x, end_exclusive_x })
		},
		(None, None) => None,
	}
}
