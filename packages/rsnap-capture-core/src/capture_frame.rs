//! Capture-frame layout and rendering plans owned by the Rust product core.

use crate::DisplayPointRect;

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
		shadows: capture_frame_shadows(canvas_width, canvas_height),
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

fn capture_frame_shadows(canvas_width: f64, canvas_height: f64) -> [CaptureFrameShadow; 3] {
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

#[cfg(test)]
mod tests {
	use super::{
		CaptureFrameBackgroundKind, CaptureFrameColorStop, CaptureFrameShadow,
		CaptureFrameSourceKind, capture_frame_aspect_fill_crop_rect, capture_frame_background_plan,
		capture_frame_plan,
	};
	use crate::DisplayPointRect;

	#[test]
	fn capture_frame_plan_matches_native_window_geometry() {
		let plan =
			capture_frame_plan(320, 180, 2.0, CaptureFrameSourceKind::Window).expect("valid plan");

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
	fn capture_frame_plan_scales_large_shadow_geometry() {
		let plan = capture_frame_plan(1440, 900, 2.0, CaptureFrameSourceKind::DragRegion)
			.expect("valid plan");

		assert_eq!(plan.canvas_width, 1647.0);
		assert_eq!(plan.canvas_height, 1107.0);
		assert_eq!(plan.image_rect, DisplayPointRect::new(103.5, 103.5, 1440.0, 900.0));
		assert_eq!(plan.corner_radius, 22.5);
		assert_shadow_near(plan.shadows[0], CaptureFrameShadow::new(0.0, 0.0, 94.095, 0.30));
		assert_shadow_near(plan.shadows[1], CaptureFrameShadow::new(0.0, -33.21, 55.35, 0.36));
		assert_shadow_near(plan.shadows[2], CaptureFrameShadow::new(0.0, -6.642, 15.498, 0.22));
	}

	#[test]
	fn capture_frame_plan_rejects_empty_input() {
		assert!(capture_frame_plan(0, 180, 2.0, CaptureFrameSourceKind::Window).is_none());
		assert!(capture_frame_plan(320, 0, 2.0, CaptureFrameSourceKind::Window).is_none());
	}

	#[test]
	fn capture_frame_aspect_fill_crop_matches_native_wide_source() {
		let rect = capture_frame_aspect_fill_crop_rect(1600, 900, 1000.0, 1000.0)
			.expect("valid crop rect");

		assert_eq!(rect, DisplayPointRect::new(350.0, 0.0, 900.0, 900.0));
	}

	#[test]
	fn capture_frame_aspect_fill_crop_matches_native_tall_source() {
		let rect =
			capture_frame_aspect_fill_crop_rect(800, 1200, 1600.0, 900.0).expect("valid crop rect");

		assert_eq!(rect, DisplayPointRect::new(0.0, 375.0, 800.0, 450.0));
	}

	#[test]
	fn capture_frame_background_plan_matches_native_wallpaper_fallback() {
		let plan = capture_frame_background_plan(CaptureFrameBackgroundKind::SystemWallpaper);

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
		let plan = capture_frame_background_plan(CaptureFrameBackgroundKind::Linen);

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

	fn assert_shadow_near(actual: CaptureFrameShadow, expected: CaptureFrameShadow) {
		const TOLERANCE: f64 = 0.000_001;

		assert!((actual.offset_x - expected.offset_x).abs() <= TOLERANCE);
		assert!((actual.offset_y - expected.offset_y).abs() <= TOLERANCE);
		assert!((actual.blur - expected.blur).abs() <= TOLERANCE);
		assert!((actual.alpha - expected.alpha).abs() <= TOLERANCE);
	}
}
