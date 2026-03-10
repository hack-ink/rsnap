use std::borrow::Cow;

use image::{RgbaImage, imageops::FilterType};

use crate::overlay::SCROLL_CAPTURE_PREVIEW_WIDTH_PX;
use crate::state::{GlobalPoint, MonitorRect, Rgb};

pub(super) fn resize_scroll_preview_segment(segment: &RgbaImage) -> RgbaImage {
	if segment.width() <= SCROLL_CAPTURE_PREVIEW_WIDTH_PX {
		return segment.clone();
	}

	let preview_height = ((segment.height() as f32 / segment.width() as f32)
		* SCROLL_CAPTURE_PREVIEW_WIDTH_PX as f32)
		.round()
		.max(1.0) as u32;

	image::imageops::resize(
		segment,
		SCROLL_CAPTURE_PREVIEW_WIDTH_PX,
		preview_height,
		FilterType::Triangle,
	)
}

pub(super) fn frozen_rgb(
	image: &Option<RgbaImage>,
	monitor: Option<MonitorRect>,
	point: GlobalPoint,
) -> Option<Rgb> {
	let Some(image) = image else {
		return None;
	};
	let monitor = monitor?;
	let (x, y) = monitor.local_u32_pixels(point)?;
	let pixel = image.get_pixel_checked(x, y)?;

	Some(Rgb::new(pixel.0[0], pixel.0[1], pixel.0[2]))
}

pub(super) fn frozen_loupe_patch(
	image: &Option<RgbaImage>,
	monitor: Option<MonitorRect>,
	point: GlobalPoint,
	width_px: u32,
	height_px: u32,
) -> Option<RgbaImage> {
	let Some(image) = image else {
		return None;
	};
	let monitor = monitor?;
	let (center_x, center_y) = monitor.local_u32_pixels(point)?;
	let mut out = RgbaImage::new(width_px.max(1), height_px.max(1));
	let out_width = out.width() as i32;
	let out_height = out.height() as i32;
	let half_width = out_width / 2;
	let half_height = out_height / 2;
	let center_x = center_x as i32;
	let center_y = center_y as i32;
	let image_width = image.width() as i32;
	let image_height = image.height() as i32;

	for out_y in 0..out.height() {
		for out_x in 0..out.width() {
			let image_x = center_x + (out_x as i32) - half_width;
			let image_y = center_y + (out_y as i32) - half_height;
			let color = if image_x >= 0
				&& image_y >= 0
				&& image_x < image_width
				&& image_y < image_height
			{
				*image.get_pixel(image_x as u32, image_y as u32)
			} else {
				image::Rgba([0, 0, 0, 0])
			};

			out.put_pixel(out_x, out_y, color);
		}
	}

	Some(out)
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

	Cow::Owned(image::imageops::resize(
		image,
		width.min(max_side),
		height.min(max_side),
		FilterType::Triangle,
	))
}
