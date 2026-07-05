use crate::abi::RsnapFloatRect;

/// FFI-safe capture-frame source discriminator.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapCaptureFrameSourceKind {
	/// User-dragged region capture.
	DragRegion = 0,
	/// Single-window capture.
	Window = 1,
	/// Full-screen capture.
	FullScreen = 2,
	/// Scroll-capture export.
	ScrollCapture = 3,
	/// Unknown or future capture source.
	Unknown = 4,
}

/// FFI-safe capture-frame background discriminator.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapCaptureFrameBackgroundKind {
	/// Prefer system wallpaper with gradient fallback.
	SystemWallpaper = 0,
	/// Blue-to-warm product gradient.
	Aurora = 1,
	/// Neutral graphite gradient.
	Graphite = 2,
	/// Light linen gradient.
	Linen = 3,
}

/// FFI-safe capture-frame render mode.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapCaptureFrameRenderKind {
	/// Draw shadows and rounded clipping around the capture.
	FramedCapture = 0,
	/// Draw a floating full-window snapshot without added clipping.
	WindowSnapshot = 1,
}

/// FFI-safe sRGB capture-frame color stop.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RsnapCaptureFrameColorStop {
	/// Red component in sRGB space.
	pub red: f64,
	/// Green component in sRGB space.
	pub green: f64,
	/// Blue component in sRGB space.
	pub blue: f64,
	/// Alpha component.
	pub alpha: f64,
}

/// FFI-safe capture-frame background drawing plan.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RsnapCaptureFrameBackgroundPlan {
	/// Ordered sRGB gradient color stops.
	pub colors: [RsnapCaptureFrameColorStop; 3],
	/// Gradient locations matching `colors`.
	pub locations: [f64; 3],
	/// Non-zero when the host should first try drawing system wallpaper.
	pub prefers_wallpaper: u8,
	/// Overlay alpha applied when wallpaper drawing succeeds.
	pub wallpaper_overlay_alpha: f64,
}

/// FFI-safe capture-frame shadow pass.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RsnapCaptureFrameShadow {
	/// Horizontal shadow offset in output pixels.
	pub offset_x: f64,
	/// Vertical shadow offset in output pixels.
	pub offset_y: f64,
	/// Shadow blur radius in output pixels.
	pub blur: f64,
	/// Shadow alpha.
	pub alpha: f64,
}

/// FFI-safe capture-frame drawing plan.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RsnapCaptureFramePlan {
	/// Canvas width in output pixels.
	pub canvas_width: f64,
	/// Canvas height in output pixels.
	pub canvas_height: f64,
	/// Image placement inside the canvas.
	pub image_rect: RsnapFloatRect,
	/// Rounded capture corner radius.
	pub corner_radius: f64,
	/// Ordered shadow passes behind the framed capture.
	pub shadows: [RsnapCaptureFrameShadow; 3],
}

/// FFI-safe platform wallpaper thumbnail request.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RsnapCaptureFrameWallpaperRequest {
	/// Maximum thumbnail dimension requested from the platform image pipeline.
	pub target_pixel_size: u32,
	/// Overlay alpha applied after drawing the wallpaper thumbnail.
	pub overlay_alpha: f64,
}
