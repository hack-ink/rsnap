use std::borrow::Cow;

use image::{
	RgbaImage,
	imageops::{self, FilterType},
};

use crate::overlay::scroll_preview_geometry::SCROLL_CAPTURE_PREVIEW_WIDTH_PX;

pub(super) fn resize_scroll_preview_segment(segment: &RgbaImage) -> RgbaImage {
	if segment.width() <= SCROLL_CAPTURE_PREVIEW_WIDTH_PX {
		return segment.clone();
	}

	let preview_height = ((segment.height() as f32 / segment.width() as f32)
		* SCROLL_CAPTURE_PREVIEW_WIDTH_PX as f32)
		.round()
		.max(1.0) as u32;

	imageops::resize(segment, SCROLL_CAPTURE_PREVIEW_WIDTH_PX, preview_height, FilterType::Triangle)
}

pub(super) fn pad_rows(
	src: &[u8],
	src_row_bytes: usize,
	dst_row_bytes: usize,
	rows: usize,
) -> Vec<u8> {
	debug_assert!(dst_row_bytes >= src_row_bytes);

	let mut out = vec![0_u8; dst_row_bytes * rows];

	for y in 0..rows {
		let src_i = y * src_row_bytes;
		let dst_i = y * dst_row_bytes;

		out[dst_i..dst_i + src_row_bytes].copy_from_slice(&src[src_i..src_i + src_row_bytes]);
	}

	out
}

pub(super) fn downscale_for_gpu_upload(image: &RgbaImage, max_side: u32) -> Cow<'_, RgbaImage> {
	if image.width() <= max_side && image.height() <= max_side {
		return Cow::Borrowed(image);
	}

	let longest_side = image.width().max(image.height()) as f32;
	let scale = (max_side as f32) / longest_side;
	let width = ((image.width() as f32) * scale).round().max(1.0) as u32;
	let height = ((image.height() as f32) * scale).round().max(1.0) as u32;

	Cow::Owned(imageops::resize(
		image,
		width.min(max_side),
		height.min(max_side),
		FilterType::Triangle,
	))
}
