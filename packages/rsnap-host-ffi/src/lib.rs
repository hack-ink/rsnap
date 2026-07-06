//! Thin C ABI bridge for the native-host reset.
//!
//! The ABI surface is intentionally small in the first landing slice. It proves the
//! new host/core direction with an opaque session handle, FFI-safe config/event
//! structs, and copy-out scene/request snapshots.

mod abi;
mod capture_frame;
mod frozen_overlay;
mod frozen_overlay_export;
mod scroll_session;
mod session;

pub use self::abi::{
	RSNAP_HOST_FFI_ABI_VERSION, RsnapCaptureFrameBackgroundKind, RsnapCaptureFrameBackgroundPlan,
	RsnapCaptureFrameColorStop, RsnapCaptureFramePlan, RsnapCaptureFrameRenderKind,
	RsnapCaptureFrameShadow, RsnapCaptureFrameSourceKind, RsnapCaptureFrameWallpaperRequest,
	RsnapCursorIntent, RsnapFloatPoint, RsnapFloatRect, RsnapFrozenAnnotationColor,
	RsnapFrozenOverlayEditSessionHandle, RsnapFrozenOverlayEditSnapshot,
	RsnapFrozenOverlayEditStyle, RsnapFrozenOverlayExportElement,
	RsnapFrozenOverlayExportElementKind, RsnapFrozenSelectionTransformKind, RsnapHostEffectKind,
	RsnapHostEvent, RsnapHostEventKind, RsnapHostReport, RsnapHostReportKind, RsnapHostRequestKind,
	RsnapHostRequestValue, RsnapMonitorRect, RsnapOwnedBytes, RsnapOwnedRgbaRegion,
	RsnapPermissionKind, RsnapPixelRect, RsnapPlatformTag, RsnapPoint, RsnapRect, RsnapRgb,
	RsnapSceneKind, RsnapSceneModel, RsnapScrollMinimapPlan, RsnapScrollObserveOutcomeKind,
	RsnapScrollObserveResult, RsnapScrollSessionHandle, RsnapSessionConfig, RsnapSessionHandle,
	RsnapStatus, RsnapToolbarItem, RsnapToolbarItemKind, RsnapWindowRect,
};
pub use self::capture_frame::{
	rsnap_bgra_frame_loupe_patch_rgba, rsnap_bgra_frame_sample_rgb,
	rsnap_capture_frame_aspect_fill_crop_rect, rsnap_capture_frame_background_plan,
	rsnap_capture_frame_plan, rsnap_capture_frame_render_rgba,
	rsnap_capture_frame_wallpaper_png_thumbnail, rsnap_capture_frame_wallpaper_request_plan,
	rsnap_export_rgba_crop_to_png, rsnap_export_rgba_crop_to_png_with_screen_scale,
	rsnap_export_rgba_to_png, rsnap_export_rgba_to_png_with_screen_scale,
	rsnap_frozen_display_crop_rect, rsnap_frozen_mosaic_light_privacy_patch_rgba,
};
pub use self::frozen_overlay::{
	rsnap_frozen_overlay_edit_session_append_text,
	rsnap_frozen_overlay_edit_session_backspace_text, rsnap_frozen_overlay_edit_session_begin,
	rsnap_frozen_overlay_edit_session_cancel_text, rsnap_frozen_overlay_edit_session_commit_text,
	rsnap_frozen_overlay_edit_session_contains_movable_annotation,
	rsnap_frozen_overlay_edit_session_copy_snapshot, rsnap_frozen_overlay_edit_session_create,
	rsnap_frozen_overlay_edit_session_destroy, rsnap_frozen_overlay_edit_session_finish,
	rsnap_frozen_overlay_edit_session_redo, rsnap_frozen_overlay_edit_session_reset,
	rsnap_frozen_overlay_edit_session_undo, rsnap_frozen_overlay_edit_session_update,
	rsnap_frozen_overlay_edit_snapshot_release,
};
pub use self::frozen_overlay_export::rsnap_frozen_overlay_export_render_rgba;
pub use self::scroll_session::{
	rsnap_scroll_session_create, rsnap_scroll_session_destroy,
	rsnap_scroll_session_observe_downward_frame,
	rsnap_scroll_session_observe_downward_frame_with_motion_hint,
	rsnap_scroll_session_take_export_rgba, rsnap_scroll_session_take_preview_rgba,
	rsnap_scroll_session_undo_last_append,
};
pub use self::session::{
	rsnap_session_copy_scene_model, rsnap_session_create, rsnap_session_destroy,
	rsnap_session_enter_live, rsnap_session_handle_host_event, rsnap_session_handle_host_report,
	rsnap_session_take_next_request,
};

use std::mem;
use std::ptr;
use std::slice;

use rsnap_capture_core::{
	self, AutoCenterImageError, DisplayPointRect, FrozenSelectionTransformInput,
	FrozenSelectionTransformKind, RectPoints, ScrollMinimapInput, ScrollMinimapPlan,
	ToolbarItemKind,
};

/// Resolves scroll-capture minimap layout and viewport marker geometry.
///
/// # Safety
///
/// `out_plan` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_minimap_plan(
	selection: RsnapFloatRect,
	export_width: f64,
	export_height: f64,
	bounds: RsnapFloatRect,
	preferred_width: f64,
	minimum_width: f64,
	gap: f64,
	margin: f64,
	image_inset: f64,
	viewport_top_pixels: f64,
	viewport_height_pixels: f64,
	out_plan: *mut RsnapScrollMinimapPlan,
) -> RsnapStatus {
	if out_plan.is_null() {
		return RsnapStatus::NullOutput;
	}

	let input = ScrollMinimapInput {
		selection: decode_float_rect(selection),
		export_width,
		export_height,
		bounds: decode_float_rect(bounds),
		preferred_width,
		minimum_width,
		gap,
		margin,
		image_inset,
		viewport_top_pixels,
		viewport_height_pixels,
	};
	let Some(plan) = rsnap_capture_core::scroll_minimap_plan(input) else {
		return RsnapStatus::Empty;
	};

	unsafe {
		ptr::write(out_plan, encode_scroll_minimap_plan(plan));
	}

	RsnapStatus::Ok
}

/// Hit-tests a pointer against frozen selection transform handles.
///
/// # Safety
///
/// `out_kind` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_selection_transform_hit_test(
	point_x: f64,
	point_y: f64,
	selection: RsnapFloatRect,
	handle_radius: f64,
	edge_tolerance: f64,
	out_kind: *mut RsnapFrozenSelectionTransformKind,
) -> RsnapStatus {
	if out_kind.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(kind) = rsnap_capture_core::frozen_selection_transform_hit_test(
		point_x,
		point_y,
		decode_float_rect(selection),
		handle_radius,
		edge_tolerance,
	) else {
		return RsnapStatus::Empty;
	};

	unsafe {
		ptr::write(out_kind, encode_frozen_selection_transform_kind(kind));
	}

	RsnapStatus::Ok
}

/// Resolves a frozen selection transform rectangle.
///
/// # Safety
///
/// `out_rect` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_selection_transform_rect(
	kind: RsnapFrozenSelectionTransformKind,
	initial_selection: RsnapFloatRect,
	monitor_frame: RsnapFloatRect,
	initial_pointer_x: f64,
	initial_pointer_y: f64,
	point_x: f64,
	point_y: f64,
	minimum_size: f64,
	out_rect: *mut RsnapFloatRect,
) -> RsnapStatus {
	if out_rect.is_null() {
		return RsnapStatus::NullOutput;
	}

	let input = FrozenSelectionTransformInput {
		kind: decode_frozen_selection_transform_kind(kind),
		initial_selection: decode_float_rect(initial_selection),
		monitor_frame: decode_float_rect(monitor_frame),
		initial_pointer_x,
		initial_pointer_y,
		point_x,
		point_y,
		minimum_size,
	};
	let Some(rect) = rsnap_capture_core::frozen_selection_transform_rect(input) else {
		return RsnapStatus::Empty;
	};

	unsafe {
		ptr::write(out_rect, encode_float_rect(rect));
	}

	RsnapStatus::Ok
}

/// Detects salient content bounds for frozen auto-center from row-major RGBA.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_rect` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_auto_center_content_bounds_rgba(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	out_rect: *mut RsnapPixelRect,
) -> RsnapStatus {
	if out_rect.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let bounds =
		match rsnap_capture_core::detect_auto_center_content_bounds_rgba(width, height, bytes) {
			Ok(Some(bounds)) => bounds,
			Ok(None) => return RsnapStatus::Empty,
			Err(
				AutoCenterImageError::InvalidDimensions | AutoCenterImageError::InvalidRgbaLength,
			) => {
				return RsnapStatus::InvalidInput;
			},
		};

	unsafe {
		ptr::write(out_rect, encode_pixel_rect(bounds));
	}

	RsnapStatus::Ok
}

/// Resolves the point shift that balances content margins inside a frozen crop.
#[unsafe(no_mangle)]
pub extern "C" fn rsnap_auto_center_margin_balance_shift_points(
	content_origin_px: f64,
	content_size_px: f64,
	crop_size_px: f64,
	capture_size_points: f64,
) -> f64 {
	rsnap_capture_core::auto_center_margin_balance_shift_points(
		content_origin_px,
		content_size_px,
		crop_size_px,
		capture_size_points,
	)
}

/// Returns the current C ABI version for the native host bridge.
#[unsafe(no_mangle)]
pub extern "C" fn rsnap_host_ffi_abi_version() -> u32 {
	RSNAP_HOST_FFI_ABI_VERSION
}

/// Releases a buffer previously returned by an RGBA export function.
///
/// # Safety
///
/// `region` must point to a struct returned by a `*_take_*_rgba` function that has not already
/// been released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_owned_rgba_region_release(region: *mut RsnapOwnedRgbaRegion) {
	let Some(region) = (unsafe { region.as_mut() }) else {
		return;
	};

	if !region.rgba.is_null() && region.capacity > 0 {
		let _ = unsafe { Vec::from_raw_parts(region.rgba, region.len, region.capacity) };
	}

	*region = RsnapOwnedRgbaRegion::default();
}

/// Releases a byte buffer previously returned by an export function.
///
/// # Safety
///
/// `bytes` must point to a struct returned by a `*_to_png` function that has not already
/// been released, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_owned_bytes_release(bytes: *mut RsnapOwnedBytes) {
	let Some(bytes) = (unsafe { bytes.as_mut() }) else {
		return;
	};

	if !bytes.bytes.is_null() && bytes.capacity > 0 {
		let _ = unsafe { Vec::from_raw_parts(bytes.bytes, bytes.len, bytes.capacity) };
	}

	*bytes = RsnapOwnedBytes::default();
}

pub(crate) unsafe fn rgba_bytes<'a>(rgba: *const u8, rgba_len: usize) -> Option<&'a [u8]> {
	if rgba.is_null() || rgba_len == 0 {
		return None;
	}

	Some(unsafe { slice::from_raw_parts(rgba, rgba_len) })
}

pub(crate) fn decode_float_rect(rect: RsnapFloatRect) -> DisplayPointRect {
	DisplayPointRect::new(rect.x, rect.y, rect.width, rect.height)
}

pub(crate) fn decode_toolbar_item_kind(kind: u32) -> ToolbarItemKind {
	match kind {
		kind if kind == RsnapToolbarItemKind::Pointer as u32 => ToolbarItemKind::Pointer,
		kind if kind == RsnapToolbarItemKind::Pen as u32 => ToolbarItemKind::Pen,
		kind if kind == RsnapToolbarItemKind::Arrow as u32 => ToolbarItemKind::Arrow,
		kind if kind == RsnapToolbarItemKind::Text as u32 => ToolbarItemKind::Text,
		kind if kind == RsnapToolbarItemKind::Mosaic as u32 => ToolbarItemKind::Mosaic,
		kind if kind == RsnapToolbarItemKind::Spotlight as u32 => ToolbarItemKind::Spotlight,
		kind if kind == RsnapToolbarItemKind::Undo as u32 => ToolbarItemKind::Undo,
		kind if kind == RsnapToolbarItemKind::Redo as u32 => ToolbarItemKind::Redo,
		kind if kind == RsnapToolbarItemKind::AutoCenter as u32 => ToolbarItemKind::AutoCenter,
		kind if kind == RsnapToolbarItemKind::Scroll as u32 => ToolbarItemKind::Scroll,
		kind if kind == RsnapToolbarItemKind::Ocr as u32 => ToolbarItemKind::Ocr,
		kind if kind == RsnapToolbarItemKind::Copy as u32 => ToolbarItemKind::Copy,
		kind if kind == RsnapToolbarItemKind::Save as u32 => ToolbarItemKind::Save,
		_ => ToolbarItemKind::Pointer,
	}
}

pub(crate) fn owned_region_from_raw_rgba(
	width: u32,
	height: u32,
	mut rgba: Vec<u8>,
) -> RsnapOwnedRgbaRegion {
	let out = RsnapOwnedRgbaRegion {
		width,
		height,
		len: rgba.len(),
		capacity: rgba.capacity(),
		rgba: rgba.as_mut_ptr(),
	};

	mem::forget(rgba);

	out
}

pub(crate) fn decode_pixel_rect(rect: RsnapPixelRect) -> RectPoints {
	RectPoints::new(rect.x, rect.y, rect.width, rect.height)
}

pub(crate) fn encode_float_rect(rect: DisplayPointRect) -> RsnapFloatRect {
	RsnapFloatRect { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
}

pub(crate) fn encode_pixel_rect(rect: RectPoints) -> RsnapPixelRect {
	RsnapPixelRect { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
}

fn decode_frozen_selection_transform_kind(
	kind: RsnapFrozenSelectionTransformKind,
) -> FrozenSelectionTransformKind {
	match kind {
		RsnapFrozenSelectionTransformKind::Move => FrozenSelectionTransformKind::Move,
		RsnapFrozenSelectionTransformKind::ResizeLeft => FrozenSelectionTransformKind::ResizeLeft,
		RsnapFrozenSelectionTransformKind::ResizeRight => FrozenSelectionTransformKind::ResizeRight,
		RsnapFrozenSelectionTransformKind::ResizeTop => FrozenSelectionTransformKind::ResizeTop,
		RsnapFrozenSelectionTransformKind::ResizeBottom => {
			FrozenSelectionTransformKind::ResizeBottom
		},
		RsnapFrozenSelectionTransformKind::ResizeTopLeft => {
			FrozenSelectionTransformKind::ResizeTopLeft
		},
		RsnapFrozenSelectionTransformKind::ResizeTopRight => {
			FrozenSelectionTransformKind::ResizeTopRight
		},
		RsnapFrozenSelectionTransformKind::ResizeBottomLeft => {
			FrozenSelectionTransformKind::ResizeBottomLeft
		},
		RsnapFrozenSelectionTransformKind::ResizeBottomRight => {
			FrozenSelectionTransformKind::ResizeBottomRight
		},
	}
}

fn encode_frozen_selection_transform_kind(
	kind: FrozenSelectionTransformKind,
) -> RsnapFrozenSelectionTransformKind {
	match kind {
		FrozenSelectionTransformKind::Move => RsnapFrozenSelectionTransformKind::Move,
		FrozenSelectionTransformKind::ResizeLeft => RsnapFrozenSelectionTransformKind::ResizeLeft,
		FrozenSelectionTransformKind::ResizeRight => RsnapFrozenSelectionTransformKind::ResizeRight,
		FrozenSelectionTransformKind::ResizeTop => RsnapFrozenSelectionTransformKind::ResizeTop,
		FrozenSelectionTransformKind::ResizeBottom => {
			RsnapFrozenSelectionTransformKind::ResizeBottom
		},
		FrozenSelectionTransformKind::ResizeTopLeft => {
			RsnapFrozenSelectionTransformKind::ResizeTopLeft
		},
		FrozenSelectionTransformKind::ResizeTopRight => {
			RsnapFrozenSelectionTransformKind::ResizeTopRight
		},
		FrozenSelectionTransformKind::ResizeBottomLeft => {
			RsnapFrozenSelectionTransformKind::ResizeBottomLeft
		},
		FrozenSelectionTransformKind::ResizeBottomRight => {
			RsnapFrozenSelectionTransformKind::ResizeBottomRight
		},
	}
}

fn encode_scroll_minimap_plan(plan: ScrollMinimapPlan) -> RsnapScrollMinimapPlan {
	RsnapScrollMinimapPlan {
		frame: encode_float_rect(plan.frame),
		image_frame: encode_float_rect(plan.image_frame),
		has_viewport_frame: u8::from(plan.viewport_frame.is_some()),
		viewport_frame: plan.viewport_frame.map_or_else(RsnapFloatRect::default, encode_float_rect),
	}
}

#[cfg(test)]
mod tests;
