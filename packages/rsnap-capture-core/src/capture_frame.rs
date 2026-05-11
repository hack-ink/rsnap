//! Capture-frame layout and rendering owned by the Rust product core.

use color_eyre::eyre::{self, Result, WrapErr};
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

use crate::{DisplayPointRect, RgbaExportImage};

/// Product source kind used to tune capture-frame styling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureFrameSourceKind {
	/// User-dragged region capture.
	DragRegion,
	/// Single-window capture.
	Window,
	/// Full-screen capture.
	FullScreen,
	/// Scroll-capture export.
	ScrollCapture,
	/// Unknown or future capture source.
	Unknown,
}

/// Capture-frame background preset chosen by the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureFrameBackgroundKind {
	/// Prefer the current system wallpaper with a subtle dark overlay, falling back to Aurora.
	SystemWallpaper,
	/// Blue-to-warm product gradient.
	Aurora,
	/// Neutral graphite gradient.
	Graphite,
	/// Light linen gradient.
	Linen,
}

/// Capture-frame render mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureFrameRenderKind {
	/// Draw the capture as a framed object with shadows and rounded clipping.
	FramedCapture,
	/// Draw the capture as a floating window snapshot without additional clipping.
	WindowSnapshot,
}

/// Borrowed RGBA image consumed by the capture-frame renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureFrameRenderImageRef<'a> {
	width: u32,
	height: u32,
	rgba: &'a [u8],
}
impl<'a> CaptureFrameRenderImageRef<'a> {
	/// Creates a borrowed RGBA image after validating dimensions and byte count.
	pub fn new(width: u32, height: u32, rgba: &'a [u8]) -> Result<Self> {
		let expected = expected_rgba_len(width, height)?;

		if rgba.len() != expected {
			return Err(eyre::eyre!(
				"capture-frame RGBA byte length mismatch: expected {expected}, got {}",
				rgba.len()
			));
		}

		Ok(Self { width, height, rgba })
	}

	/// Borrows an owned product-core export image.
	#[must_use]
	pub fn from_export(image: &'a RgbaExportImage) -> Self {
		Self { width: image.width(), height: image.height(), rgba: image.as_raw() }
	}

	/// Returns image width in pixels.
	#[must_use]
	pub const fn width(self) -> u32 {
		self.width
	}

	/// Returns image height in pixels.
	#[must_use]
	pub const fn height(self) -> u32 {
		self.height
	}

	/// Returns raw row-major RGBA bytes.
	#[must_use]
	pub const fn rgba(self) -> &'a [u8] {
		self.rgba
	}
}

/// One sRGB capture-frame background color stop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureFrameColorStop {
	/// Red component in sRGB space.
	pub red: f64,
	/// Green component in sRGB space.
	pub green: f64,
	/// Blue component in sRGB space.
	pub blue: f64,
	/// Alpha component.
	pub alpha: f64,
}
impl CaptureFrameColorStop {
	/// Creates an sRGB color stop.
	#[must_use]
	pub const fn new(red: f64, green: f64, blue: f64, alpha: f64) -> Self {
		Self { red, green, blue, alpha }
	}
}

/// Capture-frame background plan consumed by native hosts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureFrameBackgroundPlan {
	/// Ordered sRGB gradient color stops.
	pub colors: [CaptureFrameColorStop; 3],
	/// Gradient locations matching `colors`.
	pub locations: [f64; 3],
	/// Whether the host should first try drawing the system wallpaper.
	pub prefers_wallpaper: bool,
	/// Overlay alpha applied when wallpaper drawing succeeds.
	pub wallpaper_overlay_alpha: f64,
}

/// Platform wallpaper thumbnail request planned by the Rust product core.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureFrameWallpaperRequest {
	/// Maximum thumbnail dimension requested from the platform image pipeline.
	pub target_pixel_size: u32,
	/// Overlay alpha applied after drawing the wallpaper thumbnail.
	pub overlay_alpha: f64,
}

/// One capture-frame shadow pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureFrameShadow {
	/// Horizontal shadow offset in output pixels.
	pub offset_x: f64,
	/// Vertical shadow offset in output pixels.
	pub offset_y: f64,
	/// Shadow blur radius in output pixels.
	pub blur: f64,
	/// Shadow alpha.
	pub alpha: f64,
}
impl CaptureFrameShadow {
	/// Creates a shadow pass.
	#[must_use]
	pub const fn new(offset_x: f64, offset_y: f64, blur: f64, alpha: f64) -> Self {
		Self { offset_x, offset_y, blur, alpha }
	}
}

/// Capture-frame plan consumed by native hosts for final drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureFramePlan {
	/// Canvas width in output pixels.
	pub canvas_width: f64,
	/// Canvas height in output pixels.
	pub canvas_height: f64,
	/// Image placement inside the canvas.
	pub image_rect: DisplayPointRect,
	/// Rounded capture corner radius.
	pub corner_radius: f64,
	/// Ordered shadow passes behind the framed capture.
	pub shadows: [CaptureFrameShadow; 3],
}

/// Resolves capture-frame layout, rounded-corner, and shadow parameters.
#[must_use]
pub fn capture_frame_plan(
	image_width: u32,
	image_height: u32,
	screen_scale_factor: f64,
	source: CaptureFrameSourceKind,
) -> Option<CaptureFramePlan> {
	if image_width == 0 || image_height == 0 {
		return None;
	}

	let image_width = f64::from(image_width);
	let image_height = f64::from(image_height);
	let padding = capture_frame_padding(image_width, image_height);
	let canvas_width = (image_width + padding * 2.0).ceil();
	let canvas_height = (image_height + padding * 2.0).ceil();
	let image_rect = DisplayPointRect::new(padding, padding, image_width, image_height);
	let corner_radius =
		capture_frame_corner_radius(image_width, image_height, screen_scale_factor, source);

	Some(CaptureFramePlan {
		canvas_width,
		canvas_height,
		image_rect,
		corner_radius,
		shadows: capture_frame_shadows(canvas_width, canvas_height, source),
	})
}

/// Resolves the source crop rect for aspect-fill drawing.
#[must_use]
pub fn capture_frame_aspect_fill_crop_rect(
	source_width: u32,
	source_height: u32,
	destination_width: f64,
	destination_height: f64,
) -> Option<DisplayPointRect> {
	if source_width == 0
		|| source_height == 0
		|| !destination_width.is_finite()
		|| !destination_height.is_finite()
		|| destination_width <= 0.0
		|| destination_height <= 0.0
	{
		return None;
	}

	let source_width = f64::from(source_width);
	let source_height = f64::from(source_height);
	let source_aspect = source_width / source_height.max(1.0);
	let destination_aspect = destination_width / destination_height.max(1.0);

	if source_aspect > destination_aspect {
		let width = source_height * destination_aspect;

		return Some(DisplayPointRect::new(
			(source_width - width) / 2.0,
			0.0,
			width,
			source_height,
		));
	}

	let height = source_width / destination_aspect.max(f64::MIN_POSITIVE);

	Some(DisplayPointRect::new(0.0, (source_height - height) / 2.0, source_width, height))
}

/// Resolves capture-frame background colors and wallpaper fallback behavior.
#[must_use]
pub fn capture_frame_background_plan(
	kind: CaptureFrameBackgroundKind,
) -> CaptureFrameBackgroundPlan {
	const LOCATIONS: [f64; 3] = [0.0, 0.54, 1.0];
	const AURORA: [CaptureFrameColorStop; 3] = [
		CaptureFrameColorStop::new(0.10, 0.16, 0.28, 1.0),
		CaptureFrameColorStop::new(0.30, 0.47, 0.71, 1.0),
		CaptureFrameColorStop::new(0.95, 0.61, 0.43, 1.0),
	];
	const GRAPHITE: [CaptureFrameColorStop; 3] = [
		CaptureFrameColorStop::new(0.08, 0.09, 0.11, 1.0),
		CaptureFrameColorStop::new(0.24, 0.26, 0.30, 1.0),
		CaptureFrameColorStop::new(0.56, 0.59, 0.64, 1.0),
	];
	const LINEN: [CaptureFrameColorStop; 3] = [
		CaptureFrameColorStop::new(0.83, 0.87, 0.82, 1.0),
		CaptureFrameColorStop::new(0.58, 0.70, 0.71, 1.0),
		CaptureFrameColorStop::new(0.24, 0.36, 0.47, 1.0),
	];

	match kind {
		CaptureFrameBackgroundKind::SystemWallpaper => CaptureFrameBackgroundPlan {
			colors: AURORA,
			locations: LOCATIONS,
			prefers_wallpaper: true,
			wallpaper_overlay_alpha: 0.10,
		},
		CaptureFrameBackgroundKind::Aurora => CaptureFrameBackgroundPlan {
			colors: AURORA,
			locations: LOCATIONS,
			prefers_wallpaper: false,
			wallpaper_overlay_alpha: 0.0,
		},
		CaptureFrameBackgroundKind::Graphite => CaptureFrameBackgroundPlan {
			colors: GRAPHITE,
			locations: LOCATIONS,
			prefers_wallpaper: false,
			wallpaper_overlay_alpha: 0.0,
		},
		CaptureFrameBackgroundKind::Linen => CaptureFrameBackgroundPlan {
			colors: LINEN,
			locations: LOCATIONS,
			prefers_wallpaper: false,
			wallpaper_overlay_alpha: 0.0,
		},
	}
}

/// Resolves whether a platform wallpaper thumbnail should be requested for a destination.
#[must_use]
pub fn capture_frame_wallpaper_request_plan(
	kind: CaptureFrameBackgroundKind,
	destination_width: f64,
	destination_height: f64,
) -> Option<CaptureFrameWallpaperRequest> {
	if !destination_width.is_finite()
		|| !destination_height.is_finite()
		|| destination_width <= 0.0
		|| destination_height <= 0.0
	{
		return None;
	}

	let background = capture_frame_background_plan(kind);

	if !background.prefers_wallpaper {
		return None;
	}

	let target_pixel_size =
		destination_width.max(destination_height).ceil().clamp(1.0, f64::from(u32::MAX)) as u32;

	Some(CaptureFrameWallpaperRequest {
		target_pixel_size,
		overlay_alpha: background.wallpaper_overlay_alpha,
	})
}

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
	let Some(plan) =
		capture_frame_plan(source.width(), source.height(), screen_scale_factor, source_kind)
	else {
		return Ok(None);
	};
	let canvas_width = finite_canvas_dimension(plan.canvas_width)?;
	let canvas_height = finite_canvas_dimension(plan.canvas_height)?;
	let canvas_len = expected_rgba_len(canvas_width, canvas_height)?;
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

fn capture_frame_padding(image_width: f64, image_height: f64) -> f64 {
	let short_side = image_width.min(image_height);
	let long_side = image_width.max(image_height);
	let visual_padding = short_side * 0.115;
	let maximum_padding = 72.0_f64.max(long_side * 0.18);

	visual_padding.clamp(48.0, maximum_padding)
}

fn capture_frame_corner_radius(
	image_width: f64,
	image_height: f64,
	screen_scale_factor: f64,
	source: CaptureFrameSourceKind,
) -> f64 {
	let short_side = image_width.min(image_height);

	match source {
		CaptureFrameSourceKind::Window => {
			let scale_factor = if screen_scale_factor.is_finite() && screen_scale_factor > 0.0 {
				screen_scale_factor
			} else {
				2.0
			};

			(20.0 * scale_factor).max(24.0).min(short_side * 0.055)
		},
		CaptureFrameSourceKind::DragRegion => 24.0_f64.min(8.0_f64.max(short_side * 0.025)),
		CaptureFrameSourceKind::FullScreen
		| CaptureFrameSourceKind::ScrollCapture
		| CaptureFrameSourceKind::Unknown => 28.0_f64.min(8.0_f64.max(short_side * 0.025)),
	}
}

fn capture_frame_shadows(
	canvas_width: f64,
	canvas_height: f64,
	source: CaptureFrameSourceKind,
) -> [CaptureFrameShadow; 3] {
	match source {
		CaptureFrameSourceKind::Window => window_capture_frame_shadows(canvas_width, canvas_height),
		CaptureFrameSourceKind::DragRegion
		| CaptureFrameSourceKind::FullScreen
		| CaptureFrameSourceKind::ScrollCapture
		| CaptureFrameSourceKind::Unknown => document_capture_frame_shadows(canvas_width, canvas_height),
	}
}

fn window_capture_frame_shadows(canvas_width: f64, canvas_height: f64) -> [CaptureFrameShadow; 3] {
	let short_side = canvas_width.min(canvas_height);

	[
		CaptureFrameShadow::new(0.0, 0.0, 80.0_f64.max(short_side * 0.085), 0.30),
		CaptureFrameShadow::new(
			0.0,
			-22.0_f64.max(canvas_height * 0.030),
			46.0_f64.max(short_side * 0.050),
			0.36,
		),
		CaptureFrameShadow::new(
			0.0,
			-4.0_f64.max(canvas_height * 0.006),
			10.0_f64.max(short_side * 0.014),
			0.22,
		),
	]
}

fn document_capture_frame_shadows(
	canvas_width: f64,
	canvas_height: f64,
) -> [CaptureFrameShadow; 3] {
	let short_side = canvas_width.min(canvas_height);

	[
		CaptureFrameShadow::new(
			0.0,
			(canvas_height * 0.008).clamp(4.0, 10.0),
			(short_side * 0.055).clamp(32.0, 72.0),
			0.16,
		),
		CaptureFrameShadow::new(
			0.0,
			(canvas_height * 0.026).clamp(18.0, 34.0),
			(short_side * 0.038).clamp(24.0, 50.0),
			0.18,
		),
		CaptureFrameShadow::new(
			0.0,
			(canvas_height * 0.006).clamp(4.0, 8.0),
			(short_side * 0.012).clamp(7.0, 13.0),
			0.10,
		),
	]
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
	let background = capture_frame_background_plan(background_kind);

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
	let crop = capture_frame_aspect_fill_crop_rect(
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
	let expected = expected_rgba_len(source_width, source_height)?;

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

fn expected_rgba_len(width: u32, height: u32) -> Result<usize> {
	if width == 0 || height == 0 {
		return Err(eyre::eyre!(
			"capture-frame RGBA dimensions must be non-zero: width={width}, height={height}"
		));
	}

	(width as usize)
		.checked_mul(height as usize)
		.and_then(|pixels| pixels.checked_mul(4))
		.ok_or_else(|| eyre::eyre!("capture-frame RGBA byte length overflow"))
}

#[cfg(test)]
mod tests {
	use crate::DisplayPointRect;
	use crate::capture_frame::{
		self, CaptureFrameBackgroundKind, CaptureFrameColorStop, CaptureFrameRenderImageRef,
		CaptureFrameRenderKind, CaptureFrameShadow, CaptureFrameSourceKind,
	};

	#[test]
	fn capture_frame_plan_matches_native_window_geometry() {
		let plan = capture_frame::capture_frame_plan(320, 180, 2.0, CaptureFrameSourceKind::Window)
			.expect("valid plan");

		assert_eq!(plan.canvas_width, 416.0);
		assert_eq!(plan.canvas_height, 276.0);
		assert_eq!(plan.image_rect, DisplayPointRect::new(48.0, 48.0, 320.0, 180.0));
		assert_eq!(plan.corner_radius, 9.9);
		assert_eq!(
			plan.shadows,
			[
				CaptureFrameShadow::new(0.0, 0.0, 80.0, 0.30),
				CaptureFrameShadow::new(0.0, -22.0, 46.0, 0.36),
				CaptureFrameShadow::new(0.0, -4.0, 10.0, 0.22),
			]
		);
	}

	#[test]
	fn capture_frame_plan_uses_document_shadow_for_drag_regions() {
		let plan =
			capture_frame::capture_frame_plan(1_440, 900, 2.0, CaptureFrameSourceKind::DragRegion)
				.expect("valid plan");

		assert_eq!(plan.canvas_width, 1_647.0);
		assert_eq!(plan.canvas_height, 1_107.0);
		assert_eq!(plan.image_rect, DisplayPointRect::new(103.5, 103.5, 1_440.0, 900.0));
		assert_eq!(plan.corner_radius, 22.5);

		assert_shadow_near(plan.shadows[0], CaptureFrameShadow::new(0.0, 8.856, 60.885, 0.16));
		assert_shadow_near(plan.shadows[1], CaptureFrameShadow::new(0.0, 28.782, 42.066, 0.18));
		assert_shadow_near(plan.shadows[2], CaptureFrameShadow::new(0.0, 6.642, 13.0, 0.10));
	}

	#[test]
	fn drag_region_frame_keeps_top_shadow_lighter_than_bottom_shadow() {
		let source_rgba = vec![255; 1_440 * 900 * 4];
		let wallpaper_rgba = [200, 200, 200, 255].repeat(64);
		let source = CaptureFrameRenderImageRef::new(1_440, 900, &source_rgba)
			.expect("source fixture should be valid");
		let wallpaper = CaptureFrameRenderImageRef::new(8, 8, &wallpaper_rgba)
			.expect("wallpaper fixture should be valid");
		let plan =
			capture_frame::capture_frame_plan(1_440, 900, 2.0, CaptureFrameSourceKind::DragRegion)
				.expect("valid plan");
		let rendered = capture_frame::render_capture_frame_effect(
			source,
			CaptureFrameBackgroundKind::SystemWallpaper,
			2.0,
			CaptureFrameSourceKind::DragRegion,
			CaptureFrameRenderKind::FramedCapture,
			Some(wallpaper),
		)
		.expect("render should succeed")
		.expect("render should produce an image");
		let center_x = (plan.image_rect.x + plan.image_rect.width * 0.5).round() as usize;
		let top_y = (plan.image_rect.y - 12.0).round() as usize;
		let bottom_y = (plan.image_rect.y + plan.image_rect.height + 12.0).round() as usize;
		let width = rendered.width() as usize;
		let top_red = rendered.as_raw()[(top_y * width + center_x) * 4];
		let bottom_red = rendered.as_raw()[(bottom_y * width + center_x) * 4];

		assert!(
			top_red > bottom_red,
			"drag-region frame should not render a darker top band than bottom lift shadow"
		);
	}

	#[test]
	fn capture_frame_plan_rejects_empty_input() {
		assert!(
			capture_frame::capture_frame_plan(0, 180, 2.0, CaptureFrameSourceKind::Window)
				.is_none()
		);
		assert!(
			capture_frame::capture_frame_plan(320, 0, 2.0, CaptureFrameSourceKind::Window)
				.is_none()
		);
	}

	#[test]
	fn capture_frame_aspect_fill_crop_matches_native_wide_source() {
		let rect = capture_frame::capture_frame_aspect_fill_crop_rect(1_600, 900, 1_000.0, 1_000.0)
			.expect("valid crop rect");

		assert_eq!(rect, DisplayPointRect::new(350.0, 0.0, 900.0, 900.0));
	}

	#[test]
	fn capture_frame_aspect_fill_crop_matches_native_tall_source() {
		let rect = capture_frame::capture_frame_aspect_fill_crop_rect(800, 1_200, 1_600.0, 900.0)
			.expect("valid crop rect");

		assert_eq!(rect, DisplayPointRect::new(0.0, 375.0, 800.0, 450.0));
	}

	#[test]
	fn capture_frame_background_plan_matches_native_wallpaper_fallback() {
		let plan = capture_frame::capture_frame_background_plan(
			CaptureFrameBackgroundKind::SystemWallpaper,
		);

		assert!(plan.prefers_wallpaper);
		assert_eq!(plan.wallpaper_overlay_alpha, 0.10);
		assert_eq!(plan.locations, [0.0, 0.54, 1.0]);
		assert_eq!(
			plan.colors,
			[
				CaptureFrameColorStop::new(0.10, 0.16, 0.28, 1.0),
				CaptureFrameColorStop::new(0.30, 0.47, 0.71, 1.0),
				CaptureFrameColorStop::new(0.95, 0.61, 0.43, 1.0),
			]
		);
	}

	#[test]
	fn capture_frame_background_plan_matches_native_linen_gradient() {
		let plan = capture_frame::capture_frame_background_plan(CaptureFrameBackgroundKind::Linen);

		assert!(!plan.prefers_wallpaper);
		assert_eq!(plan.wallpaper_overlay_alpha, 0.0);
		assert_eq!(plan.locations, [0.0, 0.54, 1.0]);
		assert_eq!(
			plan.colors,
			[
				CaptureFrameColorStop::new(0.83, 0.87, 0.82, 1.0),
				CaptureFrameColorStop::new(0.58, 0.70, 0.71, 1.0),
				CaptureFrameColorStop::new(0.24, 0.36, 0.47, 1.0),
			]
		);
	}

	#[test]
	fn capture_frame_wallpaper_request_plan_matches_native_thumbnail_policy() {
		let request = capture_frame::capture_frame_wallpaper_request_plan(
			CaptureFrameBackgroundKind::SystemWallpaper,
			1_535.2,
			996.0,
		)
		.expect("wallpaper request");

		assert_eq!(request.target_pixel_size, 1_536);
		assert_eq!(request.overlay_alpha, 0.10);
	}

	#[test]
	fn capture_frame_wallpaper_request_plan_skips_non_wallpaper_backgrounds() {
		assert_eq!(
			capture_frame::capture_frame_wallpaper_request_plan(
				CaptureFrameBackgroundKind::Aurora,
				1_536.0,
				996.0
			),
			None
		);
	}

	#[test]
	fn capture_frame_wallpaper_request_plan_rejects_empty_destination() {
		assert_eq!(
			capture_frame::capture_frame_wallpaper_request_plan(
				CaptureFrameBackgroundKind::SystemWallpaper,
				0.0,
				996.0
			),
			None
		);
	}

	#[test]
	fn capture_frame_renderer_expands_canvas_and_draws_source_pixels() {
		let source_rgba = vec![
			255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 255,
			0, 255, 255, 255, 255, 0, 255, 255, 20, 30, 40, 255,
		];
		let source = CaptureFrameRenderImageRef::new(4, 2, &source_rgba)
			.expect("source fixture should be valid");
		let rendered = capture_frame::render_capture_frame_effect(
			source,
			CaptureFrameBackgroundKind::Aurora,
			2.0,
			CaptureFrameSourceKind::DragRegion,
			CaptureFrameRenderKind::WindowSnapshot,
			None,
		)
		.expect("render should succeed")
		.expect("render should produce an image");

		assert_eq!(rendered.width(), 100);
		assert_eq!(rendered.height(), 98);

		let first_source_pixel = ((48 * rendered.width() as usize) + 48) * 4;

		assert_eq!(
			&rendered.as_raw()[first_source_pixel..first_source_pixel + 4],
			&[255, 0, 0, 255]
		);
	}

	#[test]
	fn capture_frame_renderer_uses_wallpaper_thumbnail_when_available() {
		let source_rgba = vec![255; 2 * 2 * 4];
		let wallpaper_rgba = [64, 128, 255, 255].repeat(8 * 8);
		let source = CaptureFrameRenderImageRef::new(2, 2, &source_rgba)
			.expect("source fixture should be valid");
		let wallpaper = CaptureFrameRenderImageRef::new(8, 8, &wallpaper_rgba)
			.expect("wallpaper fixture should be valid");
		let rendered = capture_frame::render_capture_frame_effect(
			source,
			CaptureFrameBackgroundKind::SystemWallpaper,
			2.0,
			CaptureFrameSourceKind::Window,
			CaptureFrameRenderKind::WindowSnapshot,
			Some(wallpaper),
		)
		.expect("render should succeed")
		.expect("render should produce an image");

		assert_eq!(&rendered.as_raw()[0..4], &[58, 115, 230, 255]);
	}

	#[test]
	fn capture_frame_renderer_rejects_invalid_source_bytes() {
		let error = CaptureFrameRenderImageRef::new(2, 2, &[0; 15])
			.expect_err("invalid source length should fail")
			.to_string();

		assert!(error.contains("byte length mismatch"));
	}

	fn assert_shadow_near(actual: CaptureFrameShadow, expected: CaptureFrameShadow) {
		const TOLERANCE: f64 = 0.000_001;

		assert!((actual.offset_x - expected.offset_x).abs() <= TOLERANCE);
		assert!((actual.offset_y - expected.offset_y).abs() <= TOLERANCE);
		assert!((actual.blur - expected.blur).abs() <= TOLERANCE);
		assert!((actual.alpha - expected.alpha).abs() <= TOLERANCE);
	}
}
