use std::collections::VecDeque;
use std::ops::Deref;
use std::slice;
use std::time::Instant;

use image::{Rgba, RgbaImage};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{
	CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
	CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
	CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVReturnSuccess,
};

use crate::macos_color;
use crate::state::{LiveCursorSample, RectPoints, Rgb};

pub(crate) struct OrderedRegionFrame {
	pub(crate) frame_seq: u64,
	pub(crate) captured_at: Instant,
	pub(crate) image: RgbaImage,
}

#[derive(Clone)]
pub(super) struct SharedPixelBuffer(pub(super) CFRetained<CVPixelBuffer>);
// Safety: CoreVideo pixel buffers are retained CF objects. This wrapper only exposes
// immutable queries plus read-only base-address locks, so sharing retained references
// across threads does not permit unsynchronized mutation from Rust.
unsafe impl Send for SharedPixelBuffer {}

impl Deref for SharedPixelBuffer {
	type Target = CFRetained<CVPixelBuffer>;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

unsafe impl Sync for SharedPixelBuffer {}

#[derive(Clone)]
pub(super) struct QueuedPixelBufferFrame {
	pub(super) frame_seq: u64,
	pub(super) stream_generation: u64,
	pub(super) captured_at: Instant,
	pub(super) pixel_buffer: SharedPixelBuffer,
}

#[derive(Clone)]
pub(super) struct SharedQueuedPixelBufferFrames {
	pub(super) monitor_id: u32,
	pub(super) frames: VecDeque<QueuedPixelBufferFrame>,
}

pub(super) fn pixel_buffer_size_px(pixel_buffer: &CFRetained<CVPixelBuffer>) -> Option<(u32, u32)> {
	let width = CVPixelBufferGetWidth(pixel_buffer);
	let height = CVPixelBufferGetHeight(pixel_buffer);
	let width = u32::try_from(width).ok()?;
	let height = u32::try_from(height).ok()?;

	Some((width, height))
}

pub(super) fn sample_cursor_from_pixel_buffer(
	pixel_buffer: &CFRetained<CVPixelBuffer>,
	x_px: u32,
	y_px: u32,
	want_patch: bool,
	patch_width_px: u32,
	patch_height_px: u32,
) -> Option<LiveCursorSample> {
	let (width, height) = pixel_buffer_size_px(pixel_buffer)?;
	let lock_result =
		unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	if lock_result != kCVReturnSuccess {
		return None;
	}

	let out = (|| {
		let base = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;

		if base.is_null() {
			return None;
		}

		let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
		let byte_len = (height as usize).saturating_mul(bytes_per_row);
		let bytes = unsafe { slice::from_raw_parts(base, byte_len) };

		sample_cursor_from_bgra_bytes(
			bytes,
			bytes_per_row,
			width,
			height,
			x_px,
			y_px,
			want_patch,
			patch_width_px,
			patch_height_px,
		)
	})();
	let _ =
		unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	out
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sample_cursor_from_bgra_bytes(
	bytes: &[u8],
	bytes_per_row: usize,
	width_px: u32,
	height_px: u32,
	x_px: u32,
	y_px: u32,
	want_patch: bool,
	patch_width_px: u32,
	patch_height_px: u32,
) -> Option<LiveCursorSample> {
	if x_px >= width_px || y_px >= height_px {
		return None;
	}

	let offset = (y_px as usize).saturating_mul(bytes_per_row).saturating_add((x_px as usize) * 4);
	let b = *bytes.get(offset)?;
	let g = *bytes.get(offset + 1)?;
	let r = *bytes.get(offset + 2)?;
	let _a = *bytes.get(offset + 3)?;
	let rgb = Some(Rgb::new(r, g, b));
	let patch = if want_patch {
		let out_patch_w = patch_width_px.max(1);
		let out_patch_h = patch_height_px.max(1);
		let half_w = (out_patch_w as i32) / 2;
		let half_h = (out_patch_h as i32) / 2;
		let center_x = x_px as i32;
		let center_y = y_px as i32;
		let in_w = width_px as i32;
		let in_h = height_px as i32;
		let mut out_patch = RgbaImage::new(out_patch_w, out_patch_h);

		for oy in 0..(out_patch_h as i32) {
			let iy = (center_y - half_h + oy).clamp(0, in_h.saturating_sub(1));

			for ox in 0..(out_patch_w as i32) {
				let ix = (center_x - half_w + ox).clamp(0, in_w.saturating_sub(1));
				let offset =
					(iy as usize).saturating_mul(bytes_per_row).saturating_add((ix as usize) * 4);
				let b = *bytes.get(offset)?;
				let g = *bytes.get(offset + 1)?;
				let r = *bytes.get(offset + 2)?;
				let a = *bytes.get(offset + 3)?;

				out_patch.put_pixel(ox as u32, oy as u32, Rgba([r, g, b, a]));
			}
		}

		Some(out_patch)
	} else {
		None
	};

	Some(LiveCursorSample { rgb, patch })
}

pub(super) fn rgba_image_from_pixel_buffer(
	pixel_buffer: &CFRetained<CVPixelBuffer>,
	width_px: u32,
	height_px: u32,
	display_id: u32,
) -> Option<RgbaImage> {
	if let Some(image) = macos_color::rgba_image_from_pixel_buffer_color_managed(
		pixel_buffer,
		width_px,
		height_px,
		Some(display_id),
	) {
		return Some(image);
	}

	let lock_result =
		unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	if lock_result != kCVReturnSuccess {
		return None;
	}

	let out = (|| {
		let base = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;

		if base.is_null() {
			return None;
		}

		let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
		let mut out = RgbaImage::new(width_px.max(1), height_px.max(1));
		let out_w = out.width() as usize;
		let out_h = out.height() as usize;

		for y in 0..out_h {
			let row = unsafe { slice::from_raw_parts(base.add(y * bytes_per_row), bytes_per_row) };

			for x in 0..out_w {
				let idx = x * 4;
				let b = row.get(idx).copied().unwrap_or(0);
				let g = row.get(idx + 1).copied().unwrap_or(0);
				let r = row.get(idx + 2).copied().unwrap_or(0);
				let a = row.get(idx + 3).copied().unwrap_or(255);

				out.put_pixel(x as u32, y as u32, Rgba([r, g, b, a]));
			}
		}

		Some(out)
	})();
	let _ =
		unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	out
}

pub(super) fn rgba_region_from_pixel_buffer(
	pixel_buffer: &CFRetained<CVPixelBuffer>,
	rect_px: RectPoints,
) -> Option<RgbaImage> {
	let (buffer_width_px, buffer_height_px) = pixel_buffer_size_px(pixel_buffer)?;
	let width_px = rect_px.width.max(1).min(buffer_width_px.max(1));
	let height_px = rect_px.height.max(1).min(buffer_height_px.max(1));
	let x_px = rect_px.x.min(buffer_width_px.saturating_sub(width_px));
	let y_px = rect_px.y.min(buffer_height_px.saturating_sub(height_px));
	let lock_result =
		unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	if lock_result != kCVReturnSuccess {
		return None;
	}

	let out = (|| {
		let base = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;

		if base.is_null() {
			return None;
		}

		let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
		let mut out = RgbaImage::new(width_px.max(1), height_px.max(1));
		let out_w = out.width() as usize;
		let out_h = out.height() as usize;
		let src_x = x_px as usize;
		let src_y = y_px as usize;

		for y in 0..out_h {
			let row_offset = (src_y + y).saturating_mul(bytes_per_row);
			let row = unsafe { slice::from_raw_parts(base.add(row_offset), bytes_per_row) };

			for x in 0..out_w {
				let idx = (src_x + x).saturating_mul(4);
				let b = row.get(idx).copied().unwrap_or(0);
				let g = row.get(idx + 1).copied().unwrap_or(0);
				let r = row.get(idx + 2).copied().unwrap_or(0);
				let a = row.get(idx + 3).copied().unwrap_or(255);

				out.put_pixel(x as u32, y as u32, Rgba([r, g, b, a]));
			}
		}

		Some(out)
	})();
	let _ =
		unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	out
}

pub(super) fn ordered_rgba_regions_from_frames(
	frames: Vec<QueuedPixelBufferFrame>,
	rect_px: RectPoints,
) -> Vec<OrderedRegionFrame> {
	frames
		.into_iter()
		.filter_map(|frame| {
			let image = rgba_region_from_pixel_buffer(&frame.pixel_buffer, rect_px)?;

			Some(OrderedRegionFrame {
				frame_seq: frame.frame_seq,
				captured_at: frame.captured_at,
				image,
			})
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use crate::live_frame_stream_macos::live_frame_buffer;
	use crate::state::Rgb;

	#[test]
	fn sample_cursor_from_bgra_bytes_reads_rgb_without_patch() {
		let sample = live_frame_buffer::sample_cursor_from_bgra_bytes(
			&[
				1, 2, 3, 255, 11, 12, 13, 254, //
				21, 22, 23, 253, 31, 32, 33, 252,
			],
			8,
			2,
			2,
			1,
			0,
			false,
			0,
			0,
		)
		.expect("sample should exist inside bounds");

		assert_eq!(sample.rgb, Some(Rgb::new(13, 12, 11)));
		assert!(sample.patch.is_none());
	}

	#[test]
	fn sample_cursor_from_bgra_bytes_clamps_patch_edges() {
		let sample = live_frame_buffer::sample_cursor_from_bgra_bytes(
			&[
				1, 2, 3, 255, 11, 12, 13, 254, //
				21, 22, 23, 253, 31, 32, 33, 252,
			],
			8,
			2,
			2,
			0,
			0,
			true,
			3,
			3,
		)
		.expect("sample should exist inside bounds");
		let patch = sample.patch.expect("patch should be present");

		assert_eq!(patch.dimensions(), (3, 3));
		assert_eq!(patch.get_pixel(0, 0).0, [3, 2, 1, 255]);
		assert_eq!(patch.get_pixel(1, 0).0, [3, 2, 1, 255]);
		assert_eq!(patch.get_pixel(2, 2).0, [33, 32, 31, 252]);
	}
}
