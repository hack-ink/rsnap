use image::{Rgba, RgbaImage};

use crate::overlay::{OverlaySession, RectPoints};

impl OverlaySession {
	pub(super) fn auto_center_frozen_capture_rect(&mut self) -> bool {
		let Some((monitor, capture_rect)) = self.frozen_capture_rect_drag_target() else {
			return false;
		};
		let mut next_rect = capture_rect;

		for _ in 0..Self::AUTO_CENTER_MAX_ITERATIONS {
			let Some(capture_image) =
				self.cropped_frozen_capture_image_for_rect(monitor, next_rect)
			else {
				break;
			};
			let Some(content_bounds) = Self::detect_auto_center_content_bounds(&capture_image)
			else {
				break;
			};
			let delta_x_points = Self::auto_center_margin_balance_shift_points(
				content_bounds.x,
				content_bounds.width,
				capture_image.width(),
				next_rect.width,
			);
			let delta_y_points = Self::auto_center_margin_balance_shift_points(
				content_bounds.y,
				content_bounds.height,
				capture_image.height(),
				next_rect.height,
			);

			if delta_x_points == 0 && delta_y_points == 0 {
				break;
			}

			let candidate_rect = Self::clamp_frozen_capture_rect_to_monitor(
				monitor,
				next_rect.width,
				next_rect.height,
				i64::from(next_rect.x) + delta_x_points,
				i64::from(next_rect.y) + delta_y_points,
			);

			if candidate_rect == next_rect {
				break;
			}

			next_rect = candidate_rect;
		}

		if next_rect == capture_rect {
			return false;
		}

		self.apply_frozen_capture_rect_update(monitor, next_rect)
	}

	const AUTO_CENTER_MAX_ITERATIONS: usize = 6;

	fn auto_center_margin_balance_shift_points(
		content_origin_px: u32,
		content_size_px: u32,
		crop_size_px: u32,
		capture_size_points: u32,
	) -> i64 {
		if crop_size_px == 0 || capture_size_points == 0 {
			return 0;
		}

		let leading_margin_px = content_origin_px as f32;
		let trailing_margin_px =
			crop_size_px.saturating_sub(content_origin_px.saturating_add(content_size_px)) as f32;
		let delta_px = (leading_margin_px - trailing_margin_px) * 0.5;

		((delta_px * capture_size_points as f32) / crop_size_px as f32).round() as i64
	}

	fn detect_auto_center_content_bounds(image: &RgbaImage) -> Option<RectPoints> {
		let width = image.width();
		let height = image.height();

		if width < 2 || height < 2 {
			return None;
		}

		let edge_strip = Self::auto_center_edge_strip_extent(width.min(height));
		let top_mean = Self::region_rgb_mean(image, 0, width, 0, edge_strip)?;
		let bottom_mean =
			Self::region_rgb_mean(image, 0, width, height.saturating_sub(edge_strip), height)?;
		let left_mean = Self::region_rgb_mean(image, 0, edge_strip, 0, height)?;
		let right_mean =
			Self::region_rgb_mean(image, width.saturating_sub(edge_strip), width, 0, height)?;
		let threshold = {
			let edge_noise = [
				Self::region_rgb_mean_distance(image, 0, width, 0, edge_strip, top_mean),
				Self::region_rgb_mean_distance(
					image,
					0,
					width,
					height.saturating_sub(edge_strip),
					height,
					bottom_mean,
				),
				Self::region_rgb_mean_distance(image, 0, edge_strip, 0, height, left_mean),
				Self::region_rgb_mean_distance(
					image,
					width.saturating_sub(edge_strip),
					width,
					0,
					height,
					right_mean,
				),
			]
			.into_iter()
			.fold(0.0, f32::max);

			(edge_noise * 3.0).round().clamp(24.0, 96.0) as u32
		};
		let min_salient_per_row = (width / 64).max(1) as usize;
		let min_salient_per_column = (height / 64).max(1) as usize;
		let mut row_counts = vec![0_usize; height as usize];
		let mut column_counts = vec![0_usize; width as usize];

		for (x, y, pixel) in image.enumerate_pixels() {
			let salient_distance = [
				Self::rgb_distance_to_mean(pixel, top_mean),
				Self::rgb_distance_to_mean(pixel, bottom_mean),
				Self::rgb_distance_to_mean(pixel, left_mean),
				Self::rgb_distance_to_mean(pixel, right_mean),
			]
			.into_iter()
			.min()
			.unwrap_or(0);

			if salient_distance < threshold {
				continue;
			}

			row_counts[y as usize] += 1;
			column_counts[x as usize] += 1;
		}

		let top = row_counts.iter().position(|count| *count >= min_salient_per_row)?;
		let bottom = row_counts.iter().rposition(|count| *count >= min_salient_per_row)?;
		let left = column_counts.iter().position(|count| *count >= min_salient_per_column)?;
		let right = column_counts.iter().rposition(|count| *count >= min_salient_per_column)?;

		if left > right || top > bottom {
			return None;
		}

		let bounds = RectPoints::new(
			left as u32,
			top as u32,
			(right - left + 1) as u32,
			(bottom - top + 1) as u32,
		);
		let fills_crop_width = bounds.width.saturating_mul(100) >= width.saturating_mul(92);
		let fills_crop_height = bounds.height.saturating_mul(100) >= height.saturating_mul(92);

		if fills_crop_width && fills_crop_height {
			return None;
		}

		Some(bounds)
	}

	fn auto_center_edge_strip_extent(length: u32) -> u32 {
		((length as f32) * 0.08).round().clamp(1.0, 24.0) as u32
	}

	fn region_rgb_mean(image: &RgbaImage, x0: u32, x1: u32, y0: u32, y1: u32) -> Option<[f32; 3]> {
		if x0 >= x1 || y0 >= y1 {
			return None;
		}

		let mut r_total = 0_u64;
		let mut g_total = 0_u64;
		let mut b_total = 0_u64;
		let mut sample_count = 0_u64;

		for y in y0..y1 {
			for x in x0..x1 {
				let pixel = image.get_pixel(x, y);

				r_total += u64::from(pixel[0]);
				g_total += u64::from(pixel[1]);
				b_total += u64::from(pixel[2]);
				sample_count += 1;
			}
		}

		if sample_count == 0 {
			return None;
		}

		Some([
			r_total as f32 / sample_count as f32,
			g_total as f32 / sample_count as f32,
			b_total as f32 / sample_count as f32,
		])
	}

	fn region_rgb_mean_distance(
		image: &RgbaImage,
		x0: u32,
		x1: u32,
		y0: u32,
		y1: u32,
		mean: [f32; 3],
	) -> f32 {
		if x0 >= x1 || y0 >= y1 {
			return 0.0;
		}

		let mut total_distance = 0_u64;
		let mut sample_count = 0_u64;

		for y in y0..y1 {
			for x in x0..x1 {
				total_distance +=
					u64::from(Self::rgb_distance_to_mean(image.get_pixel(x, y), mean));
				sample_count += 1;
			}
		}

		if sample_count == 0 { 0.0 } else { total_distance as f32 / sample_count as f32 }
	}

	fn rgb_distance_to_mean(pixel: &Rgba<u8>, mean: [f32; 3]) -> u32 {
		(pixel[0] as f32 - mean[0]).abs().round() as u32
			+ (pixel[1] as f32 - mean[1]).abs().round() as u32
			+ (pixel[2] as f32 - mean[2]).abs().round() as u32
	}
}
