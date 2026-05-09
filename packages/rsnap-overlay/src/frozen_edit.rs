//! Portable frozen-overlay edit state owned by Rust.

use crate::frozen_export::{
	FrozenOverlayExportArrow, FrozenOverlayExportElement, FrozenOverlayExportMosaic,
	FrozenOverlayExportPen, FrozenOverlayExportPoint, FrozenOverlayExportSpotlight,
	FrozenOverlayExportSpotlightStyle, FrozenOverlayExportStrokeStyle, FrozenOverlayExportText,
	FrozenOverlayExportTextStyle,
};
use crate::text_rendering::{self, TextBounds};
use rsnap_capture_core::{DisplayPointRect, ToolbarItemKind};

const PEN_SAMPLE_MIN_DISTANCE_POINTS: f64 = 1.5;
const ARROW_MIN_DISTANCE_POINTS: f64 = 6.0;
const RECT_MIN_SIZE_POINTS: f64 = 6.0;
const TEXT_HIT_PADDING_POINTS: f64 = 4.0;

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

	fn distance_to(self, other: Self) -> f64 {
		(self.x - other.x).hypot(self.y - other.y)
	}

	fn export_point(self) -> FrozenOverlayExportPoint {
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

	fn max_x(self) -> f64 {
		self.x + self.width
	}

	fn max_y(self) -> f64 {
		self.y + self.height
	}

	fn inset(self, dx: f64, dy: f64) -> Self {
		Self::new(
			self.x + dx,
			self.y + dy,
			(self.width - dx * 2.0).max(0.0),
			(self.height - dy * 2.0).max(0.0),
		)
	}

	fn clamp_point(self, point: FrozenOverlayEditPoint) -> FrozenOverlayEditPoint {
		FrozenOverlayEditPoint::new(
			point.x.clamp(self.x, self.max_x()),
			point.y.clamp(self.y, self.max_y()),
		)
	}

	fn normalized_rect(
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

#[derive(Clone, Debug, PartialEq)]
enum ActiveFrozenOverlayEdit {
	Pen {
		points: Vec<FrozenOverlayEditPoint>,
		style: FrozenOverlayEditStrokeStyle,
	},
	Arrow {
		start: FrozenOverlayEditPoint,
		current: FrozenOverlayEditPoint,
		style: FrozenOverlayEditStrokeStyle,
	},
	Mosaic {
		anchor: FrozenOverlayEditPoint,
		current: FrozenOverlayEditPoint,
	},
	MosaicMove {
		index: usize,
		current_rect: FrozenOverlayEditRect,
		drag_offset: FrozenOverlayEditPoint,
	},
	TextMove {
		index: usize,
		current_annotation: FrozenOverlayEditText,
		drag_offset: FrozenOverlayEditPoint,
	},
	Spotlight {
		anchor: FrozenOverlayEditPoint,
		current: FrozenOverlayEditPoint,
		style: FrozenOverlayEditSpotlightStyle,
	},
}

#[derive(Clone, Debug, PartialEq)]
enum FrozenOverlayMoveTarget {
	Mosaic { index: usize, rect: FrozenOverlayEditRect },
	Text { index: usize, annotation: FrozenOverlayEditText },
}

/// Rust-owned frozen-overlay edit state.
#[derive(Debug, Default)]
pub struct FrozenOverlayEditSession {
	edits: Vec<FrozenOverlayEditElement>,
	redo_edits: Vec<FrozenOverlayEditElement>,
	active_interaction: Option<ActiveFrozenOverlayEdit>,
	active_text_edit: Option<FrozenOverlayTextEdit>,
}
impl FrozenOverlayEditSession {
	/// Clears all committed, redo, and active edit state.
	pub fn reset(&mut self) {
		self.edits.clear();
		self.redo_edits.clear();

		self.active_interaction = None;
		self.active_text_edit = None;
	}

	/// Starts a frozen-overlay interaction for the selected toolbar tool.
	pub fn begin(
		&mut self,
		tool: ToolbarItemKind,
		point: FrozenOverlayEditPoint,
		selection: FrozenOverlayEditRect,
		style: FrozenOverlayEditStyle,
	) -> bool {
		if !selection.contains(point) {
			return false;
		}

		match tool {
			ToolbarItemKind::Pen => self.begin_pen(point, style.stroke),
			ToolbarItemKind::Arrow => self.begin_arrow(point, style.stroke),
			ToolbarItemKind::Mosaic => self.begin_mosaic(point),
			ToolbarItemKind::Pointer => self.begin_move(point),
			ToolbarItemKind::Spotlight => self.begin_spotlight(point, style.spotlight),
			ToolbarItemKind::Text => {
				let _ = self.commit_text_edit(style.text);

				self.active_text_edit = Some(FrozenOverlayTextEdit {
					anchor: selection.clamp_point(point),
					text: String::new(),
				});

				true
			},
			ToolbarItemKind::Undo
			| ToolbarItemKind::Redo
			| ToolbarItemKind::AutoCenter
			| ToolbarItemKind::Scroll
			| ToolbarItemKind::Ocr
			| ToolbarItemKind::Copy
			| ToolbarItemKind::Save => false,
		}
	}

	/// Updates the active frozen-overlay pointer interaction.
	pub fn update(
		&mut self,
		point: FrozenOverlayEditPoint,
		selection: FrozenOverlayEditRect,
	) -> bool {
		let Some(active) = self.active_interaction.take() else {
			return false;
		};
		let next = match active {
			ActiveFrozenOverlayEdit::Pen { mut points, style } => {
				let clamped = selection.clamp_point(point);

				if points.last().is_some_and(|last_point| {
					last_point.distance_to(clamped) < PEN_SAMPLE_MIN_DISTANCE_POINTS
				}) {
					self.active_interaction = Some(ActiveFrozenOverlayEdit::Pen { points, style });

					return false;
				}

				points.push(clamped);

				ActiveFrozenOverlayEdit::Pen { points, style }
			},
			ActiveFrozenOverlayEdit::Arrow { start, style, .. } => ActiveFrozenOverlayEdit::Arrow {
				start,
				current: selection.clamp_point(point),
				style,
			},
			ActiveFrozenOverlayEdit::Mosaic { anchor, .. } => {
				ActiveFrozenOverlayEdit::Mosaic { anchor, current: selection.clamp_point(point) }
			},
			ActiveFrozenOverlayEdit::MosaicMove { index, current_rect, drag_offset } => {
				ActiveFrozenOverlayEdit::MosaicMove {
					index,
					current_rect: moved_rect(current_rect, drag_offset, point, selection),
					drag_offset,
				}
			},
			ActiveFrozenOverlayEdit::TextMove { index, current_annotation, drag_offset } => {
				ActiveFrozenOverlayEdit::TextMove {
					index,
					current_annotation: moved_text_annotation(
						current_annotation,
						drag_offset,
						point,
						selection,
					),
					drag_offset,
				}
			},
			ActiveFrozenOverlayEdit::Spotlight { anchor, style, .. } => {
				ActiveFrozenOverlayEdit::Spotlight {
					anchor,
					current: selection.clamp_point(point),
					style,
				}
			},
		};

		self.active_interaction = Some(next);

		true
	}

	/// Finishes the active frozen-overlay pointer interaction.
	pub fn finish(&mut self, selection: FrozenOverlayEditRect) -> bool {
		let Some(active) = self.active_interaction.take() else {
			return false;
		};
		let mut changed = true;
		let accepted = match active {
			ActiveFrozenOverlayEdit::Pen { points, style } => self.finish_pen(points, style),
			ActiveFrozenOverlayEdit::Arrow { start, current, style } => {
				self.finish_arrow(start, current, style)
			},
			ActiveFrozenOverlayEdit::Mosaic { anchor, current } => {
				self.finish_mosaic(selection, anchor, current)
			},
			ActiveFrozenOverlayEdit::MosaicMove { index, current_rect, .. } => {
				let Some(moved) = self.finish_mosaic_move(index, current_rect) else {
					return false;
				};

				changed = moved;

				true
			},
			ActiveFrozenOverlayEdit::TextMove { index, current_annotation, .. } => {
				let Some(moved) = self.finish_text_move(index, current_annotation) else {
					return false;
				};

				changed = moved;

				true
			},
			ActiveFrozenOverlayEdit::Spotlight { anchor, current, style } => {
				self.finish_spotlight(selection, anchor, current, style)
			},
		};

		if accepted && changed {
			self.redo_edits.clear();
		}

		accepted
	}

	/// Appends text to the active text edit after stripping carriage returns.
	pub fn append_text(&mut self, text: &str) -> bool {
		let Some(active_text_edit) = self.active_text_edit.as_mut() else {
			return false;
		};
		let sanitized = text.replace('\r', "");

		if sanitized.is_empty() {
			return false;
		}

		active_text_edit.text.push_str(&sanitized);

		true
	}

	/// Deletes one Unicode scalar from the active text edit.
	pub fn backspace_text(&mut self) -> bool {
		self.active_text_edit
			.as_mut()
			.is_some_and(|active_text_edit| active_text_edit.text.pop().is_some())
	}

	/// Commits the active text edit using the provided style.
	pub fn commit_text_edit(&mut self, style: FrozenOverlayEditTextStyle) -> bool {
		let Some(active_text_edit) = self.active_text_edit.take() else {
			return false;
		};

		if active_text_edit.text.trim().is_empty() {
			return false;
		}

		self.edits.push(FrozenOverlayEditElement::Text(FrozenOverlayEditText {
			anchor: active_text_edit.anchor,
			text: active_text_edit.text,
			style,
		}));
		self.redo_edits.clear();

		true
	}

	/// Cancels the active text edit without committing it.
	pub fn cancel_text_edit(&mut self) {
		self.active_text_edit = None;
	}

	/// Moves the last committed edit to the redo stack.
	pub fn undo(&mut self) -> bool {
		self.active_text_edit = None;

		let Some(edit) = self.edits.pop() else {
			return false;
		};

		self.redo_edits.push(edit);

		true
	}

	/// Restores the last redo edit.
	pub fn redo(&mut self) -> bool {
		self.active_text_edit = None;

		let Some(edit) = self.redo_edits.pop() else {
			return false;
		};

		self.edits.push(edit);

		true
	}

	/// Returns true if a movable annotation is under the provided point.
	#[must_use]
	pub fn contains_movable_annotation(&self, point: FrozenOverlayEditPoint) -> bool {
		self.move_target(point).is_some()
	}

	/// Copies the current edit state as host-renderable data.
	#[must_use]
	pub fn snapshot(&self) -> FrozenOverlayEditSnapshot {
		let moving_mosaic = self.moving_mosaic_edit_index();
		let moving_text = self.moving_text_edit_index();

		FrozenOverlayEditSnapshot {
			can_undo: !self.edits.is_empty(),
			can_redo: !self.redo_edits.is_empty(),
			keeps_frozen_selection_fixed: self.keeps_frozen_selection_fixed(),
			is_moving_movable_annotation: self.is_moving_movable_annotation(),
			has_active_interaction: self.active_interaction.is_some(),
			elements: self.visible_elements(moving_mosaic, moving_text),
			preview_pen: self.preview_pen(),
			preview_arrow: self.preview_arrow(),
			preview_mosaic: self.preview_mosaic(),
			preview_spotlight: self.preview_spotlight(),
			preview_text: self.preview_text(),
			active_text_edit: self.active_text_edit.clone(),
		}
	}

	fn begin_pen(
		&mut self,
		point: FrozenOverlayEditPoint,
		style: FrozenOverlayEditStrokeStyle,
	) -> bool {
		self.active_interaction = Some(ActiveFrozenOverlayEdit::Pen { points: vec![point], style });

		true
	}

	fn begin_arrow(
		&mut self,
		point: FrozenOverlayEditPoint,
		style: FrozenOverlayEditStrokeStyle,
	) -> bool {
		self.active_interaction =
			Some(ActiveFrozenOverlayEdit::Arrow { start: point, current: point, style });

		true
	}

	fn begin_mosaic(&mut self, point: FrozenOverlayEditPoint) -> bool {
		self.active_interaction =
			Some(ActiveFrozenOverlayEdit::Mosaic { anchor: point, current: point });

		true
	}

	fn begin_spotlight(
		&mut self,
		point: FrozenOverlayEditPoint,
		style: FrozenOverlayEditSpotlightStyle,
	) -> bool {
		self.active_interaction =
			Some(ActiveFrozenOverlayEdit::Spotlight { anchor: point, current: point, style });

		true
	}

	fn begin_move(&mut self, point: FrozenOverlayEditPoint) -> bool {
		let Some(target) = self.move_target(point) else {
			return false;
		};

		self.active_interaction = Some(match target {
			FrozenOverlayMoveTarget::Mosaic { index, rect } => {
				ActiveFrozenOverlayEdit::MosaicMove {
					index,
					current_rect: rect,
					drag_offset: FrozenOverlayEditPoint::new(point.x - rect.x, point.y - rect.y),
				}
			},
			FrozenOverlayMoveTarget::Text { index, annotation } => {
				ActiveFrozenOverlayEdit::TextMove {
					index,
					drag_offset: FrozenOverlayEditPoint::new(
						point.x - annotation.anchor.x,
						point.y - annotation.anchor.y,
					),
					current_annotation: annotation,
				}
			},
		});

		true
	}

	fn finish_pen(
		&mut self,
		points: Vec<FrozenOverlayEditPoint>,
		style: FrozenOverlayEditStrokeStyle,
	) -> bool {
		if points.len() < 2 {
			return false;
		}

		self.edits.push(FrozenOverlayEditElement::Pen(FrozenOverlayEditPen { points, style }));

		true
	}

	fn finish_arrow(
		&mut self,
		start: FrozenOverlayEditPoint,
		current: FrozenOverlayEditPoint,
		style: FrozenOverlayEditStrokeStyle,
	) -> bool {
		if start.distance_to(current) < ARROW_MIN_DISTANCE_POINTS {
			return false;
		}

		self.edits.push(FrozenOverlayEditElement::Arrow(FrozenOverlayEditArrow {
			start,
			end: current,
			style,
		}));

		true
	}

	fn finish_mosaic(
		&mut self,
		selection: FrozenOverlayEditRect,
		anchor: FrozenOverlayEditPoint,
		current: FrozenOverlayEditPoint,
	) -> bool {
		let rect = selection.normalized_rect(anchor, current);

		if rect.width < RECT_MIN_SIZE_POINTS || rect.height < RECT_MIN_SIZE_POINTS {
			return false;
		}

		self.edits.push(FrozenOverlayEditElement::Mosaic(FrozenOverlayEditMosaic { rect }));

		true
	}

	fn finish_spotlight(
		&mut self,
		selection: FrozenOverlayEditRect,
		anchor: FrozenOverlayEditPoint,
		current: FrozenOverlayEditPoint,
		style: FrozenOverlayEditSpotlightStyle,
	) -> bool {
		let rect = selection.normalized_rect(anchor, current);

		if rect.width < RECT_MIN_SIZE_POINTS || rect.height < RECT_MIN_SIZE_POINTS {
			return false;
		}

		self.edits
			.push(FrozenOverlayEditElement::Spotlight(FrozenOverlayEditSpotlight { rect, style }));

		true
	}

	fn finish_mosaic_move(
		&mut self,
		index: usize,
		current_rect: FrozenOverlayEditRect,
	) -> Option<bool> {
		let Some(FrozenOverlayEditElement::Mosaic(annotation)) = self.edits.get_mut(index) else {
			return None;
		};

		if annotation.rect == current_rect {
			return Some(false);
		}

		annotation.rect = current_rect;

		Some(true)
	}

	fn finish_text_move(
		&mut self,
		index: usize,
		current_annotation: FrozenOverlayEditText,
	) -> Option<bool> {
		let Some(FrozenOverlayEditElement::Text(annotation)) = self.edits.get_mut(index) else {
			return None;
		};

		if *annotation == current_annotation {
			return Some(false);
		}

		*annotation = current_annotation;

		Some(true)
	}

	fn move_target(&self, point: FrozenOverlayEditPoint) -> Option<FrozenOverlayMoveTarget> {
		for (index, edit) in self.edits.iter().enumerate().rev() {
			match edit {
				FrozenOverlayEditElement::Mosaic(annotation) if annotation.rect.contains(point) => {
					return Some(FrozenOverlayMoveTarget::Mosaic { index, rect: annotation.rect });
				},
				FrozenOverlayEditElement::Text(annotation)
					if text_hit_bounds(annotation).contains(point) =>
				{
					return Some(FrozenOverlayMoveTarget::Text {
						index,
						annotation: annotation.clone(),
					});
				},
				FrozenOverlayEditElement::Pen(_)
				| FrozenOverlayEditElement::Arrow(_)
				| FrozenOverlayEditElement::Mosaic(_)
				| FrozenOverlayEditElement::Spotlight(_)
				| FrozenOverlayEditElement::Text(_) => {},
			}
		}

		None
	}

	fn keeps_frozen_selection_fixed(&self) -> bool {
		!self.edits.is_empty()
			|| !self.redo_edits.is_empty()
			|| self.active_interaction.is_some()
			|| self.active_text_edit.is_some()
	}

	fn is_moving_movable_annotation(&self) -> bool {
		matches!(
			self.active_interaction,
			Some(
				ActiveFrozenOverlayEdit::MosaicMove { .. }
					| ActiveFrozenOverlayEdit::TextMove { .. }
			)
		)
	}

	fn moving_mosaic_edit_index(&self) -> Option<usize> {
		match self.active_interaction {
			Some(ActiveFrozenOverlayEdit::MosaicMove { index, .. }) => Some(index),
			Some(
				ActiveFrozenOverlayEdit::Pen { .. }
				| ActiveFrozenOverlayEdit::Arrow { .. }
				| ActiveFrozenOverlayEdit::Mosaic { .. }
				| ActiveFrozenOverlayEdit::TextMove { .. }
				| ActiveFrozenOverlayEdit::Spotlight { .. },
			)
			| None => None,
		}
	}

	fn moving_text_edit_index(&self) -> Option<usize> {
		match self.active_interaction {
			Some(ActiveFrozenOverlayEdit::TextMove { index, .. }) => Some(index),
			Some(
				ActiveFrozenOverlayEdit::Pen { .. }
				| ActiveFrozenOverlayEdit::Arrow { .. }
				| ActiveFrozenOverlayEdit::Mosaic { .. }
				| ActiveFrozenOverlayEdit::MosaicMove { .. }
				| ActiveFrozenOverlayEdit::Spotlight { .. },
			)
			| None => None,
		}
	}

	fn visible_elements(
		&self,
		moving_mosaic: Option<usize>,
		moving_text: Option<usize>,
	) -> Vec<FrozenOverlayEditElement> {
		self.edits
			.iter()
			.enumerate()
			.filter_map(|(index, edit)| {
				if Some(index) == moving_mosaic || Some(index) == moving_text {
					None
				} else {
					Some(edit.clone())
				}
			})
			.collect()
	}

	fn preview_pen(&self) -> Option<FrozenOverlayEditPen> {
		match self.active_interaction.as_ref()? {
			ActiveFrozenOverlayEdit::Pen { points, style } => {
				Some(FrozenOverlayEditPen { points: points.clone(), style: *style })
			},
			ActiveFrozenOverlayEdit::Arrow { .. }
			| ActiveFrozenOverlayEdit::Mosaic { .. }
			| ActiveFrozenOverlayEdit::MosaicMove { .. }
			| ActiveFrozenOverlayEdit::TextMove { .. }
			| ActiveFrozenOverlayEdit::Spotlight { .. } => None,
		}
	}

	fn preview_arrow(&self) -> Option<FrozenOverlayEditArrow> {
		match self.active_interaction.as_ref()? {
			ActiveFrozenOverlayEdit::Arrow { start, current, style } => {
				Some(FrozenOverlayEditArrow { start: *start, end: *current, style: *style })
			},
			ActiveFrozenOverlayEdit::Pen { .. }
			| ActiveFrozenOverlayEdit::Mosaic { .. }
			| ActiveFrozenOverlayEdit::MosaicMove { .. }
			| ActiveFrozenOverlayEdit::TextMove { .. }
			| ActiveFrozenOverlayEdit::Spotlight { .. } => None,
		}
	}

	fn preview_mosaic(&self) -> Option<FrozenOverlayEditMosaic> {
		match self.active_interaction.as_ref()? {
			ActiveFrozenOverlayEdit::Mosaic { anchor, current } => {
				Some(FrozenOverlayEditMosaic { rect: normalized_rect(*anchor, *current) })
			},
			ActiveFrozenOverlayEdit::MosaicMove { current_rect, .. } => {
				Some(FrozenOverlayEditMosaic { rect: *current_rect })
			},
			ActiveFrozenOverlayEdit::Pen { .. }
			| ActiveFrozenOverlayEdit::Arrow { .. }
			| ActiveFrozenOverlayEdit::TextMove { .. }
			| ActiveFrozenOverlayEdit::Spotlight { .. } => None,
		}
	}

	fn preview_spotlight(&self) -> Option<FrozenOverlayEditSpotlight> {
		match self.active_interaction.as_ref()? {
			ActiveFrozenOverlayEdit::Spotlight { anchor, current, style } => {
				Some(FrozenOverlayEditSpotlight {
					rect: normalized_rect(*anchor, *current),
					style: *style,
				})
			},
			ActiveFrozenOverlayEdit::Pen { .. }
			| ActiveFrozenOverlayEdit::Arrow { .. }
			| ActiveFrozenOverlayEdit::Mosaic { .. }
			| ActiveFrozenOverlayEdit::MosaicMove { .. }
			| ActiveFrozenOverlayEdit::TextMove { .. } => None,
		}
	}

	fn preview_text(&self) -> Option<FrozenOverlayEditText> {
		match self.active_interaction.as_ref()? {
			ActiveFrozenOverlayEdit::TextMove { current_annotation, .. } => {
				Some(current_annotation.clone())
			},
			ActiveFrozenOverlayEdit::Pen { .. }
			| ActiveFrozenOverlayEdit::Arrow { .. }
			| ActiveFrozenOverlayEdit::Mosaic { .. }
			| ActiveFrozenOverlayEdit::MosaicMove { .. }
			| ActiveFrozenOverlayEdit::Spotlight { .. } => None,
		}
	}
}

fn normalized_rect(
	anchor: FrozenOverlayEditPoint,
	current: FrozenOverlayEditPoint,
) -> FrozenOverlayEditRect {
	FrozenOverlayEditRect::new(
		anchor.x.min(current.x),
		anchor.y.min(current.y),
		(current.x - anchor.x).abs(),
		(current.y - anchor.y).abs(),
	)
}

fn moved_rect(
	rect: FrozenOverlayEditRect,
	drag_offset: FrozenOverlayEditPoint,
	point: FrozenOverlayEditPoint,
	selection: FrozenOverlayEditRect,
) -> FrozenOverlayEditRect {
	let max_min_x = selection.x.max(selection.max_x() - rect.width);
	let max_min_y = selection.y.max(selection.max_y() - rect.height);

	FrozenOverlayEditRect::new(
		(point.x - drag_offset.x).clamp(selection.x, max_min_x),
		(point.y - drag_offset.y).clamp(selection.y, max_min_y),
		rect.width,
		rect.height,
	)
}

fn moved_text_annotation(
	annotation: FrozenOverlayEditText,
	drag_offset: FrozenOverlayEditPoint,
	point: FrozenOverlayEditPoint,
	selection: FrozenOverlayEditRect,
) -> FrozenOverlayEditText {
	let bounds = text_bounds(&annotation);
	let max_anchor_x = selection.x.max(selection.max_x() - bounds.width);
	let max_anchor_y = selection.y.max(selection.max_y() - bounds.height);
	let anchor = FrozenOverlayEditPoint::new(
		(point.x - drag_offset.x).clamp(selection.x, max_anchor_x),
		(point.y - drag_offset.y).clamp(selection.y, max_anchor_y),
	);

	FrozenOverlayEditText { anchor, ..annotation }
}

fn text_hit_bounds(annotation: &FrozenOverlayEditText) -> FrozenOverlayEditRect {
	text_bounds(annotation).inset(-TEXT_HIT_PADDING_POINTS, -TEXT_HIT_PADDING_POINTS)
}

fn text_bounds(annotation: &FrozenOverlayEditText) -> FrozenOverlayEditRect {
	let font_size = annotation.style.font_size_points.max(1.0) as f32;
	let bounds =
		text_rendering::measure_text_bounds(&annotation.text, font_size).unwrap_or_else(|| {
			let width = annotation.text.chars().count().max(1) as f32 * font_size * 0.6;

			TextBounds { width, height: font_size * 1.2 }
		});

	FrozenOverlayEditRect::new(
		annotation.anchor.x,
		annotation.anchor.y,
		f64::from(bounds.width.ceil().max(1.0)),
		f64::from(bounds.height.ceil().max(1.0)),
	)
}

#[cfg(test)]
mod tests {
	use crate::frozen_edit::{
		FrozenOverlayEditColor, FrozenOverlayEditElement, FrozenOverlayEditPoint,
		FrozenOverlayEditRect, FrozenOverlayEditSession, FrozenOverlayEditStyle,
		FrozenOverlayEditTextStyle, FrozenOverlayTextEdit,
	};
	use rsnap_capture_core::ToolbarItemKind;

	fn selection() -> FrozenOverlayEditRect {
		FrozenOverlayEditRect::new(10.0, 20.0, 400.0, 240.0)
	}

	#[test]
	fn pen_lifecycle_exports_visible_stroke() {
		let style = FrozenOverlayEditStyle::default();
		let mut session = FrozenOverlayEditSession::default();

		assert!(session.begin(
			ToolbarItemKind::Pen,
			FrozenOverlayEditPoint::new(20.0, 30.0),
			selection(),
			style,
		));
		assert!(session.update(FrozenOverlayEditPoint::new(40.0, 50.0), selection()));
		assert!(session.finish(selection()));

		let snapshot = session.snapshot();

		assert!(snapshot.can_undo);
		assert_eq!(snapshot.elements.len(), 1);
		assert!(matches!(snapshot.elements[0], FrozenOverlayEditElement::Pen(_)));
	}

	#[test]
	fn undo_and_redo_move_committed_elements_between_stacks() {
		let style = FrozenOverlayEditStyle::default();
		let mut session = FrozenOverlayEditSession::default();

		assert!(session.begin(
			ToolbarItemKind::Mosaic,
			FrozenOverlayEditPoint::new(20.0, 30.0),
			selection(),
			style,
		));
		assert!(session.update(FrozenOverlayEditPoint::new(80.0, 100.0), selection()));
		assert!(session.finish(selection()));
		assert!(session.undo());
		assert!(session.snapshot().can_redo);
		assert!(session.snapshot().elements.is_empty());
		assert!(session.redo());
		assert_eq!(session.snapshot().elements.len(), 1);
	}

	#[test]
	fn mosaic_move_hides_original_while_preview_is_active() {
		let style = FrozenOverlayEditStyle::default();
		let mut session = FrozenOverlayEditSession::default();

		assert!(session.begin(
			ToolbarItemKind::Mosaic,
			FrozenOverlayEditPoint::new(20.0, 30.0),
			selection(),
			style,
		));
		assert!(session.update(FrozenOverlayEditPoint::new(80.0, 100.0), selection()));
		assert!(session.finish(selection()));
		assert!(session.begin(
			ToolbarItemKind::Pointer,
			FrozenOverlayEditPoint::new(30.0, 40.0),
			selection(),
			style,
		));

		let snapshot = session.snapshot();

		assert!(snapshot.is_moving_movable_annotation);
		assert!(snapshot.elements.is_empty());
		assert!(snapshot.preview_mosaic.is_some());
	}

	#[test]
	fn text_lifecycle_commits_and_trims_empty_edits() {
		let style = FrozenOverlayEditStyle::default();
		let mut session = FrozenOverlayEditSession::default();

		assert!(session.begin(
			ToolbarItemKind::Text,
			FrozenOverlayEditPoint::new(20.0, 30.0),
			selection(),
			style,
		));
		assert!(session.append_text("  "));
		assert!(!session.commit_text_edit(FrozenOverlayEditTextStyle::default()));
		assert!(session.begin(
			ToolbarItemKind::Text,
			FrozenOverlayEditPoint::new(30.0, 40.0),
			selection(),
			style,
		));
		assert!(session.append_text("Hello"));
		assert!(session.backspace_text());
		assert!(session.append_text("o"));
		assert!(session.commit_text_edit(FrozenOverlayEditTextStyle {
			color: FrozenOverlayEditColor::White,
			..FrozenOverlayEditTextStyle::default()
		}));
		assert_eq!(session.snapshot().active_text_edit, None);
		assert!(matches!(session.snapshot().elements[0], FrozenOverlayEditElement::Text(_)));
	}

	#[test]
	fn snapshot_reports_active_text_edit() {
		let mut session = FrozenOverlayEditSession::default();

		assert!(session.begin(
			ToolbarItemKind::Text,
			FrozenOverlayEditPoint::new(20.0, 30.0),
			selection(),
			FrozenOverlayEditStyle::default(),
		));
		assert!(session.append_text("Text"));
		assert_eq!(
			session.snapshot().active_text_edit,
			Some(FrozenOverlayTextEdit {
				anchor: FrozenOverlayEditPoint::new(20.0, 30.0),
				text: String::from("Text"),
			})
		);
	}
}
