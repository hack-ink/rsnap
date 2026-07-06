use crate::DisplayPointRect;

/// Point-space coordinate used by frozen-overlay export annotations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportPoint {
	/// X coordinate in frozen capture point-space.
	pub x: f64,
	/// Y coordinate in frozen capture point-space.
	pub y: f64,
}
impl FrozenOverlayExportPoint {
	/// Creates a frozen-overlay export point.
	#[must_use]
	pub const fn new(x: f64, y: f64) -> Self {
		Self { x, y }
	}
}

/// Stroke style used by pen and arrow export annotations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportStrokeStyle {
	/// Stroke width in frozen capture points.
	pub stroke_width_points: f32,
	/// Source color as non-premultiplied RGBA bytes.
	pub rgba: [u8; 4],
}

/// Spotlight border style used by export annotations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportSpotlightStyle {
	/// Border width in frozen capture points.
	pub border_width_points: f32,
	/// Border color as non-premultiplied RGBA bytes.
	pub border_rgba: [u8; 4],
}

/// Text style used by export annotations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportTextStyle {
	/// Font size in frozen capture points.
	pub font_size_points: f32,
	/// Text fill color as non-premultiplied RGBA bytes.
	pub rgba: [u8; 4],
}

/// Pen stroke export annotation.
#[derive(Clone, Debug, PartialEq)]
pub struct FrozenOverlayExportPen {
	/// Stroke points in frozen capture point-space.
	pub points: Vec<FrozenOverlayExportPoint>,
	/// Stroke style.
	pub style: FrozenOverlayExportStrokeStyle,
}

/// Arrow export annotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportArrow {
	/// Arrow tail in frozen capture point-space.
	pub start: FrozenOverlayExportPoint,
	/// Arrow tip in frozen capture point-space.
	pub end: FrozenOverlayExportPoint,
	/// Stroke style.
	pub style: FrozenOverlayExportStrokeStyle,
}

/// Mosaic privacy export annotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportMosaic {
	/// Mosaic rectangle in frozen capture point-space.
	pub rect: DisplayPointRect,
}

/// Spotlight export annotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportSpotlight {
	/// Spotlight rectangle in frozen capture point-space.
	pub rect: DisplayPointRect,
	/// Spotlight border style.
	pub style: FrozenOverlayExportSpotlightStyle,
}

/// Text export annotation.
#[derive(Clone, Debug, PartialEq)]
pub struct FrozenOverlayExportText {
	/// Text anchor in frozen capture point-space.
	pub anchor: FrozenOverlayExportPoint,
	/// Text payload.
	pub text: String,
	/// Text style.
	pub style: FrozenOverlayExportTextStyle,
}

/// One committed frozen-overlay edit to composite into an exported image.
#[derive(Clone, Debug, PartialEq)]
pub enum FrozenOverlayExportElement {
	/// Pen stroke annotation.
	Pen(FrozenOverlayExportPen),
	/// Arrow annotation.
	Arrow(FrozenOverlayExportArrow),
	/// Mosaic privacy rectangle.
	Mosaic(FrozenOverlayExportMosaic),
	/// Spotlight annotation.
	Spotlight(FrozenOverlayExportSpotlight),
	/// Text annotation.
	Text(FrozenOverlayExportText),
}
