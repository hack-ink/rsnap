//! Frozen mosaic privacy-patch rendering owned by the Rust product core.

use image::{Rgba, RgbaImage};

use crate::export::DisplayPointRect;

const FROZEN_MOSAIC_BLOCK_SIZE_PIXELS: f64 = 10.0;

/// Builds the light privacy mosaic patch used by native frozen-overlay rendering.
#[must_use]
pub fn frozen_mosaic_light_privacy_patch(
	image_width: u32,
	image_height: u32,
	source_rect: DisplayPointRect,
) -> Option<RgbaImage> {
	let crop_rect = integral_image_intersection(source_rect, image_width, image_height)?;
	let patch_width = mosaic_patch_axis(crop_rect.width)?;
	let patch_height = mosaic_patch_axis(crop_rect.height)?;
	let seed_x = (f64::from(crop_rect.x) / FROZEN_MOSAIC_BLOCK_SIZE_PIXELS).floor() as u32;
	let seed_y = (f64::from(crop_rect.y) / FROZEN_MOSAIC_BLOCK_SIZE_PIXELS).floor() as u32;

	Some(RgbaImage::from_fn(patch_width, patch_height, |x, y| {
		frozen_mosaic_light_privacy_color(
			x.saturating_add(seed_x),
			y.saturating_add(seed_y),
			patch_width,
			patch_height,
		)
	}))
}

fn integral_image_intersection(
	rect: DisplayPointRect,
	image_width: u32,
	image_height: u32,
) -> Option<crate::RectPoints> {
	if image_width == 0
		|| image_height == 0
		|| !rect.x.is_finite()
		|| !rect.y.is_finite()
		|| !rect.width.is_finite()
		|| !rect.height.is_finite()
		|| rect.width <= 0.0
		|| rect.height <= 0.0
	{
		return None;
	}

	let right = rect.x + rect.width;
	let bottom = rect.y + rect.height;

	if !right.is_finite() || !bottom.is_finite() {
		return None;
	}

	let clipped_left = rect.x.floor().max(0.0);
	let clipped_top = rect.y.floor().max(0.0);
	let clipped_right = right.ceil().min(f64::from(image_width));
	let clipped_bottom = bottom.ceil().min(f64::from(image_height));

	if clipped_left >= clipped_right || clipped_top >= clipped_bottom {
		return None;
	}

	let x = integral_f64_to_u32(clipped_left)?;
	let y = integral_f64_to_u32(clipped_top)?;
	let max_x = integral_f64_to_u32(clipped_right)?;
	let max_y = integral_f64_to_u32(clipped_bottom)?;

	Some(crate::RectPoints::new(x, y, max_x.checked_sub(x)?, max_y.checked_sub(y)?))
}

fn integral_f64_to_u32(value: f64) -> Option<u32> {
	if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) {
		return None;
	}

	Some(value as u32)
}

fn mosaic_patch_axis(crop_axis: u32) -> Option<u32> {
	if crop_axis == 0 {
		return None;
	}

	Some(((f64::from(crop_axis) / FROZEN_MOSAIC_BLOCK_SIZE_PIXELS).ceil() as u32).max(1))
}

fn frozen_mosaic_light_privacy_color(x: u32, y: u32, width: u32, height: u32) -> Rgba<u8> {
	let hash = frozen_mosaic_hash(x, y, width, height);
	let group_hash = frozen_mosaic_hash(x / 2, y / 2, width, height);
	let base = 0.74 + f64::from(group_hash & 3) * 0.035;
	let variation = (f64::from((hash >> 8) & 3) - 1.5) * 0.012;
	let warmth = f64::from((group_hash >> 3) & 1) * 0.012;

	Rgba([
		frozen_mosaic_byte(base + variation + warmth),
		frozen_mosaic_byte(base + variation + warmth * 0.5),
		frozen_mosaic_byte(base + variation),
		255,
	])
}

fn frozen_mosaic_hash(x: u32, y: u32, width: u32, height: u32) -> u32 {
	let mut hash = x.wrapping_mul(0x045d_9f3b)
		^ y.wrapping_mul(0x119d_e1f3)
		^ width.wrapping_mul(0x27d4_eb2d)
		^ height.wrapping_mul(0x1656_67b1);

	hash ^= hash >> 16;
	hash = hash.wrapping_mul(0x7feb_352d);
	hash ^= hash >> 15;
	hash = hash.wrapping_mul(0x846c_a68b);
	hash ^= hash >> 16;

	hash
}

fn frozen_mosaic_byte(value: f64) -> u8 {
	(value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
	use crate::mosaic::{self, DisplayPointRect};

	#[test]
	fn mosaic_light_privacy_patch_matches_native_dimensions_and_seeded_colors() {
		let patch = mosaic::frozen_mosaic_light_privacy_patch(
			100,
			80,
			DisplayPointRect::new(4.2, 9.1, 28.4, 21.0),
		)
		.expect("valid patch");

		assert_eq!(patch.dimensions(), (3, 3));
		assert_eq!(patch.get_pixel(0, 0).0, [211, 211, 211, 255]);
		assert_eq!(patch.get_pixel(1, 0).0, [205, 205, 205, 255]);
		assert_eq!(patch.get_pixel(2, 0).0, [202, 201, 199, 255]);
		assert_eq!(patch.get_pixel(0, 2).0, [220, 220, 220, 255]);
	}

	#[test]
	fn mosaic_light_privacy_patch_clips_to_image_bounds() {
		let patch = mosaic::frozen_mosaic_light_privacy_patch(
			32,
			24,
			DisplayPointRect::new(25.5, 18.2, 20.0, 20.0),
		)
		.expect("clipped patch");

		assert_eq!(patch.dimensions(), (1, 1));
		assert_eq!(patch.get_pixel(0, 0).0[3], 255);
	}

	#[test]
	fn mosaic_light_privacy_patch_rejects_empty_or_outside_rects() {
		assert!(
			mosaic::frozen_mosaic_light_privacy_patch(
				100,
				80,
				DisplayPointRect::new(10.0, 10.0, 0.0, 20.0)
			)
			.is_none()
		);
		assert!(
			mosaic::frozen_mosaic_light_privacy_patch(
				100,
				80,
				DisplayPointRect::new(120.0, 10.0, 10.0, 20.0)
			)
			.is_none()
		);
	}
}
