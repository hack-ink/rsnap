use crate::{
	DisplayPointRect, FrozenOverlayExportArrow, FrozenOverlayExportElement,
	FrozenOverlayExportMosaic, FrozenOverlayExportPen, FrozenOverlayExportPoint,
	FrozenOverlayExportSpotlight, FrozenOverlayExportSpotlightStyle,
	FrozenOverlayExportStrokeStyle, FrozenOverlayExportText, FrozenOverlayExportTextStyle,
};

/// Frozen annotation color selected by the host UI.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FrozenOverlayEditColor {
	/// White annotation color.
	White,
	/// Yellow annotation color.
	Yellow,
	/// Green annotation color.
	Green,
	/// Blue annotation color.
	#[default]
	Blue,
	/// Red annotation color.
	Red,
	/// Black annotation color.
	Black,
}
impl FrozenOverlayEditColor {
	/// Returns the non-premultiplied sRGBA export color.
	#[must_use]
	pub const fn export_rgba(self) -> [u8; 4] {
		match self {
			Self::White => [255, 255, 255, 255],
			Self::Yellow => [255, 219, 77, 255],
			Self::Green => [92, 214, 149, 255],
			Self::Blue => [102, 178, 255, 255],
			Self::Red => [255, 107, 107, 255],
			Self::Black => [24, 24, 24, 255],
		}
	}
}

/// Point-space coordinate used by frozen-overlay edit state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrozenOverlayEditPoint {
	/// X coordinate in frozen capture point-space.
	pub x: f64,
	/// Y coordinate in frozen capture point-space.
	pub y: f64,
}
impl FrozenOverlayEditPoint {
	/// Creates a frozen-overlay edit point.
	#[must_use]
	pub const fn new(x: f64, y: f64) -> Self {
		Self { x, y }
	}

	pub(super) fn distance_to(self, other: Self) -> f64 {
		(self.x - other.x).hypot(self.y - other.y)
	}

	pub(super) fn export_point(self) -> FrozenOverlayExportPoint {
		FrozenOverlayExportPoint::new(self.x, self.y)
	}
}

/// Rect-space coordinate used by frozen-overlay edit state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrozenOverlayEditRect {
	/// Left coordinate in frozen capture point-space.
	pub x: f64,
	/// Top coordinate in frozen capture point-space.
	pub y: f64,
	/// Rectangle width in frozen capture points.
	pub width: f64,
	/// Rectangle height in frozen capture points.
	pub height: f64,
}
impl FrozenOverlayEditRect {
	/// Creates a frozen-overlay edit rectangle.
	#[must_use]
	pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
		Self { x, y, width, height }
	}

	/// Converts a shared display-point rectangle into edit geometry.
	#[must_use]
	pub const fn from_display_rect(rect: DisplayPointRect) -> Self {
		Self::new(rect.x, rect.y, rect.width, rect.height)
	}

	/// Converts edit geometry into a shared display-point rectangle.
	#[must_use]
	pub const fn display_rect(self) -> DisplayPointRect {
		DisplayPointRect::new(self.x, self.y, self.width, self.height)
	}

	/// Returns true when the rectangle has finite, positive dimensions.
	#[must_use]
	pub fn is_valid(self) -> bool {
		self.x.is_finite()
			&& self.y.is_finite()
			&& self.width.is_finite()
			&& self.height.is_finite()
			&& self.width > 0.0
			&& self.height > 0.0
	}

	/// Returns true when the point lies inside the rectangle bounds.
	#[must_use]
	pub fn contains(self, point: FrozenOverlayEditPoint) -> bool {
		self.is_valid()
			&& point.x >= self.x
			&& point.y >= self.y
			&& point.x < self.max_x()
			&& point.y < self.max_y()
	}

	pub(super) fn max_x(self) -> f64 {
		self.x + self.width
	}

	pub(super) fn max_y(self) -> f64 {
		self.y + self.height
	}

	pub(super) fn inset(self, dx: f64, dy: f64) -> Self {
		Self::new(
			self.x + dx,
			self.y + dy,
			(self.width - dx * 2.0).max(0.0),
			(self.height - dy * 2.0).max(0.0),
		)
	}

	pub(super) fn clamp_point(self, point: FrozenOverlayEditPoint) -> FrozenOverlayEditPoint {
		FrozenOverlayEditPoint::new(
			point.x.clamp(self.x, self.max_x()),
			point.y.clamp(self.y, self.max_y()),
		)
	}

	pub(super) fn normalized_rect(
		self,
		anchor: FrozenOverlayEditPoint,
		current: FrozenOverlayEditPoint,
	) -> Self {
		let clamped_anchor = self.clamp_point(anchor);
		let clamped_current = self.clamp_point(current);

		Self::new(
			clamped_anchor.x.min(clamped_current.x),
			clamped_anchor.y.min(clamped_current.y),
			(clamped_current.x - clamped_anchor.x).abs(),
			(clamped_current.y - clamped_anchor.y).abs(),
		)
	}
}

/// Stroke style used by frozen pen and arrow annotations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayEditStrokeStyle {
	/// Stroke width in frozen capture points.
	pub stroke_width_points: f64,
	/// Selected annotation color.
	pub color: FrozenOverlayEditColor,
}
impl FrozenOverlayEditStrokeStyle {
	fn export_style(self) -> FrozenOverlayExportStrokeStyle {
		FrozenOverlayExportStrokeStyle {
			stroke_width_points: self.stroke_width_points as f32,
			rgba: self.color.export_rgba(),
		}
	}
}

impl Default for FrozenOverlayEditStrokeStyle {
	fn default() -> Self {
		Self { stroke_width_points: 3.0, color: FrozenOverlayEditColor::Blue }
	}
}

/// Spotlight style used by frozen spotlight annotations.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrozenOverlayEditSpotlightStyle {
	/// Border width in frozen capture points.
	pub border_width_points: f64,
	/// Selected border color.
	pub border_color: FrozenOverlayEditColor,
}
impl FrozenOverlayEditSpotlightStyle {
	fn export_style(self) -> FrozenOverlayExportSpotlightStyle {
		FrozenOverlayExportSpotlightStyle {
			border_width_points: self.border_width_points as f32,
			border_rgba: self.border_color.export_rgba(),
		}
	}
}

/// Text style used by frozen text annotations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayEditTextStyle {
	/// Font size in frozen capture points.
	pub font_size_points: f64,
	/// Selected text color.
	pub color: FrozenOverlayEditColor,
}
impl FrozenOverlayEditTextStyle {
	fn export_style(self) -> FrozenOverlayExportTextStyle {
		FrozenOverlayExportTextStyle {
			font_size_points: self.font_size_points as f32,
			rgba: self.color.export_rgba(),
		}
	}
}

impl Default for FrozenOverlayEditTextStyle {
	fn default() -> Self {
		Self { font_size_points: 16.0, color: FrozenOverlayEditColor::Blue }
	}
}

/// Full annotation style payload provided by the native host UI.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrozenOverlayEditStyle {
	/// Current pen and arrow style.
	pub stroke: FrozenOverlayEditStrokeStyle,
	/// Current spotlight style.
	pub spotlight: FrozenOverlayEditSpotlightStyle,
	/// Current text style.
	pub text: FrozenOverlayEditTextStyle,
}

/// Frozen pen stroke annotation.
#[derive(Clone, Debug, PartialEq)]
pub struct FrozenOverlayEditPen {
	/// Stroke points in frozen capture point-space.
	pub points: Vec<FrozenOverlayEditPoint>,
	/// Stroke style.
	pub style: FrozenOverlayEditStrokeStyle,
}
impl FrozenOverlayEditPen {
	fn export_element(&self) -> FrozenOverlayExportElement {
		FrozenOverlayExportElement::Pen(FrozenOverlayExportPen {
			points: self.points.iter().map(|point| point.export_point()).collect(),
			style: self.style.export_style(),
		})
	}
}

/// Frozen arrow annotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayEditArrow {
	/// Arrow tail in frozen capture point-space.
	pub start: FrozenOverlayEditPoint,
	/// Arrow tip in frozen capture point-space.
	pub end: FrozenOverlayEditPoint,
	/// Stroke style.
	pub style: FrozenOverlayEditStrokeStyle,
}
impl FrozenOverlayEditArrow {
	fn export_element(self) -> FrozenOverlayExportElement {
		FrozenOverlayExportElement::Arrow(FrozenOverlayExportArrow {
			start: self.start.export_point(),
			end: self.end.export_point(),
			style: self.style.export_style(),
		})
	}
}

/// Frozen mosaic privacy annotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayEditMosaic {
	/// Mosaic rectangle in frozen capture point-space.
	pub rect: FrozenOverlayEditRect,
}
impl FrozenOverlayEditMosaic {
	fn export_element(self) -> FrozenOverlayExportElement {
		FrozenOverlayExportElement::Mosaic(FrozenOverlayExportMosaic {
			rect: self.rect.display_rect(),
		})
	}
}

/// Frozen spotlight annotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayEditSpotlight {
	/// Spotlight rectangle in frozen capture point-space.
	pub rect: FrozenOverlayEditRect,
	/// Spotlight style.
	pub style: FrozenOverlayEditSpotlightStyle,
}
impl FrozenOverlayEditSpotlight {
	fn export_element(self) -> FrozenOverlayExportElement {
		FrozenOverlayExportElement::Spotlight(FrozenOverlayExportSpotlight {
			rect: self.rect.display_rect(),
			style: self.style.export_style(),
		})
	}
}

/// Frozen text annotation.
#[derive(Clone, Debug, PartialEq)]
pub struct FrozenOverlayEditText {
	/// Text anchor in frozen capture point-space.
	pub anchor: FrozenOverlayEditPoint,
	/// Text payload.
	pub text: String,
	/// Text style.
	pub style: FrozenOverlayEditTextStyle,
}
impl FrozenOverlayEditText {
	fn export_element(&self) -> FrozenOverlayExportElement {
		FrozenOverlayExportElement::Text(FrozenOverlayExportText {
			anchor: self.anchor.export_point(),
			text: self.text.clone(),
			style: self.style.export_style(),
		})
	}
}

/// One committed frozen-overlay edit.
#[derive(Clone, Debug, PartialEq)]
pub enum FrozenOverlayEditElement {
	/// Pen stroke annotation.
	Pen(FrozenOverlayEditPen),
	/// Arrow annotation.
	Arrow(FrozenOverlayEditArrow),
	/// Mosaic privacy rectangle.
	Mosaic(FrozenOverlayEditMosaic),
	/// Spotlight annotation.
	Spotlight(FrozenOverlayEditSpotlight),
	/// Text annotation.
	Text(FrozenOverlayEditText),
}
impl FrozenOverlayEditElement {
	/// Converts the edit element into the export compositor payload.
	#[must_use]
	pub fn export_element(&self) -> FrozenOverlayExportElement {
		match self {
			Self::Pen(annotation) => annotation.export_element(),
			Self::Arrow(annotation) => annotation.export_element(),
			Self::Mosaic(annotation) => annotation.export_element(),
			Self::Spotlight(annotation) => annotation.export_element(),
			Self::Text(annotation) => annotation.export_element(),
		}
	}
}

/// Active text edit payload.
#[derive(Clone, Debug, PartialEq)]
pub struct FrozenOverlayTextEdit {
	/// Text anchor in frozen capture point-space.
	pub anchor: FrozenOverlayEditPoint,
	/// Uncommitted text payload.
	pub text: String,
}

/// Snapshot copied from the Rust-owned edit state for host rendering.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrozenOverlayEditSnapshot {
	/// Whether undo is available.
	pub can_undo: bool,
	/// Whether redo is available.
	pub can_redo: bool,
	/// Whether selection transforms should be locked out by existing edits.
	pub keeps_frozen_selection_fixed: bool,
	/// Whether a movable annotation is currently being dragged.
	pub is_moving_movable_annotation: bool,
	/// Whether any pointer interaction is currently active.
	pub has_active_interaction: bool,
	/// Visible committed elements, excluding the target currently represented by a move preview.
	pub elements: Vec<FrozenOverlayEditElement>,
	/// Active pen preview.
	pub preview_pen: Option<FrozenOverlayEditPen>,
	/// Active arrow preview.
	pub preview_arrow: Option<FrozenOverlayEditArrow>,
	/// Active mosaic or mosaic-move preview.
	pub preview_mosaic: Option<FrozenOverlayEditMosaic>,
	/// Active spotlight preview.
	pub preview_spotlight: Option<FrozenOverlayEditSpotlight>,
	/// Active moved text preview.
	pub preview_text: Option<FrozenOverlayEditText>,
	/// Active text edit state.
	pub active_text_edit: Option<FrozenOverlayTextEdit>,
}
