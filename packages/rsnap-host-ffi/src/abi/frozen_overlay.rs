use std::os::raw::c_char;
use std::ptr;

use crate::abi::{RsnapFloatPoint, RsnapFloatRect};

/// FFI-safe frozen annotation color discriminator.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapFrozenAnnotationColor {
	/// White annotation color.
	White = 0,
	/// Yellow annotation color.
	Yellow = 1,
	/// Green annotation color.
	Green = 2,
	/// Blue annotation color.
	Blue = 3,
	/// Red annotation color.
	Red = 4,
	/// Black annotation color.
	Black = 5,
}

/// FFI-safe frozen-overlay export element discriminator.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapFrozenOverlayExportElementKind {
	/// Pen stroke annotation.
	Pen = 0,
	/// Arrow annotation.
	Arrow = 1,
	/// Mosaic privacy rectangle.
	Mosaic = 2,
	/// Spotlight annotation.
	Spotlight = 3,
	/// Text annotation.
	Text = 4,
}

/// FFI-safe frozen-overlay export element.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RsnapFrozenOverlayExportElement {
	/// Element kind.
	pub kind: RsnapFrozenOverlayExportElementKind,
	/// Rectangle payload for mosaic or spotlight annotations.
	pub rect: RsnapFloatRect,
	/// Start point, arrow tail, or text anchor.
	pub start: RsnapFloatPoint,
	/// Arrow tip.
	pub end: RsnapFloatPoint,
	/// Optional point buffer for pen strokes.
	pub points: *const RsnapFloatPoint,
	/// Number of points in `points`.
	pub points_len: usize,
	/// Optional null-terminated UTF-8 text payload.
	pub text: *const c_char,
	/// Stroke width in points for pen and arrow annotations.
	pub stroke_width_points: f64,
	/// Border width in points for spotlight annotations.
	pub border_width_points: f64,
	/// Font size in points for text annotations.
	pub font_size_points: f64,
	/// Annotation color.
	pub color: RsnapFrozenAnnotationColor,
}

/// FFI-safe frozen-overlay edit style payload.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RsnapFrozenOverlayEditStyle {
	/// Stroke width in points for pen and arrow annotations.
	pub stroke_width_points: f64,
	/// Stroke color for pen and arrow annotations.
	pub stroke_color: RsnapFrozenAnnotationColor,
	/// Border width in points for spotlight annotations.
	pub spotlight_border_width_points: f64,
	/// Border color for spotlight annotations.
	pub spotlight_color: RsnapFrozenAnnotationColor,
	/// Font size in points for text annotations.
	pub text_font_size_points: f64,
	/// Text color.
	pub text_color: RsnapFrozenAnnotationColor,
}
impl Default for RsnapFrozenOverlayEditStyle {
	fn default() -> Self {
		Self {
			stroke_width_points: 3.0,
			stroke_color: RsnapFrozenAnnotationColor::Blue,
			spotlight_border_width_points: 0.0,
			spotlight_color: RsnapFrozenAnnotationColor::Blue,
			text_font_size_points: 16.0,
			text_color: RsnapFrozenAnnotationColor::Blue,
		}
	}
}

/// FFI-safe owned frozen-overlay edit snapshot.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RsnapFrozenOverlayEditSnapshot {
	/// Non-zero when undo is available.
	pub can_undo: u8,
	/// Non-zero when redo is available.
	pub can_redo: u8,
	/// Non-zero when selection transforms should be locked out.
	pub keeps_frozen_selection_fixed: u8,
	/// Non-zero when a movable annotation is being dragged.
	pub is_moving_movable_annotation: u8,
	/// Non-zero when any pointer interaction is active.
	pub has_active_interaction: u8,
	/// Owned visible committed elements.
	pub elements: *mut RsnapFrozenOverlayExportElement,
	/// Number of visible committed elements.
	pub elements_len: usize,
	/// Non-zero when `preview_pen` is present.
	pub has_preview_pen: u8,
	/// Active pen preview.
	pub preview_pen: RsnapFrozenOverlayExportElement,
	/// Non-zero when `preview_arrow` is present.
	pub has_preview_arrow: u8,
	/// Active arrow preview.
	pub preview_arrow: RsnapFrozenOverlayExportElement,
	/// Non-zero when `preview_mosaic` is present.
	pub has_preview_mosaic: u8,
	/// Active mosaic preview.
	pub preview_mosaic: RsnapFrozenOverlayExportElement,
	/// Non-zero when `preview_spotlight` is present.
	pub has_preview_spotlight: u8,
	/// Active spotlight preview.
	pub preview_spotlight: RsnapFrozenOverlayExportElement,
	/// Non-zero when `preview_text` is present.
	pub has_preview_text: u8,
	/// Active moved text preview.
	pub preview_text: RsnapFrozenOverlayExportElement,
	/// Non-zero when `active_text_edit` is present.
	pub has_active_text_edit: u8,
	/// Active text edit payload.
	pub active_text_edit: RsnapFrozenOverlayExportElement,
}
impl Default for RsnapFrozenOverlayEditSnapshot {
	fn default() -> Self {
		Self {
			can_undo: 0,
			can_redo: 0,
			keeps_frozen_selection_fixed: 0,
			is_moving_movable_annotation: 0,
			has_active_interaction: 0,
			elements: ptr::null_mut(),
			elements_len: 0,
			has_preview_pen: 0,
			preview_pen: frozen_overlay_empty_element(),
			has_preview_arrow: 0,
			preview_arrow: frozen_overlay_empty_element(),
			has_preview_mosaic: 0,
			preview_mosaic: frozen_overlay_empty_element(),
			has_preview_spotlight: 0,
			preview_spotlight: frozen_overlay_empty_element(),
			has_preview_text: 0,
			preview_text: frozen_overlay_empty_element(),
			has_active_text_edit: 0,
			active_text_edit: frozen_overlay_empty_element(),
		}
	}
}

/// FFI-safe frozen selection transform discriminator.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapFrozenSelectionTransformKind {
	/// Move the whole selection rectangle.
	Move = 0,
	/// Resize the left edge.
	ResizeLeft = 1,
	/// Resize the right edge.
	ResizeRight = 2,
	/// Resize the top edge.
	ResizeTop = 3,
	/// Resize the bottom edge.
	ResizeBottom = 4,
	/// Resize the top-left corner.
	ResizeTopLeft = 5,
	/// Resize the top-right corner.
	ResizeTopRight = 6,
	/// Resize the bottom-left corner.
	ResizeBottomLeft = 7,
	/// Resize the bottom-right corner.
	ResizeBottomRight = 8,
}

pub(crate) fn frozen_overlay_empty_element() -> RsnapFrozenOverlayExportElement {
	RsnapFrozenOverlayExportElement {
		kind: RsnapFrozenOverlayExportElementKind::Mosaic,
		rect: RsnapFloatRect::default(),
		start: RsnapFloatPoint::default(),
		end: RsnapFloatPoint::default(),
		points: ptr::null(),
		points_len: 0,
		text: ptr::null(),
		stroke_width_points: 0.0,
		border_width_points: 0.0,
		font_size_points: 0.0,
		color: RsnapFrozenAnnotationColor::Blue,
	}
}
