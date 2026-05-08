//! Frozen auto-center content detection owned by the Rust product core.

use crate::RectPoints;

/// Error returned when an auto-center image payload is invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutoCenterImageError {
	/// The image dimensions are too small or overflow platform sizes.
	InvalidDimensions,
	/// The RGBA payload does not contain exactly `width * height * 4` bytes.
	InvalidRgbaLength,
}

/// Detects the salient content bounds used by frozen auto-center.
pub fn detect_auto_center_content_bounds_rgba(
	width: u32,
	height: u32,
	rgba: &[u8],
) -> Result<Option<RectPoints>, AutoCenterImageError> {
	let width = usize::try_from(width).map_err(|_error| AutoCenterImageError::InvalidDimensions)?;
	let height =
		usize::try_from(height).map_err(|_error| AutoCenterImageError::InvalidDimensions)?;
	if width < 2 || height < 2 {
		return Ok(None);
	}

	let expected_len = width
		.checked_mul(height)
		.and_then(|pixels| pixels.checked_mul(4))
		.ok_or(AutoCenterImageError::InvalidDimensions)?;
	if rgba.len() != expected_len {
		return Err(AutoCenterImageError::InvalidRgbaLength);
	}

	let edge_strip = edge_strip_pixels(width, height);
	let edge_means = EdgeMeans {
		top: region_rgb_mean(rgba, width, 0, width, 0, edge_strip),
		bottom: region_rgb_mean(rgba, width, 0, width, height - edge_strip, height),
		left: region_rgb_mean(rgba, width, 0, edge_strip, 0, height),
		right: region_rgb_mean(rgba, width, width - edge_strip, width, 0, height),
	};
	let threshold = salient_threshold(rgba, width, height, edge_strip, edge_means);
	let min_salient_per_row = 1_usize.max(width / 64);
	let min_salient_per_column = 1_usize.max(height / 64);

	let Some(bounds) = salient_bounds(
		rgba,
		width,
		height,
		edge_means,
		threshold,
		min_salient_per_row,
		min_salient_per_column,
	) else {
		return Ok(None);
	};

	let fills_crop_width = bounds.width as usize * 100 >= width * 92;
	let fills_crop_height = bounds.height as usize * 100 >= height * 92;
	if fills_crop_width && fills_crop_height {
		return Ok(None);
	}

	Ok(Some(bounds))
}

/// Resolves the point shift that balances content margins inside a frozen crop.
#[must_use]
pub fn auto_center_margin_balance_shift_points(
	content_origin_px: f64,
	content_size_px: f64,
	crop_size_px: f64,
	capture_size_points: f64,
) -> f64 {
	if crop_size_px <= 0.0 || capture_size_points <= 0.0 {
		return 0.0;
	}

	let leading_margin_px = content_origin_px;
	let trailing_margin_px = crop_size_px - (content_origin_px + content_size_px);
	let delta_px = (leading_margin_px - trailing_margin_px) * 0.5;

	(delta_px * capture_size_points / crop_size_px).round()
}

#[derive(Clone, Copy)]
struct EdgeMeans {
	top: [f64; 3],
	bottom: [f64; 3],
	left: [f64; 3],
	right: [f64; 3],
}

fn edge_strip_pixels(width: usize, height: usize) -> usize {
	let short_side = width.min(height) as f64;

	1_usize.max(24_usize.min((short_side * 0.08).round() as usize))
}

fn salient_threshold(
	rgba: &[u8],
	width: usize,
	height: usize,
	edge_strip: usize,
	means: EdgeMeans,
) -> f64 {
	region_rgb_mean_distance(rgba, width, 0, width, 0, edge_strip, means.top)
		.max(region_rgb_mean_distance(
			rgba,
			width,
			0,
			width,
			height - edge_strip,
			height,
			means.bottom,
		))
		.max(region_rgb_mean_distance(rgba, width, 0, edge_strip, 0, height, means.left))
		.max(region_rgb_mean_distance(
			rgba,
			width,
			width - edge_strip,
			width,
			0,
			height,
			means.right,
		))
		.mul_add(3.0, 0.0)
		.round()
		.clamp(24.0, 96.0)
}

fn salient_bounds(
	rgba: &[u8],
	width: usize,
	height: usize,
	means: EdgeMeans,
	threshold: f64,
	min_salient_per_row: usize,
	min_salient_per_column: usize,
) -> Option<RectPoints> {
	let mut row_counts = vec![0_usize; height];
	let mut column_counts = vec![0_usize; width];

	for (y, row_count) in row_counts.iter_mut().enumerate() {
		for (x, column_count) in column_counts.iter_mut().enumerate() {
			let rgb = rgb_at(rgba, width, x, y);
			let salient_distance = rgb_distance_to_mean(rgb, means.top)
				.min(rgb_distance_to_mean(rgb, means.bottom))
				.min(rgb_distance_to_mean(rgb, means.left))
				.min(rgb_distance_to_mean(rgb, means.right));
			if salient_distance < threshold {
				continue;
			}
			*row_count += 1;
			*column_count += 1;
		}
	}

	let top = row_counts.iter().position(|count| *count >= min_salient_per_row)?;
	let bottom = row_counts.iter().rposition(|count| *count >= min_salient_per_row)?;
	let left = column_counts.iter().position(|count| *count >= min_salient_per_column)?;
	let right = column_counts.iter().rposition(|count| *count >= min_salient_per_column)?;
	if left > right || top > bottom {
		return None;
	}

	Some(RectPoints::new(
		left as u32,
		top as u32,
		(right - left + 1) as u32,
		(bottom - top + 1) as u32,
	))
}

fn region_rgb_mean(
	rgba: &[u8],
	width: usize,
	x0: usize,
	x1: usize,
	y0: usize,
	y1: usize,
) -> [f64; 3] {
	let mut r_total = 0.0;
	let mut g_total = 0.0;
	let mut b_total = 0.0;
	let mut count = 0.0;

	for y in y0..y1 {
		for x in x0..x1 {
			let rgb = rgb_at(rgba, width, x, y);
			r_total += rgb[0];
			g_total += rgb[1];
			b_total += rgb[2];
			count += 1.0;
		}
	}

	[r_total / count, g_total / count, b_total / count]
}

fn region_rgb_mean_distance(
	rgba: &[u8],
	width: usize,
	x0: usize,
	x1: usize,
	y0: usize,
	y1: usize,
	mean: [f64; 3],
) -> f64 {
	let mut total = 0.0;
	let mut count = 0.0;
	for y in y0..y1 {
		for x in x0..x1 {
			total += rgb_distance_to_mean(rgb_at(rgba, width, x, y), mean);
			count += 1.0;
		}
	}

	if count == 0.0 { 0.0 } else { total / count }
}

fn rgb_at(rgba: &[u8], width: usize, x: usize, y: usize) -> [f64; 3] {
	let offset = (y * width + x) * 4;

	[f64::from(rgba[offset]), f64::from(rgba[offset + 1]), f64::from(rgba[offset + 2])]
}

fn rgb_distance_to_mean(rgb: [f64; 3], mean: [f64; 3]) -> f64 {
	(rgb[0] - mean[0]).abs().round()
		+ (rgb[1] - mean[1]).abs().round()
		+ (rgb[2] - mean[2]).abs().round()
}

#[cfg(test)]
mod tests {
	use super::{
		AutoCenterImageError, auto_center_margin_balance_shift_points,
		detect_auto_center_content_bounds_rgba,
	};
	use crate::RectPoints;

	#[test]
	fn detects_centered_content_bounds_from_rgba() {
		let rgba = auto_center_fixture(100, 80, Some(RectPoints::new(30, 20, 24, 18)));
		let bounds =
			detect_auto_center_content_bounds_rgba(100, 80, &rgba).expect("valid RGBA fixture");

		assert_eq!(bounds, Some(RectPoints::new(30, 20, 24, 18)));
	}

	#[test]
	fn returns_empty_for_uniform_or_full_frame_content() {
		let uniform = auto_center_fixture(100, 80, None);
		let full = auto_center_fixture(100, 80, Some(RectPoints::new(2, 2, 96, 76)));

		assert_eq!(
			detect_auto_center_content_bounds_rgba(100, 80, &uniform)
				.expect("valid uniform fixture"),
			None
		);
		assert_eq!(
			detect_auto_center_content_bounds_rgba(100, 80, &full).expect("valid full fixture"),
			None
		);
	}

	#[test]
	fn rejects_invalid_rgba_length() {
		assert!(matches!(
			detect_auto_center_content_bounds_rgba(100, 80, &[0, 1, 2, 3]),
			Err(AutoCenterImageError::InvalidRgbaLength)
		));
	}

	#[test]
	fn margin_balance_shift_matches_native_math() {
		assert_eq!(auto_center_margin_balance_shift_points(30.0, 24.0, 100.0, 50.0), -4.0);
		assert_eq!(auto_center_margin_balance_shift_points(0.0, 24.0, 100.0, 50.0), -19.0);
		assert_eq!(auto_center_margin_balance_shift_points(30.0, 24.0, 0.0, 50.0), 0.0);
	}

	fn auto_center_fixture(width: u32, height: u32, content: Option<RectPoints>) -> Vec<u8> {
		let mut rgba = vec![180_u8; (width * height * 4) as usize];
		for pixel in rgba.chunks_exact_mut(4) {
			pixel[3] = 255;
		}

		if let Some(content) = content {
			for y in content.y..content.y + content.height {
				for x in content.x..content.x + content.width {
					let offset = ((y * width + x) * 4) as usize;
					rgba[offset] = 24;
					rgba[offset + 1] = 32;
					rgba[offset + 2] = 40;
				}
			}
		}

		rgba
	}
}
