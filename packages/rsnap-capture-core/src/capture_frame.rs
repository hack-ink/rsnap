//! Capture-frame layout and rendering owned by the Rust product core.

mod model;
mod rendering;

pub use self::model::{
	CaptureFrameBackgroundKind, CaptureFrameBackgroundPlan, CaptureFrameColorStop,
	CaptureFramePlan, CaptureFrameRenderImageRef, CaptureFrameRenderKind, CaptureFrameShadow,
	CaptureFrameSourceKind, CaptureFrameWallpaperRequest,
};
pub use self::rendering::render_capture_frame_effect;

use crate::DisplayPointRect;

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
