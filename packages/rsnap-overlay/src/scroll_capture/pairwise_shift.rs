#[cfg(target_os = "macos")]
use std::ptr;

#[cfg(target_os = "macos")]
use color_eyre::eyre::{self, Result};
use image::RgbaImage;
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

use crate::scroll_capture::{
	DIRECTION_WARNING_MARGIN_X100, DirectionMatch, DownwardRegistration, OverlapSearchConfig,
	ScrollDirection, support,
};

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
