use image::{Rgba, RgbaImage};

use crate::point::PixelPoint;

#[derive(Clone, Copy, Debug)]
struct PixelRect {
	x: u32,
	y: u32,
	width: u32,
	height: u32,
}

pub(super) fn draw_polyline(
	image: &mut RgbaImage,
	points: &[PixelPoint],
	line_width: f32,
	color: Rgba<u8>,
) {
	if points.is_empty() || line_width <= f32::EPSILON {
		return;
	}
	if points.len() == 1 {
		draw_segments(image, &[(points[0], points[0])], line_width, color);

		return;
	}

	let segments = points.windows(2).map(|points| (points[0], points[1])).collect::<Vec<_>>();

	draw_segments(image, &segments, line_width, color);
}

pub(super) fn draw_segments(
	image: &mut RgbaImage,
	segments: &[(PixelPoint, PixelPoint)],
	line_width: f32,
	color: Rgba<u8>,
) {
	if segments.is_empty() || image.width() == 0 || image.height() == 0 {
		return;
	}

	let radius = (line_width * 0.5).max(0.5);
	let Some(mask_rect) = segments_pixel_bounds(segments, image.width(), image.height(), radius)
	else {
		return;
	};
	let mut coverage_mask = vec![0_u8; mask_rect.width as usize * mask_rect.height as usize];

	for (start, end) in segments {
		rasterize_segment(
			&mut coverage_mask,
			mask_rect,
			image.width(),
			image.height(),
			*start,
			*end,
			radius,
		);
	}

	blend_coverage_mask(image, mask_rect, &coverage_mask, color);
}

fn rasterize_segment(
	coverage_mask: &mut [u8],
	mask_rect: PixelRect,
	width: u32,
	height: u32,
	start: PixelPoint,
	end: PixelPoint,
	radius: f32,
) {
	let delta_x = end.x - start.x;
	let delta_y = end.y - start.y;
	let delta_len_sq = delta_x.mul_add(delta_x, delta_y * delta_y);

	if delta_len_sq <= f32::EPSILON {
		rasterize_circle(coverage_mask, mask_rect, width, height, start, radius);

		return;
	}

	let Some(bounds) = segment_pixel_bounds(start, end, width, height, radius)
		.and_then(|bounds| intersect_pixel_rect(bounds, mask_rect))
	else {
		return;
	};

	for y in bounds.y..bounds.y + bounds.height {
		for x in bounds.x..bounds.x + bounds.width {
			let sample = PixelPoint::new(x as f32 + 0.5, y as f32 + 0.5);
			let projection = (((sample.x - start.x) * delta_x + (sample.y - start.y) * delta_y)
				/ delta_len_sq)
				.clamp(0.0, 1.0);
			let nearest =
				PixelPoint::new(start.x + delta_x * projection, start.y + delta_y * projection);

			update_coverage_mask(
				coverage_mask,
				mask_rect,
				x,
				y,
				stroke_coverage(sample.distance(nearest), radius),
			);
		}
	}
}

fn rasterize_circle(
	coverage_mask: &mut [u8],
	mask_rect: PixelRect,
	width: u32,
	height: u32,
	center: PixelPoint,
	radius: f32,
) {
	let Some(bounds) = circle_pixel_bounds(center, width, height, radius)
		.and_then(|bounds| intersect_pixel_rect(bounds, mask_rect))
	else {
		return;
	};

	for y in bounds.y..bounds.y + bounds.height {
		for x in bounds.x..bounds.x + bounds.width {
			let sample = PixelPoint::new(x as f32 + 0.5, y as f32 + 0.5);

			update_coverage_mask(
				coverage_mask,
				mask_rect,
				x,
				y,
				stroke_coverage(sample.distance(center), radius),
			);
		}
	}
}

fn segments_pixel_bounds(
	segments: &[(PixelPoint, PixelPoint)],
	width: u32,
	height: u32,
	radius: f32,
) -> Option<PixelRect> {
	let mut bounds = None;

	for (start, end) in segments {
		let Some(segment_bounds) = segment_pixel_bounds(*start, *end, width, height, radius) else {
			continue;
		};

		bounds = Some(match bounds {
			Some(bounds) => union_pixel_rect(bounds, segment_bounds),
			None => segment_bounds,
		});
	}

	bounds
}

fn segment_pixel_bounds(
	start: PixelPoint,
	end: PixelPoint,
	width: u32,
	height: u32,
	radius: f32,
) -> Option<PixelRect> {
	pixel_bounds(
		start.x.min(end.x) - radius - 0.5,
		start.y.min(end.y) - radius - 0.5,
		start.x.max(end.x) + radius + 0.5,
		start.y.max(end.y) + radius + 0.5,
		width,
		height,
	)
}

fn circle_pixel_bounds(
	center: PixelPoint,
	width: u32,
	height: u32,
	radius: f32,
) -> Option<PixelRect> {
	pixel_bounds(
		center.x - radius - 0.5,
		center.y - radius - 0.5,
		center.x + radius + 0.5,
		center.y + radius + 0.5,
		width,
		height,
	)
}

fn pixel_bounds(
	min_x: f32,
	min_y: f32,
	max_x: f32,
	max_y: f32,
	width: u32,
	height: u32,
) -> Option<PixelRect> {
	if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
		return None;
	}

	let left = min_x.floor().max(0.0) as u32;
	let top = min_y.floor().max(0.0) as u32;
	let right = (max_x.ceil() + 1.0).clamp(0.0, width as f32) as u32;
	let bottom = (max_y.ceil() + 1.0).clamp(0.0, height as f32) as u32;

	if left >= right || top >= bottom {
		return None;
	}

	Some(PixelRect { x: left, y: top, width: right - left, height: bottom - top })
}

fn intersect_pixel_rect(first: PixelRect, second: PixelRect) -> Option<PixelRect> {
	let left = first.x.max(second.x);
	let top = first.y.max(second.y);
	let right = (first.x + first.width).min(second.x + second.width);
	let bottom = (first.y + first.height).min(second.y + second.height);

	if left >= right || top >= bottom {
		return None;
	}

	Some(PixelRect { x: left, y: top, width: right - left, height: bottom - top })
}

fn union_pixel_rect(first: PixelRect, second: PixelRect) -> PixelRect {
	let left = first.x.min(second.x);
	let top = first.y.min(second.y);
	let right = (first.x + first.width).max(second.x + second.width);
	let bottom = (first.y + first.height).max(second.y + second.height);

	PixelRect { x: left, y: top, width: right - left, height: bottom - top }
}

fn stroke_coverage(distance: f32, radius: f32) -> u8 {
	((radius + 0.5 - distance).clamp(0.0, 1.0) * 255.0).round() as u8
}

fn update_coverage_mask(
	coverage_mask: &mut [u8],
	mask_rect: PixelRect,
	x: u32,
	y: u32,
	coverage: u8,
) {
	if coverage == 0 {
		return;
	}

	let index = (y - mask_rect.y) as usize * mask_rect.width as usize + (x - mask_rect.x) as usize;

	coverage_mask[index] = coverage_mask[index].max(coverage);
}

fn blend_coverage_mask(
	image: &mut RgbaImage,
	mask_rect: PixelRect,
	coverage_mask: &[u8],
	color: Rgba<u8>,
) {
	let source_alpha = f32::from(color[3]) / 255.0;
	let image_width = image.width() as usize;
	let image_bytes = image.as_mut();
	let mask_width = mask_rect.width as usize;
	let left = mask_rect.x as usize;
	let top = mask_rect.y as usize;

	for local_y in 0..mask_rect.height as usize {
		let mask_row_start = local_y * mask_width;
		let image_row_start = ((top + local_y) * image_width + left) * 4;

		for local_x in 0..mask_width {
			let mask_alpha = coverage_mask[mask_row_start + local_x];

			if mask_alpha == 0 {
				continue;
			}

			let pixel_start = image_row_start + local_x * 4;
			let pixel_end = pixel_start + 4;

			blend_pixel_channels(
				&mut image_bytes[pixel_start..pixel_end],
				color,
				f32::from(mask_alpha) / 255.0 * source_alpha,
			);
		}
	}
}

fn blend_pixel_channels(pixel: &mut [u8], color: Rgba<u8>, src_a: f32) {
	let dst_a = f32::from(pixel[3]) / 255.0;
	let out_a = src_a + dst_a * (1.0 - src_a);

	if out_a <= f32::EPSILON {
		return;
	}

	for channel in 0..3 {
		let src = f32::from(color[channel]) / 255.0;
		let dst = f32::from(pixel[channel]) / 255.0;
		let out = (src * src_a + dst * dst_a * (1.0 - src_a)) / out_a;

		pixel[channel] = (out * 255.0).round().clamp(0.0, 255.0) as u8;
	}

	pixel[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

#[cfg(test)]
mod tests {
	use image::{Rgba, RgbaImage};

	use crate::frozen_overlay_export::stroke_raster;
	use crate::point::PixelPoint;

	#[test]
	fn draw_polyline_renders_single_point_strokes() {
		let mut image = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));

		stroke_raster::draw_polyline(
			&mut image,
			&[PixelPoint::new(4.0, 4.0)],
			2.0,
			Rgba([255, 0, 0, 255]),
		);

		assert!(image.pixels().any(|pixel| pixel[0] > 0));
	}

	#[test]
	fn draw_segments_skips_non_finite_bounds() {
		let mut image = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));

		stroke_raster::draw_segments(
			&mut image,
			&[(PixelPoint::new(f32::NAN, 4.0), PixelPoint::new(4.0, 4.0))],
			2.0,
			Rgba([255, 0, 0, 255]),
		);

		assert!(image.pixels().all(|pixel| *pixel == Rgba([0, 0, 0, 255])));
	}
}
