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

use crate::state::RectPoints;

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
