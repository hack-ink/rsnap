use color_eyre::eyre::{self, Result};

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

pub(super) fn expected_rgba_len(width: u32, height: u32) -> Result<usize> {
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
