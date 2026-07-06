//! Frozen-overlay edit C ABI entrypoints.

use std::ffi::{CStr, CString};
use std::mem;
use std::os::raw::c_char;
use std::ptr::{self, NonNull};

use crate::abi;
use crate::{
	RsnapFloatPoint, RsnapFloatRect, RsnapFrozenAnnotationColor,
	RsnapFrozenOverlayEditSessionHandle, RsnapFrozenOverlayEditSnapshot,
	RsnapFrozenOverlayEditStyle, RsnapFrozenOverlayExportElement,
	RsnapFrozenOverlayExportElementKind, RsnapStatus, RsnapToolbarItemKind,
};
use rsnap_capture_core::{
	FrozenOverlayEditArrow, FrozenOverlayEditColor, FrozenOverlayEditElement,
	FrozenOverlayEditMosaic, FrozenOverlayEditPen, FrozenOverlayEditPoint, FrozenOverlayEditRect,
	FrozenOverlayEditSession, FrozenOverlayEditSnapshot, FrozenOverlayEditSpotlight,
	FrozenOverlayEditSpotlightStyle, FrozenOverlayEditStrokeStyle, FrozenOverlayEditStyle,
	FrozenOverlayEditText, FrozenOverlayEditTextStyle, FrozenOverlayTextEdit,
};

/// Creates a Rust-owned frozen-overlay edit session.
///
/// The returned pointer must be released by calling
/// `rsnap_frozen_overlay_edit_session_destroy`.
#[unsafe(no_mangle)]
pub extern "C" fn rsnap_frozen_overlay_edit_session_create()
-> *mut RsnapFrozenOverlayEditSessionHandle {
	Box::into_raw(Box::new(RsnapFrozenOverlayEditSessionHandle {
		session: FrozenOverlayEditSession::default(),
	}))
}

/// Destroys a frozen-overlay edit session.
///
/// # Safety
///
/// The pointer must either be null or a pointer returned by
/// `rsnap_frozen_overlay_edit_session_create` that has not already been destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_destroy(
	handle: *mut RsnapFrozenOverlayEditSessionHandle,
) {
	if let Some(handle) = NonNull::new(handle) {
		unsafe {
			drop(Box::from_raw(handle.as_ptr()));
		}
	}
}

/// Resets a frozen-overlay edit session.
///
/// # Safety
///
/// `handle` must be null or a valid pointer returned by
/// `rsnap_frozen_overlay_edit_session_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_reset(
	handle: *mut RsnapFrozenOverlayEditSessionHandle,
) -> RsnapStatus {
	let Some(handle) = (unsafe { frozen_edit_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	handle.session.reset();

	RsnapStatus::Ok
}

/// Starts a Rust-owned frozen-overlay interaction.
///
/// # Safety
///
/// `handle` must be a valid frozen-overlay edit handle and `out_changed` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_begin(
	handle: *mut RsnapFrozenOverlayEditSessionHandle,
	tool: RsnapToolbarItemKind,
	point: RsnapFloatPoint,
	selection: RsnapFloatRect,
	style: RsnapFrozenOverlayEditStyle,
	out_changed: *mut u8,
) -> RsnapStatus {
	let Some(handle) = (unsafe { frozen_edit_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};
	let Some(out_changed) = (unsafe { out_changed.as_mut() }) else {
		return RsnapStatus::NullOutput;
	};
	let Some(point) = decode_frozen_edit_point(point) else {
		return RsnapStatus::InvalidInput;
	};
	let Some(selection) = decode_frozen_edit_rect(selection) else {
		return RsnapStatus::InvalidInput;
	};
	let Some(style) = decode_frozen_overlay_edit_style(style) else {
		return RsnapStatus::InvalidInput;
	};

	*out_changed = u8::from(handle.session.begin(
		crate::decode_toolbar_item_kind(tool as u32),
		point,
		selection,
		style,
	));

	RsnapStatus::Ok
}

/// Updates a Rust-owned frozen-overlay interaction.
///
/// # Safety
///
/// `handle` must be a valid frozen-overlay edit handle and `out_changed` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_update(
	handle: *mut RsnapFrozenOverlayEditSessionHandle,
	point: RsnapFloatPoint,
	selection: RsnapFloatRect,
	out_changed: *mut u8,
) -> RsnapStatus {
	let Some(handle) = (unsafe { frozen_edit_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};
	let Some(out_changed) = (unsafe { out_changed.as_mut() }) else {
		return RsnapStatus::NullOutput;
	};
	let Some(point) = decode_frozen_edit_point(point) else {
		return RsnapStatus::InvalidInput;
	};
	let Some(selection) = decode_frozen_edit_rect(selection) else {
		return RsnapStatus::InvalidInput;
	};

	*out_changed = u8::from(handle.session.update(point, selection));

	RsnapStatus::Ok
}

/// Finishes a Rust-owned frozen-overlay interaction.
///
/// # Safety
///
/// `handle` must be a valid frozen-overlay edit handle and `out_changed` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_finish(
	handle: *mut RsnapFrozenOverlayEditSessionHandle,
	selection: RsnapFloatRect,
	out_changed: *mut u8,
) -> RsnapStatus {
	let Some(handle) = (unsafe { frozen_edit_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};
	let Some(out_changed) = (unsafe { out_changed.as_mut() }) else {
		return RsnapStatus::NullOutput;
	};
	let Some(selection) = decode_frozen_edit_rect(selection) else {
		return RsnapStatus::InvalidInput;
	};

	*out_changed = u8::from(handle.session.finish(selection));

	RsnapStatus::Ok
}

/// Appends UTF-8 text to the active frozen text edit.
///
/// # Safety
///
/// `handle` must be a valid frozen-overlay edit handle, `text` must point to a valid
/// null-terminated UTF-8 string, and `out_changed` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_append_text(
	handle: *mut RsnapFrozenOverlayEditSessionHandle,
	text: *const c_char,
	out_changed: *mut u8,
) -> RsnapStatus {
	let Some(handle) = (unsafe { frozen_edit_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};
	let Some(out_changed) = (unsafe { out_changed.as_mut() }) else {
		return RsnapStatus::NullOutput;
	};

	if text.is_null() {
		return RsnapStatus::InvalidInput;
	}

	let Ok(text) = (unsafe { CStr::from_ptr(text) }).to_str() else {
		return RsnapStatus::InvalidInput;
	};

	*out_changed = u8::from(handle.session.append_text(text));

	RsnapStatus::Ok
}

/// Deletes one scalar from the active frozen text edit.
///
/// # Safety
///
/// `handle` must be a valid frozen-overlay edit handle and `out_changed` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_backspace_text(
	handle: *mut RsnapFrozenOverlayEditSessionHandle,
	out_changed: *mut u8,
) -> RsnapStatus {
	let Some(handle) = (unsafe { frozen_edit_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};
	let Some(out_changed) = (unsafe { out_changed.as_mut() }) else {
		return RsnapStatus::NullOutput;
	};

	*out_changed = u8::from(handle.session.backspace_text());

	RsnapStatus::Ok
}

/// Commits the active frozen text edit.
///
/// # Safety
///
/// `handle` must be a valid frozen-overlay edit handle and `out_changed` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_commit_text(
	handle: *mut RsnapFrozenOverlayEditSessionHandle,
	style: RsnapFrozenOverlayEditStyle,
	out_changed: *mut u8,
) -> RsnapStatus {
	let Some(handle) = (unsafe { frozen_edit_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};
	let Some(out_changed) = (unsafe { out_changed.as_mut() }) else {
		return RsnapStatus::NullOutput;
	};
	let Some(style) = decode_frozen_overlay_edit_style(style) else {
		return RsnapStatus::InvalidInput;
	};

	*out_changed = u8::from(handle.session.commit_text_edit(style.text));

	RsnapStatus::Ok
}

/// Cancels the active frozen text edit.
///
/// # Safety
///
/// `handle` must be null or a valid frozen-overlay edit handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_cancel_text(
	handle: *mut RsnapFrozenOverlayEditSessionHandle,
) -> RsnapStatus {
	let Some(handle) = (unsafe { frozen_edit_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	handle.session.cancel_text_edit();

	RsnapStatus::Ok
}

/// Undoes the latest frozen-overlay edit.
///
/// # Safety
///
/// `handle` must be a valid frozen-overlay edit handle and `out_changed` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_undo(
	handle: *mut RsnapFrozenOverlayEditSessionHandle,
	out_changed: *mut u8,
) -> RsnapStatus {
	let Some(handle) = (unsafe { frozen_edit_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};
	let Some(out_changed) = (unsafe { out_changed.as_mut() }) else {
		return RsnapStatus::NullOutput;
	};

	*out_changed = u8::from(handle.session.undo());

	RsnapStatus::Ok
}

/// Redoes the latest frozen-overlay edit.
///
/// # Safety
///
/// `handle` must be a valid frozen-overlay edit handle and `out_changed` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_redo(
	handle: *mut RsnapFrozenOverlayEditSessionHandle,
	out_changed: *mut u8,
) -> RsnapStatus {
	let Some(handle) = (unsafe { frozen_edit_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};
	let Some(out_changed) = (unsafe { out_changed.as_mut() }) else {
		return RsnapStatus::NullOutput;
	};

	*out_changed = u8::from(handle.session.redo());

	RsnapStatus::Ok
}

/// Tests whether a movable frozen-overlay annotation is under a point.
///
/// # Safety
///
/// `handle` must be a valid frozen-overlay edit handle and `out_contains` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_contains_movable_annotation(
	handle: *const RsnapFrozenOverlayEditSessionHandle,
	point: RsnapFloatPoint,
	out_contains: *mut u8,
) -> RsnapStatus {
	let Some(handle) = (unsafe { frozen_edit_handle_ref(handle) }) else {
		return RsnapStatus::NullHandle;
	};
	let Some(out_contains) = (unsafe { out_contains.as_mut() }) else {
		return RsnapStatus::NullOutput;
	};
	let Some(point) = decode_frozen_edit_point(point) else {
		return RsnapStatus::InvalidInput;
	};

	*out_contains = u8::from(handle.session.contains_movable_annotation(point));

	RsnapStatus::Ok
}

/// Copies an owned frozen-overlay edit snapshot for native-host rendering.
///
/// # Safety
///
/// `handle` must be a valid frozen-overlay edit handle and `out_snapshot` must be writable.
/// Release a successful snapshot with `rsnap_frozen_overlay_edit_snapshot_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_session_copy_snapshot(
	handle: *const RsnapFrozenOverlayEditSessionHandle,
	out_snapshot: *mut RsnapFrozenOverlayEditSnapshot,
) -> RsnapStatus {
	let Some(handle) = (unsafe { frozen_edit_handle_ref(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_snapshot.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(snapshot) = encode_frozen_overlay_edit_snapshot(handle.session.snapshot()) else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_snapshot, snapshot);
	}

	RsnapStatus::Ok
}

/// Releases an owned frozen-overlay edit snapshot.
///
/// # Safety
///
/// `snapshot` must point to a snapshot returned by
/// `rsnap_frozen_overlay_edit_session_copy_snapshot`, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_edit_snapshot_release(
	snapshot: *mut RsnapFrozenOverlayEditSnapshot,
) {
	let Some(snapshot) = (unsafe { snapshot.as_mut() }) else {
		return;
	};

	unsafe {
		release_frozen_overlay_snapshot_elements(snapshot.elements, snapshot.elements_len);
		release_frozen_overlay_snapshot_element(&mut snapshot.preview_pen);
		release_frozen_overlay_snapshot_element(&mut snapshot.preview_arrow);
		release_frozen_overlay_snapshot_element(&mut snapshot.preview_mosaic);
		release_frozen_overlay_snapshot_element(&mut snapshot.preview_spotlight);
		release_frozen_overlay_snapshot_element(&mut snapshot.preview_text);
		release_frozen_overlay_snapshot_element(&mut snapshot.active_text_edit);
	}

	*snapshot = RsnapFrozenOverlayEditSnapshot::default();
}

unsafe fn frozen_edit_handle_mut<'a>(
	handle: *mut RsnapFrozenOverlayEditSessionHandle,
) -> Option<&'a mut RsnapFrozenOverlayEditSessionHandle> {
	unsafe { handle.as_mut() }
}

unsafe fn frozen_edit_handle_ref<'a>(
	handle: *const RsnapFrozenOverlayEditSessionHandle,
) -> Option<&'a RsnapFrozenOverlayEditSessionHandle> {
	unsafe { handle.as_ref() }
}

fn decode_frozen_overlay_edit_style(
	style: RsnapFrozenOverlayEditStyle,
) -> Option<FrozenOverlayEditStyle> {
	Some(FrozenOverlayEditStyle {
		stroke: FrozenOverlayEditStrokeStyle {
			stroke_width_points: finite_nonnegative(style.stroke_width_points)?,
			color: decode_frozen_edit_color(style.stroke_color),
		},
		spotlight: FrozenOverlayEditSpotlightStyle {
			border_width_points: finite_nonnegative(style.spotlight_border_width_points)?,
			border_color: decode_frozen_edit_color(style.spotlight_color),
		},
		text: FrozenOverlayEditTextStyle {
			font_size_points: finite_positive(style.text_font_size_points)?,
			color: decode_frozen_edit_color(style.text_color),
		},
	})
}

fn decode_frozen_edit_color(color: RsnapFrozenAnnotationColor) -> FrozenOverlayEditColor {
	match color {
		RsnapFrozenAnnotationColor::White => FrozenOverlayEditColor::White,
		RsnapFrozenAnnotationColor::Yellow => FrozenOverlayEditColor::Yellow,
		RsnapFrozenAnnotationColor::Green => FrozenOverlayEditColor::Green,
		RsnapFrozenAnnotationColor::Blue => FrozenOverlayEditColor::Blue,
		RsnapFrozenAnnotationColor::Red => FrozenOverlayEditColor::Red,
		RsnapFrozenAnnotationColor::Black => FrozenOverlayEditColor::Black,
	}
}

fn encode_frozen_edit_color(color: FrozenOverlayEditColor) -> RsnapFrozenAnnotationColor {
	match color {
		FrozenOverlayEditColor::White => RsnapFrozenAnnotationColor::White,
		FrozenOverlayEditColor::Yellow => RsnapFrozenAnnotationColor::Yellow,
		FrozenOverlayEditColor::Green => RsnapFrozenAnnotationColor::Green,
		FrozenOverlayEditColor::Blue => RsnapFrozenAnnotationColor::Blue,
		FrozenOverlayEditColor::Red => RsnapFrozenAnnotationColor::Red,
		FrozenOverlayEditColor::Black => RsnapFrozenAnnotationColor::Black,
	}
}

fn decode_frozen_edit_point(point: RsnapFloatPoint) -> Option<FrozenOverlayEditPoint> {
	(point.x.is_finite() && point.y.is_finite())
		.then_some(FrozenOverlayEditPoint { x: point.x, y: point.y })
}

fn decode_frozen_edit_rect(rect: RsnapFloatRect) -> Option<FrozenOverlayEditRect> {
	let rect = FrozenOverlayEditRect::new(rect.x, rect.y, rect.width, rect.height);

	rect.is_valid().then_some(rect)
}

fn finite_nonnegative(value: f64) -> Option<f64> {
	(value.is_finite() && value >= 0.0).then_some(value)
}

fn finite_positive(value: f64) -> Option<f64> {
	(value.is_finite() && value > 0.0).then_some(value)
}

fn encode_frozen_overlay_edit_snapshot(
	snapshot: FrozenOverlayEditSnapshot,
) -> Option<RsnapFrozenOverlayEditSnapshot> {
	let mut elements = snapshot
		.elements
		.iter()
		.map(encode_frozen_overlay_edit_element)
		.collect::<Option<Vec<_>>>()?;
	let elements_len = elements.len();
	let elements_ptr = if elements.is_empty() {
		ptr::null_mut()
	} else {
		let ptr = elements.as_mut_ptr();

		mem::forget(elements);

		ptr
	};

	Some(RsnapFrozenOverlayEditSnapshot {
		can_undo: u8::from(snapshot.can_undo),
		can_redo: u8::from(snapshot.can_redo),
		keeps_frozen_selection_fixed: u8::from(snapshot.keeps_frozen_selection_fixed),
		is_moving_movable_annotation: u8::from(snapshot.is_moving_movable_annotation),
		has_active_interaction: u8::from(snapshot.has_active_interaction),
		elements: elements_ptr,
		elements_len,
		has_preview_pen: u8::from(snapshot.preview_pen.is_some()),
		preview_pen: encode_optional_frozen_overlay_edit_pen(snapshot.preview_pen.as_ref())?,
		has_preview_arrow: u8::from(snapshot.preview_arrow.is_some()),
		preview_arrow: snapshot
			.preview_arrow
			.map(encode_frozen_overlay_edit_arrow)
			.unwrap_or_else(abi::frozen_overlay_empty_element),
		has_preview_mosaic: u8::from(snapshot.preview_mosaic.is_some()),
		preview_mosaic: snapshot
			.preview_mosaic
			.map(encode_frozen_overlay_edit_mosaic)
			.unwrap_or_else(abi::frozen_overlay_empty_element),
		has_preview_spotlight: u8::from(snapshot.preview_spotlight.is_some()),
		preview_spotlight: snapshot
			.preview_spotlight
			.map(encode_frozen_overlay_edit_spotlight)
			.unwrap_or_else(abi::frozen_overlay_empty_element),
		has_preview_text: u8::from(snapshot.preview_text.is_some()),
		preview_text: encode_optional_frozen_overlay_edit_text(snapshot.preview_text.as_ref())?,
		has_active_text_edit: u8::from(snapshot.active_text_edit.is_some()),
		active_text_edit: encode_optional_frozen_overlay_active_text_edit(
			snapshot.active_text_edit.as_ref(),
		)?,
	})
}

fn encode_frozen_overlay_edit_element(
	element: &FrozenOverlayEditElement,
) -> Option<RsnapFrozenOverlayExportElement> {
	match element {
		FrozenOverlayEditElement::Pen(annotation) => encode_frozen_overlay_edit_pen(annotation),
		FrozenOverlayEditElement::Arrow(annotation) => {
			Some(encode_frozen_overlay_edit_arrow(*annotation))
		},
		FrozenOverlayEditElement::Mosaic(annotation) => {
			Some(encode_frozen_overlay_edit_mosaic(*annotation))
		},
		FrozenOverlayEditElement::Spotlight(annotation) => {
			Some(encode_frozen_overlay_edit_spotlight(*annotation))
		},
		FrozenOverlayEditElement::Text(annotation) => encode_frozen_overlay_edit_text(annotation),
	}
}

fn encode_frozen_overlay_edit_pen(
	annotation: &FrozenOverlayEditPen,
) -> Option<RsnapFrozenOverlayExportElement> {
	let (points, points_len) = owned_frozen_overlay_points(&annotation.points);

	Some(RsnapFrozenOverlayExportElement {
		kind: RsnapFrozenOverlayExportElementKind::Pen,
		points,
		points_len,
		stroke_width_points: annotation.style.stroke_width_points,
		color: encode_frozen_edit_color(annotation.style.color),
		..abi::frozen_overlay_empty_element()
	})
}

fn encode_optional_frozen_overlay_edit_pen(
	annotation: Option<&FrozenOverlayEditPen>,
) -> Option<RsnapFrozenOverlayExportElement> {
	annotation
		.map_or_else(|| Some(abi::frozen_overlay_empty_element()), encode_frozen_overlay_edit_pen)
}

fn encode_frozen_overlay_edit_arrow(
	annotation: FrozenOverlayEditArrow,
) -> RsnapFrozenOverlayExportElement {
	RsnapFrozenOverlayExportElement {
		kind: RsnapFrozenOverlayExportElementKind::Arrow,
		start: encode_frozen_edit_point(annotation.start),
		end: encode_frozen_edit_point(annotation.end),
		stroke_width_points: annotation.style.stroke_width_points,
		color: encode_frozen_edit_color(annotation.style.color),
		..abi::frozen_overlay_empty_element()
	}
}

fn encode_frozen_overlay_edit_mosaic(
	annotation: FrozenOverlayEditMosaic,
) -> RsnapFrozenOverlayExportElement {
	RsnapFrozenOverlayExportElement {
		kind: RsnapFrozenOverlayExportElementKind::Mosaic,
		rect: encode_frozen_edit_rect(annotation.rect),
		..abi::frozen_overlay_empty_element()
	}
}

fn encode_frozen_overlay_edit_spotlight(
	annotation: FrozenOverlayEditSpotlight,
) -> RsnapFrozenOverlayExportElement {
	RsnapFrozenOverlayExportElement {
		kind: RsnapFrozenOverlayExportElementKind::Spotlight,
		rect: encode_frozen_edit_rect(annotation.rect),
		border_width_points: annotation.style.border_width_points,
		color: encode_frozen_edit_color(annotation.style.border_color),
		..abi::frozen_overlay_empty_element()
	}
}

fn encode_frozen_overlay_edit_text(
	annotation: &FrozenOverlayEditText,
) -> Option<RsnapFrozenOverlayExportElement> {
	let text = owned_c_string(&annotation.text)?;

	Some(RsnapFrozenOverlayExportElement {
		kind: RsnapFrozenOverlayExportElementKind::Text,
		start: encode_frozen_edit_point(annotation.anchor),
		text,
		font_size_points: annotation.style.font_size_points,
		color: encode_frozen_edit_color(annotation.style.color),
		..abi::frozen_overlay_empty_element()
	})
}

fn encode_optional_frozen_overlay_edit_text(
	annotation: Option<&FrozenOverlayEditText>,
) -> Option<RsnapFrozenOverlayExportElement> {
	annotation
		.map_or_else(|| Some(abi::frozen_overlay_empty_element()), encode_frozen_overlay_edit_text)
}

fn encode_frozen_overlay_active_text_edit(
	edit: &FrozenOverlayTextEdit,
) -> Option<RsnapFrozenOverlayExportElement> {
	let text = owned_c_string(&edit.text)?;

	Some(RsnapFrozenOverlayExportElement {
		kind: RsnapFrozenOverlayExportElementKind::Text,
		start: encode_frozen_edit_point(edit.anchor),
		text,
		..abi::frozen_overlay_empty_element()
	})
}

fn encode_optional_frozen_overlay_active_text_edit(
	edit: Option<&FrozenOverlayTextEdit>,
) -> Option<RsnapFrozenOverlayExportElement> {
	edit.map_or_else(
		|| Some(abi::frozen_overlay_empty_element()),
		encode_frozen_overlay_active_text_edit,
	)
}

fn owned_frozen_overlay_points(
	points: &[FrozenOverlayEditPoint],
) -> (*const RsnapFloatPoint, usize) {
	if points.is_empty() {
		return (ptr::null(), 0);
	}

	let mut owned: Vec<_> = points.iter().copied().map(encode_frozen_edit_point).collect();
	let ptr = owned.as_mut_ptr();
	let len = owned.len();

	mem::forget(owned);

	(ptr, len)
}

fn owned_c_string(text: &str) -> Option<*const c_char> {
	CString::new(text).ok().map(|text| text.into_raw() as *const c_char)
}

fn encode_frozen_edit_point(point: FrozenOverlayEditPoint) -> RsnapFloatPoint {
	RsnapFloatPoint { x: point.x, y: point.y }
}

fn encode_frozen_edit_rect(rect: FrozenOverlayEditRect) -> RsnapFloatRect {
	RsnapFloatRect { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
}

unsafe fn release_frozen_overlay_snapshot_elements(
	elements: *mut RsnapFrozenOverlayExportElement,
	elements_len: usize,
) {
	if elements.is_null() || elements_len == 0 {
		return;
	}

	let mut elements = unsafe { Vec::from_raw_parts(elements, elements_len, elements_len) };

	for element in &mut elements {
		unsafe {
			release_frozen_overlay_snapshot_element(element);
		}
	}
}

unsafe fn release_frozen_overlay_snapshot_element(element: &mut RsnapFrozenOverlayExportElement) {
	if !element.points.is_null() && element.points_len > 0 {
		let _ = unsafe {
			Vec::from_raw_parts(
				element.points as *mut RsnapFloatPoint,
				element.points_len,
				element.points_len,
			)
		};
	}
	if !element.text.is_null() {
		let _ = unsafe { CString::from_raw(element.text as *mut c_char) };
	}

	*element = abi::frozen_overlay_empty_element();
}
