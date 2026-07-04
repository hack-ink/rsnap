use color_eyre::eyre::{self, Result, WrapErr};
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

use crate::capture_frame::{
	self, CaptureFrameBackgroundKind, CaptureFrameBackgroundPlan, CaptureFrameRenderImageRef,
	CaptureFrameRenderKind, CaptureFrameShadow, CaptureFrameSourceKind, model,
};
use crate::{DisplayPointRect, RgbaExportImage};

/// Renders the complete capture-frame effect into a new RGBA export image.
///
/// Platform hosts provide raw capture pixels and, when available, a pre-decoded wallpaper
/// thumbnail. Rust owns the durable product behavior: canvas geometry, background selection,
/// wallpaper aspect-fill, overlay, shadows, rounded clipping, and final RGBA composition.
pub fn render_capture_frame_effect(
	source: CaptureFrameRenderImageRef<'_>,
	background_kind: CaptureFrameBackgroundKind,
	screen_scale_factor: f64,
	source_kind: CaptureFrameSourceKind,
	render_kind: CaptureFrameRenderKind,
	wallpaper: Option<CaptureFrameRenderImageRef<'_>>,
) -> Result<Option<RgbaExportImage>> {
	let Some(plan) = capture_frame::capture_frame_plan(
		source.width(),
		source.height(),
		screen_scale_factor,
		source_kind,
	) else {
		return Ok(None);
	};
	let canvas_width = finite_canvas_dimension(plan.canvas_width)?;
	let canvas_height = finite_canvas_dimension(plan.canvas_height)?;
	let canvas_len = model::expected_rgba_len(canvas_width, canvas_height)?;
	let mut canvas = vec![0_u8; canvas_len];

	draw_capture_frame_background(
		&mut canvas,
		canvas_width,
		canvas_height,
		background_kind,
		wallpaper,
	)?;

	match render_kind {
		CaptureFrameRenderKind::FramedCapture => {
			for shadow in plan.shadows {
				draw_soft_rounded_shadow(
					&mut canvas,
					canvas_width,
					canvas_height,
					plan.image_rect,
					plan.corner_radius,
					shadow,
				);
			}

			draw_capture_source(
				&mut canvas,
				canvas_width,
				canvas_height,
				source,
				plan.image_rect,
				Some(plan.corner_radius),
			)?;
		},
		CaptureFrameRenderKind::WindowSnapshot => {
			draw_capture_source(
				&mut canvas,
				canvas_width,
				canvas_height,
				source,
				plan.image_rect,
				None,
			)?;
		},
	}

	RgbaExportImage::from_raw(canvas_width, canvas_height, canvas).map(Some)
}

fn finite_canvas_dimension(value: f64) -> Result<u32> {
	if !value.is_finite() || value <= 0.0 || value > f64::from(u32::MAX) {
		return Err(eyre::eyre!("capture-frame canvas dimension is invalid: {value}"));
	}

	Ok(value.ceil() as u32)
}

fn draw_capture_frame_background(
	canvas: &mut [u8],
	canvas_width: u32,
	canvas_height: u32,
	background_kind: CaptureFrameBackgroundKind,
	wallpaper: Option<CaptureFrameRenderImageRef<'_>>,
) -> Result<()> {
	let background = capture_frame::capture_frame_background_plan(background_kind);

	if background.prefers_wallpaper
		&& let Some(wallpaper) = wallpaper
	{
		draw_wallpaper_background(
			canvas,
			canvas_width,
			canvas_height,
			wallpaper,
			background.wallpaper_overlay_alpha,
		)?;

		return Ok(());
	}

	draw_gradient_background(canvas, canvas_width, canvas_height, background);

	Ok(())
}

fn draw_gradient_background(
	canvas: &mut [u8],
	canvas_width: u32,
	canvas_height: u32,
	background: CaptureFrameBackgroundPlan,
) {
	let width = usize::try_from(canvas_width).unwrap_or(0);
	let height = usize::try_from(canvas_height).unwrap_or(0);

	if width == 0 || height == 0 {
		return;
	}

	let dx = f64::from(canvas_width);
	let dy = -f64::from(canvas_height);
	let length_squared = (dx * dx + dy * dy).max(f64::MIN_POSITIVE);

	for y in 0..height {
		for x in 0..width {
			let px = x as f64 + 0.5;
			let py = y as f64 + 0.5;
			let projection = (px * dx + (py - f64::from(canvas_height)) * dy) / length_squared;
			let color = gradient_color_at(background, projection.clamp(0.0, 1.0));
			let index = (y * width + x) * 4;

			canvas[index] = color[0];
			canvas[index + 1] = color[1];
			canvas[index + 2] = color[2];
			canvas[index + 3] = color[3];
		}
	}
}

fn gradient_color_at(background: CaptureFrameBackgroundPlan, location: f64) -> [u8; 4] {
	let segment = if location <= background.locations[1] { 0 } else { 1 };
	let start_location = background.locations[segment];
	let end_location = background.locations[segment + 1];
	let span = (end_location - start_location).max(f64::MIN_POSITIVE);
	let t = ((location - start_location) / span).clamp(0.0, 1.0);
	let start = background.colors[segment];
	let end = background.colors[segment + 1];

	[
		unit_to_u8(lerp(start.red, end.red, t)),
		unit_to_u8(lerp(start.green, end.green, t)),
		unit_to_u8(lerp(start.blue, end.blue, t)),
		unit_to_u8(lerp(start.alpha, end.alpha, t)),
	]
}

fn draw_wallpaper_background(
	canvas: &mut [u8],
	canvas_width: u32,
	canvas_height: u32,
	wallpaper: CaptureFrameRenderImageRef<'_>,
	overlay_alpha: f64,
) -> Result<()> {
	let crop = capture_frame::capture_frame_aspect_fill_crop_rect(
		wallpaper.width(),
		wallpaper.height(),
		f64::from(canvas_width),
		f64::from(canvas_height),
	)
	.ok_or_else(|| eyre::eyre!("capture-frame wallpaper crop is invalid"))?;
	let (crop_x, crop_y, crop_width, crop_height) =
		integral_crop_rect(crop, wallpaper.width(), wallpaper.height())
			.ok_or_else(|| eyre::eyre!("capture-frame wallpaper crop is empty"))?;
	let cropped = crop_rgba_to_vec(wallpaper, crop_x, crop_y, crop_width, crop_height)?;
	let fitted = resize_rgba_exact(crop_width, crop_height, &cropped, canvas_width, canvas_height)
		.wrap_err("failed to resize capture-frame wallpaper background")?;

	canvas.copy_from_slice(&fitted);

	apply_black_overlay(canvas, overlay_alpha);

	Ok(())
}

fn apply_black_overlay(canvas: &mut [u8], alpha: f64) {
	let alpha = alpha.clamp(0.0, 1.0) as f32;

	if alpha <= 0.0 {
		return;
	}

	for pixel in canvas.chunks_exact_mut(4) {
		blend_black_alpha(pixel, alpha);
	}
}

fn draw_capture_source(
	canvas: &mut [u8],
	canvas_width: u32,
	canvas_height: u32,
	source: CaptureFrameRenderImageRef<'_>,
	destination: DisplayPointRect,
	clip_radius: Option<f64>,
) -> Result<()> {
	let Some((destination_x, destination_y, destination_width, destination_height)) =
		destination_pixel_rect(destination, canvas_width, canvas_height)
	else {
		return Ok(());
	};
	let resized_source;
	let source_rgba =
		if source.width() == destination_width && source.height() == destination_height {
			source.rgba()
		} else {
			resized_source = resize_rgba_exact(
				source.width(),
				source.height(),
				source.rgba(),
				destination_width,
				destination_height,
			)
			.wrap_err("failed to resize capture source into capture frame")?;

			&resized_source
		};
	let canvas_width_usize =
		usize::try_from(canvas_width).wrap_err("failed to convert canvas width")?;
	let destination_width_usize =
		usize::try_from(destination_width).wrap_err("failed to convert destination width")?;
	let destination_height_usize =
		usize::try_from(destination_height).wrap_err("failed to convert destination height")?;
	let destination_x_usize =
		usize::try_from(destination_x).wrap_err("failed to convert destination x")?;
	let destination_y_usize =
		usize::try_from(destination_y).wrap_err("failed to convert destination y")?;

	for y in 0..destination_height_usize {
		for x in 0..destination_width_usize {
			let source_index = (y * destination_width_usize + x) * 4;
			let canvas_x = destination_x_usize + x;
			let canvas_y = destination_y_usize + y;
			let canvas_index = (canvas_y * canvas_width_usize + canvas_x) * 4;
			let coverage = clip_radius.map_or(1.0, |radius| {
				rounded_rect_coverage(
					canvas_x as f64 + 0.5,
					canvas_y as f64 + 0.5,
					destination,
					radius,
				)
			});

			if coverage > 0.0 {
				let source_alpha = (f32::from(source_rgba[source_index + 3]) / 255.0) * coverage;

				blend_rgba_pixel(
					&mut canvas[canvas_index..canvas_index + 4],
					&source_rgba[source_index..source_index + 4],
					source_alpha,
				);
			}
		}
	}

	Ok(())
}

fn draw_soft_rounded_shadow(
	canvas: &mut [u8],
	canvas_width: u32,
	canvas_height: u32,
	image_rect: DisplayPointRect,
	corner_radius: f64,
	shadow: CaptureFrameShadow,
) {
	if !shadow.blur.is_finite()
		|| !shadow.alpha.is_finite()
		|| shadow.blur <= 0.0
		|| shadow.alpha <= 0.0
	{
		return;
	}

	let shadow_rect = DisplayPointRect::new(
		image_rect.x + shadow.offset_x,
		image_rect.y + shadow.offset_y,
		image_rect.width,
		image_rect.height,
	);
	let blur = shadow.blur.max(1.0);
	let influence = blur * 2.0 + 2.0;
	let min_x = ((shadow_rect.x - influence).floor().max(0.0)) as u32;
	let min_y = ((shadow_rect.y - influence).floor().max(0.0)) as u32;
	let max_x = ((shadow_rect.x + shadow_rect.width + influence)
		.ceil()
		.min(f64::from(canvas_width))) as u32;
	let max_y = ((shadow_rect.y + shadow_rect.height + influence)
		.ceil()
		.min(f64::from(canvas_height))) as u32;
	let canvas_width_usize = canvas_width as usize;

	for y in min_y..max_y {
		for x in min_x..max_x {
			let distance = rounded_rect_signed_distance(
				f64::from(x) + 0.5,
				f64::from(y) + 0.5,
				shadow_rect,
				corner_radius,
			)
			.max(0.0);
			let softness = (1.0 - distance / (blur * 1.6)).clamp(0.0, 1.0);

			if softness <= 0.0 {
				continue;
			}

			let eased = softness * softness * (3.0 - 2.0 * softness);
			let alpha = (shadow.alpha * eased).clamp(0.0, 1.0) as f32;
			let index = ((y as usize) * canvas_width_usize + (x as usize)) * 4;

			blend_black_alpha(&mut canvas[index..index + 4], alpha);
		}
	}
}

fn destination_pixel_rect(
	rect: DisplayPointRect,
	canvas_width: u32,
	canvas_height: u32,
) -> Option<(u32, u32, u32, u32)> {
	if !rect.x.is_finite()
		|| !rect.y.is_finite()
		|| !rect.width.is_finite()
		|| !rect.height.is_finite()
		|| rect.width <= 0.0
		|| rect.height <= 0.0
	{
		return None;
	}

	let x = rect.x.round().max(0.0).min(f64::from(canvas_width)) as u32;
	let y = rect.y.round().max(0.0).min(f64::from(canvas_height)) as u32;
	let width = rect.width.round().max(1.0) as u32;
	let height = rect.height.round().max(1.0) as u32;
	let width = width.min(canvas_width.checked_sub(x)?);
	let height = height.min(canvas_height.checked_sub(y)?);

	(width > 0 && height > 0).then_some((x, y, width, height))
}

fn integral_crop_rect(
	rect: DisplayPointRect,
	image_width: u32,
	image_height: u32,
) -> Option<(u32, u32, u32, u32)> {
	if !rect.x.is_finite()
		|| !rect.y.is_finite()
		|| !rect.width.is_finite()
		|| !rect.height.is_finite()
		|| rect.width <= 0.0
		|| rect.height <= 0.0
	{
		return None;
	}

	let x = rect.x.floor().max(0.0).min(f64::from(image_width)) as u32;
	let y = rect.y.floor().max(0.0).min(f64::from(image_height)) as u32;
	let right = (rect.x + rect.width).ceil().max(0.0).min(f64::from(image_width)) as u32;
	let bottom = (rect.y + rect.height).ceil().max(0.0).min(f64::from(image_height)) as u32;
	let width = right.checked_sub(x)?;
	let height = bottom.checked_sub(y)?;

	(width > 0 && height > 0).then_some((x, y, width, height))
}

fn crop_rgba_to_vec(
	image: CaptureFrameRenderImageRef<'_>,
	x: u32,
	y: u32,
	width: u32,
	height: u32,
) -> Result<Vec<u8>> {
	let source_width = usize::try_from(image.width()).wrap_err("failed to convert source width")?;
	let x = usize::try_from(x).wrap_err("failed to convert crop x")?;
	let y = usize::try_from(y).wrap_err("failed to convert crop y")?;
	let width = usize::try_from(width).wrap_err("failed to convert crop width")?;
	let height = usize::try_from(height).wrap_err("failed to convert crop height")?;
	let mut cropped = vec![0_u8; width * height * 4];

	for row in 0..height {
		let source_start = ((y + row) * source_width + x) * 4;
		let source_end = source_start + width * 4;
		let destination_start = row * width * 4;

		cropped[destination_start..destination_start + width * 4]
			.copy_from_slice(&image.rgba()[source_start..source_end]);
	}

	Ok(cropped)
}

fn resize_rgba_exact(
	source_width: u32,
	source_height: u32,
	source_rgba: &[u8],
	destination_width: u32,
	destination_height: u32,
) -> Result<Vec<u8>> {
	let expected = model::expected_rgba_len(source_width, source_height)?;

	if source_rgba.len() != expected {
		return Err(eyre::eyre!(
			"capture-frame resize byte length mismatch: expected {expected}, got {}",
			source_rgba.len()
		));
	}
	if source_width == destination_width && source_height == destination_height {
		return Ok(source_rgba.to_vec());
	}

	let source_ref = ImageRef::new(source_width, source_height, source_rgba, PixelType::U8x4)
		.wrap_err("failed to prepare capture-frame source image")?;
	let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));
	let mut destination_image = Image::new(destination_width, destination_height, PixelType::U8x4);
	let mut resizer = Resizer::new();

	resizer
		.resize(&source_ref, &mut destination_image, &options)
		.wrap_err("failed to Lanczos-resize capture-frame image")?;

	Ok(destination_image.into_vec())
}

fn rounded_rect_coverage(px: f64, py: f64, rect: DisplayPointRect, radius: f64) -> f32 {
	(0.5 - rounded_rect_signed_distance(px, py, rect, radius)).clamp(0.0, 1.0) as f32
}

fn rounded_rect_signed_distance(px: f64, py: f64, rect: DisplayPointRect, radius: f64) -> f64 {
	let half_width = rect.width * 0.5;
	let half_height = rect.height * 0.5;
	let radius = radius.max(0.0).min(half_width).min(half_height);
	let center_x = rect.x + half_width;
	let center_y = rect.y + half_height;
	let qx = (px - center_x).abs() - (half_width - radius);
	let qy = (py - center_y).abs() - (half_height - radius);
	let outside_x = qx.max(0.0);
	let outside_y = qy.max(0.0);
	let outside = (outside_x * outside_x + outside_y * outside_y).sqrt();
	let inside = qx.max(qy).min(0.0);

	outside + inside - radius
}

fn blend_rgba_pixel(destination: &mut [u8], source: &[u8], alpha: f32) {
	let alpha = alpha.clamp(0.0, 1.0);

	if alpha <= 0.0 {
		return;
	}

	let inverse = 1.0 - alpha;

	destination[0] = blend_channel(source[0], destination[0], alpha, inverse);
	destination[1] = blend_channel(source[1], destination[1], alpha, inverse);
	destination[2] = blend_channel(source[2], destination[2], alpha, inverse);
	destination[3] = 255;
}

fn blend_black_alpha(destination: &mut [u8], alpha: f32) {
	let inverse = 1.0 - alpha.clamp(0.0, 1.0);

	destination[0] = (f32::from(destination[0]) * inverse).round().clamp(0.0, 255.0) as u8;
	destination[1] = (f32::from(destination[1]) * inverse).round().clamp(0.0, 255.0) as u8;
	destination[2] = (f32::from(destination[2]) * inverse).round().clamp(0.0, 255.0) as u8;
	destination[3] = 255;
}

fn blend_channel(source: u8, destination: u8, alpha: f32, inverse: f32) -> u8 {
	(f32::from(source) * alpha + f32::from(destination) * inverse).round().clamp(0.0, 255.0) as u8
}

fn lerp(start: f64, end: f64, t: f64) -> f64 {
	start + (end - start) * t
}

fn unit_to_u8(value: f64) -> u8 {
	(value.clamp(0.0, 1.0) * 255.0).round() as u8
}
