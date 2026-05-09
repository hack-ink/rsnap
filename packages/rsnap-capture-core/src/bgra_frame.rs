//! BGRA frame sampling primitives shared by native capture hosts.

use image::{Rgba, RgbaImage};

use crate::{DisplayPointRect, Rgb};

/// Borrowed BGRA8 frame storage with row-stride metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BgraFrameView<'a> {
	/// Frame width in pixels.
	pub width: u32,
	/// Frame height in pixels.
	pub height: u32,
	/// Source bytes per row, including any platform padding.
	pub bytes_per_row: usize,
	/// Packed BGRA8 source bytes.
	pub bytes: &'a [u8],
}
impl BgraFrameView<'_> {
	/// Returns true when the dimensions, stride, and byte storage can contain the frame.
	#[must_use]
	pub fn is_valid(self) -> bool {
		frame_is_valid(self)
	}
}

/// Samples an RGB value from a BGRA frame using display-space coordinates.
#[must_use]
pub fn sample_rgb_from_bgra_frame(
	frame: BgraFrameView<'_>,
	display_frame: DisplayPointRect,
	point_x: f64,
	point_y: f64,
) -> Option<Rgb> {
	let (x, y) = display_point_to_pixel(frame, display_frame, point_x, point_y)?;
	let offset = pixel_offset(frame, x, y)?;

	Some(Rgb::new(frame.bytes[offset + 2], frame.bytes[offset + 1], frame.bytes[offset]))
}

/// Builds a square RGBA loupe patch from a BGRA frame using display-space coordinates.
#[must_use]
pub fn loupe_patch_rgba_from_bgra_frame(
	frame: BgraFrameView<'_>,
	display_frame: DisplayPointRect,
	point_x: f64,
	point_y: f64,
	side_pixels: u32,
) -> Option<RgbaImage> {
	let (center_x, center_y) = display_point_to_pixel(frame, display_frame, point_x, point_y)?;
	let side = side_pixels.max(1);
	let half = i64::from(side / 2);
	let max_x = i64::from(frame.width.checked_sub(1)?);
	let max_y = i64::from(frame.height.checked_sub(1)?);

	Some(RgbaImage::from_fn(side, side, |output_x, output_y| {
		let source_x = clamp_i64(
			i64::try_from(center_x).unwrap_or(i64::MAX) - half + i64::from(output_x),
			0,
			max_x,
		);
		let source_y = clamp_i64(
			i64::try_from(center_y).unwrap_or(i64::MAX) - half + i64::from(output_y),
			0,
			max_y,
		);
		let offset = pixel_offset(frame, source_x as usize, source_y as usize)
			.expect("validated BGRA frame should contain every patch pixel");

		Rgba([
			frame.bytes[offset + 2],
			frame.bytes[offset + 1],
			frame.bytes[offset],
			frame.bytes[offset + 3],
		])
	}))
}

fn display_point_to_pixel(
	frame: BgraFrameView<'_>,
	display_frame: DisplayPointRect,
	point_x: f64,
	point_y: f64,
) -> Option<(usize, usize)> {
	if !frame_is_valid(frame) || !display_frame_is_valid(display_frame) {
		return None;
	}

	let display_max_x = display_frame.x + display_frame.width;
	let display_max_y = display_frame.y + display_frame.height;

	if !(point_x.is_finite()
		&& point_y.is_finite()
		&& point_x >= display_frame.x
		&& point_x < display_max_x
		&& point_y >= display_frame.y
		&& point_y < display_max_y)
	{
		return None;
	}

	let x_ratio = (point_x - display_frame.x) / display_frame.width;
	let y_ratio = (display_max_y - point_y) / display_frame.height;
	let x = ((x_ratio * f64::from(frame.width)).floor() as i64).clamp(0, i64::from(frame.width) - 1)
		as usize;
	let y = ((y_ratio * f64::from(frame.height)).floor() as i64)
		.clamp(0, i64::from(frame.height) - 1) as usize;

	Some((x, y))
}

fn frame_is_valid(frame: BgraFrameView<'_>) -> bool {
	if frame.width == 0 || frame.height == 0 {
		return false;
	}

	let width_bytes = usize::try_from(frame.width).ok().and_then(|width| width.checked_mul(4));
	let Some(width_bytes) = width_bytes else {
		return false;
	};

	if frame.bytes_per_row < width_bytes {
		return false;
	}

	let required_len = usize::try_from(frame.height)
		.ok()
		.and_then(|height| height.checked_mul(frame.bytes_per_row));
	let Some(required_len) = required_len else {
		return false;
	};

	frame.bytes.len() >= required_len
}

fn display_frame_is_valid(rect: DisplayPointRect) -> bool {
	rect.x.is_finite()
		&& rect.y.is_finite()
		&& rect.width.is_finite()
		&& rect.height.is_finite()
		&& rect.width > 0.0
		&& rect.height > 0.0
}

fn pixel_offset(frame: BgraFrameView<'_>, x: usize, y: usize) -> Option<usize> {
	if x >= usize::try_from(frame.width).ok()? || y >= usize::try_from(frame.height).ok()? {
		return None;
	}

	let row = y.checked_mul(frame.bytes_per_row)?;
	let column = x.checked_mul(4)?;
	let offset = row.checked_add(column)?;

	(offset + 3 < frame.bytes.len()).then_some(offset)
}

const fn clamp_i64(value: i64, min: i64, max: i64) -> i64 {
	if value < min {
		min
	} else if value > max {
		max
	} else {
		value
	}
}

#[cfg(test)]
mod tests {
	use crate::bgra_frame::{self, BgraFrameView};
	use crate::{DisplayPointRect, Rgb};

	#[test]
	fn samples_rgb_from_bgra_display_point() {
		let bytes = bgra_fixture(4, 3, 20);
		let rgb = bgra_frame::sample_rgb_from_bgra_frame(
			BgraFrameView { width: 4, height: 3, bytes_per_row: 20, bytes: &bytes },
			DisplayPointRect::new(10.0, 20.0, 40.0, 30.0),
			25.0,
			45.0,
		);

		assert_eq!(rgb, Some(Rgb::new(11, 21, 31)));
	}

	#[test]
	fn samples_bottom_edge_like_native_mapping() {
		let bytes = bgra_fixture(4, 3, 16);
		let rgb = bgra_frame::sample_rgb_from_bgra_frame(
			BgraFrameView { width: 4, height: 3, bytes_per_row: 16, bytes: &bytes },
			DisplayPointRect::new(0.0, 0.0, 4.0, 3.0),
			0.0,
			0.0,
		);

		assert_eq!(rgb, Some(Rgb::new(20, 40, 60)));
	}

	#[test]
	fn loupe_patch_clamps_edges_and_converts_bgra_to_rgba() {
		let bytes = bgra_fixture(4, 3, 16);
		let patch = bgra_frame::loupe_patch_rgba_from_bgra_frame(
			BgraFrameView { width: 4, height: 3, bytes_per_row: 16, bytes: &bytes },
			DisplayPointRect::new(0.0, 0.0, 4.0, 3.0),
			0.0,
			2.0,
			3,
		)
		.expect("valid patch");

		assert_eq!(patch.dimensions(), (3, 3));
		assert_eq!(patch.get_pixel(0, 0).0, [10, 20, 30, 200]);
		assert_eq!(patch.get_pixel(2, 2).0, [21, 41, 61, 203]);
	}

	#[test]
	fn rejects_invalid_frame_inputs() {
		let bytes = bgra_fixture(4, 3, 16);

		assert_eq!(
			bgra_frame::sample_rgb_from_bgra_frame(
				BgraFrameView { width: 4, height: 3, bytes_per_row: 12, bytes: &bytes },
				DisplayPointRect::new(0.0, 0.0, 4.0, 3.0),
				1.0,
				1.0,
			),
			None
		);
		assert_eq!(
			bgra_frame::loupe_patch_rgba_from_bgra_frame(
				BgraFrameView { width: 4, height: 3, bytes_per_row: 16, bytes: &bytes[..12] },
				DisplayPointRect::new(0.0, 0.0, 4.0, 3.0),
				1.0,
				1.0,
				3,
			),
			None
		);
	}

	fn bgra_fixture(width: u32, height: u32, bytes_per_row: usize) -> Vec<u8> {
		let mut bytes = vec![0xEE; bytes_per_row * height as usize];

		for y in 0..height {
			for x in 0..width {
				let offset = y as usize * bytes_per_row + x as usize * 4;

				bytes[offset] = 30 + y as u8 * 15 + x as u8;
				bytes[offset + 1] = 20 + y as u8 * 10 + x as u8;
				bytes[offset + 2] = 10 + y as u8 * 5 + x as u8;
				bytes[offset + 3] = 200 + y as u8 + x as u8;
			}
		}

		bytes
	}
}
