//! Thin C ABI bridge for the native-host reset.
//!
//! The ABI surface is intentionally small in the first landing slice. It proves the
//! new host/core direction with an opaque session handle, FFI-safe config/event
//! structs, and copy-out scene/request snapshots.

mod abi;

#[cfg(target_os = "macos")]
pub use abi::RsnapLiveSamplerHandle;
pub use abi::{
	RSNAP_HOST_FFI_ABI_VERSION, RsnapCaptureFrameBackgroundKind, RsnapCaptureFrameBackgroundPlan,
	RsnapCaptureFrameColorStop, RsnapCaptureFramePlan, RsnapCaptureFrameRenderKind,
	RsnapCaptureFrameShadow, RsnapCaptureFrameSourceKind, RsnapCaptureFrameWallpaperRequest,
	RsnapCursorIntent, RsnapFloatPoint, RsnapFloatRect, RsnapFrozenAnnotationColor,
	RsnapFrozenOverlayEditSessionHandle, RsnapFrozenOverlayEditSnapshot,
	RsnapFrozenOverlayEditStyle, RsnapFrozenOverlayExportElement,
	RsnapFrozenOverlayExportElementKind, RsnapFrozenSelectionTransformKind, RsnapHostEffectKind,
	RsnapHostEvent, RsnapHostEventKind, RsnapHostReport, RsnapHostReportKind, RsnapHostRequestKind,
	RsnapHostRequestValue, RsnapLiveSample, RsnapMonitorRect, RsnapOwnedBytes,
	RsnapOwnedRgbaRegion, RsnapPermissionKind, RsnapPixelRect, RsnapPlatformTag, RsnapPoint,
	RsnapRect, RsnapRgb, RsnapRgbaRegion, RsnapSceneKind, RsnapSceneModel, RsnapScrollMinimapPlan,
	RsnapScrollObserveOutcomeKind, RsnapScrollObserveResult, RsnapScrollSessionHandle,
	RsnapSessionConfig, RsnapSessionHandle, RsnapStatus, RsnapToolbarItem, RsnapToolbarItemKind,
	RsnapWindowRect,
};

use std::ffi::{CStr, CString};
use std::mem;
use std::os::raw::c_char;
use std::ptr::{self, NonNull};
use std::slice;

#[cfg(not(target_os = "macos"))]
use rsnap_overlay as _;

#[cfg(target_os = "macos")]
use self::abi::RSNAP_LIVE_SAMPLE_PATCH_CAPACITY;
use self::abi::{RSNAP_STATUS_MESSAGE_CAPACITY, RSNAP_TOOLBAR_ITEM_CAPACITY};
use rsnap_capture_core::SceneModel;
use rsnap_capture_core::{
	self, AutoCenterImageError, BgraFrameView, CaptureFrameBackgroundKind,
	CaptureFrameBackgroundPlan, CaptureFrameColorStop, CaptureFramePlan,
	CaptureFrameRenderImageRef, CaptureFrameRenderKind, CaptureFrameShadow, CaptureFrameSourceKind,
	CaptureFrameWallpaperRequest, CaptureMode, CaptureSessionCore, CursorIntent, DisplayPointRect,
	FrozenSelectionTransformInput, FrozenSelectionTransformKind, GlobalRect, HostEffectKind,
	HostEvent, HostReport, HostRequest, PermissionKind, PlatformTag, RectPoints, Rgb,
	RgbaExportImage, ScrollMinimapInput, ScrollMinimapPlan, SessionConfig, ToolbarItemKind,
	ToolbarItemModel, WindowRect,
};
use rsnap_overlay::frozen_edit::{
	FrozenOverlayEditArrow, FrozenOverlayEditColor, FrozenOverlayEditElement,
	FrozenOverlayEditMosaic, FrozenOverlayEditPen, FrozenOverlayEditPoint, FrozenOverlayEditRect,
	FrozenOverlayEditSession, FrozenOverlayEditSnapshot, FrozenOverlayEditSpotlight,
	FrozenOverlayEditSpotlightStyle, FrozenOverlayEditStrokeStyle, FrozenOverlayEditStyle,
	FrozenOverlayEditText, FrozenOverlayEditTextStyle, FrozenOverlayTextEdit,
};
use rsnap_overlay::frozen_export::{
	self, FrozenOverlayExportArrow, FrozenOverlayExportElement, FrozenOverlayExportMosaic,
	FrozenOverlayExportPen, FrozenOverlayExportPoint, FrozenOverlayExportSpotlight,
	FrozenOverlayExportSpotlightStyle, FrozenOverlayExportStrokeStyle, FrozenOverlayExportText,
	FrozenOverlayExportTextStyle,
};
#[cfg(target_os = "macos")]
use rsnap_overlay::host_live_sampling_macos::HostMacLiveSampler;
use rsnap_overlay::scroll_stitching::{
	ScrollStitchImage, ScrollStitchObserveOutcome, ScrollStitchSession,
};

/// Creates a new opaque session handle.
///
/// # Safety
///
/// The returned pointer must be released by calling `rsnap_session_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_session_create(
	config: RsnapSessionConfig,
) -> *mut RsnapSessionHandle {
	let session = CaptureSessionCore::with_config(decode_session_config(config));

	Box::into_raw(Box::new(RsnapSessionHandle { session }))
}

/// Creates a scroll-capture stitcher from the first frozen viewport frame.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data. The returned pointer must be released by calling
/// `rsnap_scroll_session_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_create(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	preview_width_px: u32,
) -> *mut RsnapScrollSessionHandle {
	let Some(bytes) = (unsafe { rgba_bytes(rgba, rgba_len) }) else {
		return ptr::null_mut();
	};
	let Ok(session) = ScrollStitchSession::new_from_rgba(width, height, bytes, preview_width_px)
	else {
		return ptr::null_mut();
	};

	Box::into_raw(Box::new(RsnapScrollSessionHandle { session }))
}

/// Destroys a scroll-capture stitcher returned by `rsnap_scroll_session_create`.
///
/// # Safety
///
/// The pointer must either be null or a pointer returned by
/// `rsnap_scroll_session_create` that has not already been destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_destroy(handle: *mut RsnapScrollSessionHandle) {
	if handle.is_null() {
		return;
	}

	unsafe {
		drop(Box::from_raw(handle));
	}
}

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
		decode_toolbar_item_kind(tool as u32),
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

/// Observes one discrete viewport screenshot for downward scroll-capture stitching.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_scroll_session_create`.
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_result` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_observe_downward_frame(
	handle: *mut RsnapScrollSessionHandle,
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	out_result: *mut RsnapScrollObserveResult,
) -> RsnapStatus {
	let Some(handle) = (unsafe { scroll_session_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_result.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let outcome = match handle.session.observe_worker_pairwise_rgba(width, height, bytes) {
		Ok(outcome) => outcome,
		Err(_err) => return RsnapStatus::InvalidInput,
	};
	let (export_width, export_height) = handle.session.export_dimensions();

	unsafe {
		ptr::write(
			out_result,
			encode_scroll_observe_result(outcome, export_width, export_height, &handle.session),
		);
	}

	RsnapStatus::Ok
}

/// Observes one discrete viewport screenshot with an optional downward motion hint.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_scroll_session_create`.
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_result` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_observe_downward_frame_with_motion_hint(
	handle: *mut RsnapScrollSessionHandle,
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	motion_rows_hint: u32,
	allow_burst_search: u8,
	out_result: *mut RsnapScrollObserveResult,
) -> RsnapStatus {
	let Some(handle) = (unsafe { scroll_session_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_result.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let hint = (motion_rows_hint > 0).then_some(motion_rows_hint);
	let outcome = match handle.session.observe_downward_rgba_with_motion_hint(
		width,
		height,
		bytes,
		hint,
		allow_burst_search != 0,
	) {
		Ok(outcome) => outcome,
		Err(_err) => return RsnapStatus::InvalidInput,
	};
	let (export_width, export_height) = handle.session.export_dimensions();

	unsafe {
		ptr::write(
			out_result,
			encode_scroll_observe_result(outcome, export_width, export_height, &handle.session),
		);
	}

	RsnapStatus::Ok
}

/// Copies the current committed scroll-capture export into a Rust-owned RGBA buffer.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_scroll_session_create`, and
/// `out_region` must be writable. The returned buffer must be released with
/// `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_take_export_rgba(
	handle: *mut RsnapScrollSessionHandle,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { scroll_session_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let export = handle.session.export_image();

	unsafe {
		ptr::write(out_region, owned_region_from_scroll_image(export));
	}

	RsnapStatus::Ok
}

/// Copies the current committed scroll-capture preview into a Rust-owned RGBA buffer.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_scroll_session_create`, and
/// `out_region` must be writable. The returned buffer must be released with
/// `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_take_preview_rgba(
	handle: *mut RsnapScrollSessionHandle,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { scroll_session_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let preview = handle.session.preview_image();

	unsafe {
		ptr::write(out_region, owned_region_from_scroll_image(preview));
	}

	RsnapStatus::Ok
}

/// Reverts the most recent committed scroll-capture append when possible.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_scroll_session_create`, and
/// `out_result` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_undo_last_append(
	handle: *mut RsnapScrollSessionHandle,
	out_result: *mut RsnapScrollObserveResult,
) -> RsnapStatus {
	let Some(handle) = (unsafe { scroll_session_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_result.is_null() {
		return RsnapStatus::NullOutput;
	}

	let did_undo = handle.session.undo_last_append();
	let (export_width, export_height) = handle.session.export_dimensions();
	let kind = if did_undo {
		ScrollStitchObserveOutcome::PreviewUpdated
	} else {
		ScrollStitchObserveOutcome::NoChange
	};

	unsafe {
		ptr::write(
			out_result,
			encode_scroll_observe_result(kind, export_width, export_height, &handle.session),
		);
	}

	RsnapStatus::Ok
}

/// Encodes a full RGBA export image as lossless PNG through the Rust product core.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_png` must be writable. The returned buffer must
/// be released with `rsnap_owned_bytes_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_export_rgba_to_png(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	out_png: *mut RsnapOwnedBytes,
) -> RsnapStatus {
	if out_png.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(image) = RgbaExportImage::from_raw(width, height, bytes.to_vec()) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(png) = image.to_png_bytes() else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_png, owned_bytes_from_vec(png));
	}

	RsnapStatus::Ok
}

/// Encodes a full RGBA export image as lossless PNG with physical-pixel density metadata.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_png` must be writable. The returned buffer must
/// be released with `rsnap_owned_bytes_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_export_rgba_to_png_with_screen_scale(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	scale_factor_x1000: u32,
	out_png: *mut RsnapOwnedBytes,
) -> RsnapStatus {
	if out_png.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(image) = RgbaExportImage::from_raw(width, height, bytes.to_vec()) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(png) = image.to_png_bytes_with_screen_scale(scale_factor_x1000) else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_png, owned_bytes_from_vec(png));
	}

	RsnapStatus::Ok
}

/// Encodes a pixel-space RGBA export crop as lossless PNG through the Rust product core.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_png` must be writable. The returned buffer must
/// be released with `rsnap_owned_bytes_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_export_rgba_crop_to_png(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	crop_rect: RsnapPixelRect,
	out_png: *mut RsnapOwnedBytes,
) -> RsnapStatus {
	if out_png.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(image) = RgbaExportImage::from_raw(width, height, bytes.to_vec()) else {
		return RsnapStatus::InvalidInput;
	};
	let Some(cropped) = image.crop(decode_pixel_rect(crop_rect)) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(png) = cropped.to_png_bytes() else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_png, owned_bytes_from_vec(png));
	}

	RsnapStatus::Ok
}

/// Encodes a pixel-space RGBA crop as lossless PNG with physical-pixel density metadata.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_png` must be writable. The returned buffer must
/// be released with `rsnap_owned_bytes_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_export_rgba_crop_to_png_with_screen_scale(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	crop_rect: RsnapPixelRect,
	scale_factor_x1000: u32,
	out_png: *mut RsnapOwnedBytes,
) -> RsnapStatus {
	if out_png.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(image) = RgbaExportImage::from_raw(width, height, bytes.to_vec()) else {
		return RsnapStatus::InvalidInput;
	};
	let Some(cropped) = image.crop(decode_pixel_rect(crop_rect)) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(png) = cropped.to_png_bytes_with_screen_scale(scale_factor_x1000) else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_png, owned_bytes_from_vec(png));
	}

	RsnapStatus::Ok
}

/// Composites frozen-overlay annotations into a full RGBA export image through Rust.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data. `elements` must either be null with `elements_len == 0`, or point
/// to `elements_len` readable element records whose nested point and text pointers stay
/// valid for the duration of the call. The returned buffer must be released with
/// `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_export_render_rgba(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	selection: RsnapFloatRect,
	elements: *const RsnapFrozenOverlayExportElement,
	elements_len: usize,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let Some(elements) = (unsafe { decode_frozen_overlay_export_elements(elements, elements_len) })
	else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(image) = frozen_export::render_frozen_overlay_export_rgba(
		width,
		height,
		bytes,
		decode_float_rect(selection),
		&elements,
	) else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_region, owned_region_from_raw_rgba(width, height, image.into_raw()));
	}

	RsnapStatus::Ok
}

/// Resolves a frozen display selection into an image-local pixel crop rectangle.
///
/// # Safety
///
/// `out_rect` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_display_crop_rect(
	image_width: u32,
	image_height: u32,
	display_frame: RsnapFloatRect,
	selection: RsnapFloatRect,
	out_rect: *mut RsnapPixelRect,
) -> RsnapStatus {
	if out_rect.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(crop_rect) = rsnap_capture_core::frozen_display_crop_rect(
		image_width,
		image_height,
		decode_float_rect(display_frame),
		decode_float_rect(selection),
	) else {
		return RsnapStatus::Empty;
	};

	unsafe {
		ptr::write(out_rect, encode_pixel_rect(crop_rect));
	}

	RsnapStatus::Ok
}

/// Builds a light privacy mosaic patch as row-major RGBA bytes.
///
/// # Safety
///
/// `out_region` must be writable. The returned buffer must be released with
/// `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_mosaic_light_privacy_patch_rgba(
	image_width: u32,
	image_height: u32,
	source_rect: RsnapFloatRect,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(patch) = rsnap_capture_core::frozen_mosaic_light_privacy_patch(
		image_width,
		image_height,
		decode_float_rect(source_rect),
	) else {
		return RsnapStatus::Empty;
	};
	let (width, height) = patch.dimensions();

	unsafe {
		ptr::write(out_region, owned_region_from_raw_rgba(width, height, patch.into_raw()));
	}

	RsnapStatus::Ok
}

/// Samples an RGB value from a borrowed BGRA frame.
///
/// # Safety
///
/// `bgra` must point to `bgra_len` readable bytes while this function runs, and
/// `out_rgb` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_bgra_frame_sample_rgb(
	width: u32,
	height: u32,
	bytes_per_row: usize,
	bgra: *const u8,
	bgra_len: usize,
	display_frame: RsnapFloatRect,
	point_x: f64,
	point_y: f64,
	out_rgb: *mut RsnapRgb,
) -> RsnapStatus {
	if out_rgb.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(frame) = (unsafe { decode_bgra_frame(width, height, bytes_per_row, bgra, bgra_len) })
	else {
		return RsnapStatus::InvalidInput;
	};
	let Some(rgb) = rsnap_capture_core::sample_rgb_from_bgra_frame(
		frame,
		decode_float_rect(display_frame),
		point_x,
		point_y,
	) else {
		return RsnapStatus::Empty;
	};

	unsafe {
		ptr::write(out_rgb, RsnapRgb { r: rgb.r, g: rgb.g, b: rgb.b });
	}

	RsnapStatus::Ok
}

/// Builds a square RGBA loupe patch from a borrowed BGRA frame.
///
/// # Safety
///
/// `bgra` must point to `bgra_len` readable bytes while this function runs. `out_region`
/// must be writable, and the returned buffer must be released with
/// `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_bgra_frame_loupe_patch_rgba(
	width: u32,
	height: u32,
	bytes_per_row: usize,
	bgra: *const u8,
	bgra_len: usize,
	display_frame: RsnapFloatRect,
	point_x: f64,
	point_y: f64,
	side_pixels: u32,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(frame) = (unsafe { decode_bgra_frame(width, height, bytes_per_row, bgra, bgra_len) })
	else {
		return RsnapStatus::InvalidInput;
	};
	let Some(patch) = rsnap_capture_core::loupe_patch_rgba_from_bgra_frame(
		frame,
		decode_float_rect(display_frame),
		point_x,
		point_y,
		side_pixels,
	) else {
		unsafe {
			ptr::write(out_region, RsnapOwnedRgbaRegion::default());
		}

		return RsnapStatus::Empty;
	};
	let (width, height) = patch.dimensions();

	unsafe {
		ptr::write(out_region, owned_region_from_raw_rgba(width, height, patch.into_raw()));
	}

	RsnapStatus::Ok
}

/// Resolves capture-frame layout and shadow parameters.
///
/// # Safety
///
/// `out_plan` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_capture_frame_plan(
	image_width: u32,
	image_height: u32,
	screen_scale_factor: f64,
	source_kind: RsnapCaptureFrameSourceKind,
	out_plan: *mut RsnapCaptureFramePlan,
) -> RsnapStatus {
	if out_plan.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(plan) = rsnap_capture_core::capture_frame_plan(
		image_width,
		image_height,
		screen_scale_factor,
		decode_capture_frame_source_kind(source_kind),
	) else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_plan, encode_capture_frame_plan(plan));
	}

	RsnapStatus::Ok
}

/// Resolves the source crop rect for aspect-fill drawing.
///
/// # Safety
///
/// `out_rect` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_capture_frame_aspect_fill_crop_rect(
	source_width: u32,
	source_height: u32,
	destination_width: f64,
	destination_height: f64,
	out_rect: *mut RsnapFloatRect,
) -> RsnapStatus {
	if out_rect.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(rect) = rsnap_capture_core::capture_frame_aspect_fill_crop_rect(
		source_width,
		source_height,
		destination_width,
		destination_height,
	) else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_rect, encode_float_rect(rect));
	}

	RsnapStatus::Ok
}

/// Resolves capture-frame background colors and wallpaper fallback behavior.
///
/// # Safety
///
/// `out_plan` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_capture_frame_background_plan(
	background_kind: RsnapCaptureFrameBackgroundKind,
	out_plan: *mut RsnapCaptureFrameBackgroundPlan,
) -> RsnapStatus {
	if out_plan.is_null() {
		return RsnapStatus::NullOutput;
	}

	let plan = rsnap_capture_core::capture_frame_background_plan(
		decode_capture_frame_background_kind(background_kind),
	);

	unsafe {
		ptr::write(out_plan, encode_capture_frame_background_plan(plan));
	}

	RsnapStatus::Ok
}

/// Resolves a platform wallpaper thumbnail request for a capture-frame destination.
///
/// # Safety
///
/// `out_request` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_capture_frame_wallpaper_request_plan(
	background_kind: RsnapCaptureFrameBackgroundKind,
	destination_width: f64,
	destination_height: f64,
	out_request: *mut RsnapCaptureFrameWallpaperRequest,
) -> RsnapStatus {
	if out_request.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(request) = rsnap_capture_core::capture_frame_wallpaper_request_plan(
		decode_capture_frame_background_kind(background_kind),
		destination_width,
		destination_height,
	) else {
		return RsnapStatus::Empty;
	};

	unsafe {
		ptr::write(out_request, encode_capture_frame_wallpaper_request(request));
	}

	RsnapStatus::Ok
}

/// Decodes a PNG wallpaper thumbnail through Rust's streaming low-memory cached path.
///
/// Non-PNG paths and decode failures return `Empty` so native hosts can skip wallpaper drawing.
///
/// # Safety
///
/// `path` must be a valid null-terminated UTF-8 string, and `out_region` must be a valid writable
/// pointer. When `Ok` is returned, the caller must release the returned buffer with
/// `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_capture_frame_wallpaper_png_thumbnail(
	path: *const c_char,
	target_pixel_size: u32,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	unsafe {
		ptr::write(out_region, RsnapOwnedRgbaRegion::default());
	}

	if path.is_null() || target_pixel_size == 0 {
		return RsnapStatus::InvalidInput;
	}

	let Ok(path) = (unsafe { CStr::from_ptr(path) }).to_str() else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(Some(thumbnail)) =
		rsnap_capture_core::capture_frame_wallpaper_png_thumbnail_cached(path, target_pixel_size)
	else {
		return RsnapStatus::Empty;
	};
	let image = thumbnail.into_image();
	let out = owned_region_from_raw_rgba(image.width(), image.height(), image.into_raw());

	unsafe {
		ptr::write(out_region, out);
	}

	RsnapStatus::Ok
}

/// Renders the complete capture-frame effect as Rust-owned RGBA bytes.
///
/// Swift/native hosts only pass source pixels and an optional platform wallpaper path. Rust owns
/// wallpaper thumbnail planning/cache/decode, background drawing, shadows, clipping, and final
/// composition.
///
/// # Safety
///
/// `source_rgba` must point to `source_rgba_len` readable bytes containing
/// `source_width * source_height * 4` row-major RGBA data. `wallpaper_path` may be null or a valid
/// null-terminated UTF-8 string. `out_region` must be writable, and the returned buffer must be
/// released with `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_capture_frame_render_rgba(
	source_width: u32,
	source_height: u32,
	source_rgba: *const u8,
	source_rgba_len: usize,
	screen_scale_factor: f64,
	source_kind: RsnapCaptureFrameSourceKind,
	background_kind: RsnapCaptureFrameBackgroundKind,
	render_kind: RsnapCaptureFrameRenderKind,
	wallpaper_path: *const c_char,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	unsafe {
		ptr::write(out_region, RsnapOwnedRgbaRegion::default());
	}

	let Some(source_bytes) = (unsafe { rgba_bytes(source_rgba, source_rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(source) = CaptureFrameRenderImageRef::new(source_width, source_height, source_bytes)
	else {
		return RsnapStatus::InvalidInput;
	};
	let background_kind = decode_capture_frame_background_kind(background_kind);
	let source_kind = decode_capture_frame_source_kind(source_kind);
	let render_kind = decode_capture_frame_render_kind(render_kind);
	let wallpaper = match unsafe {
		capture_frame_wallpaper_for_render(
			source,
			screen_scale_factor,
			source_kind,
			background_kind,
			wallpaper_path,
		)
	} {
		Ok(wallpaper) => wallpaper,
		Err(_err) => return RsnapStatus::InvalidInput,
	};
	let wallpaper_ref = wallpaper.as_ref().map(CaptureFrameRenderImageRef::from_export);
	let Ok(Some(rendered)) = rsnap_capture_core::render_capture_frame_effect(
		source,
		background_kind,
		screen_scale_factor,
		source_kind,
		render_kind,
		wallpaper_ref,
	) else {
		return RsnapStatus::InvalidInput;
	};
	let image = rendered.into_image();

	unsafe {
		ptr::write(
			out_region,
			owned_region_from_raw_rgba(image.width(), image.height(), image.into_raw()),
		);
	}

	RsnapStatus::Ok
}

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

/// Creates a new opaque live-sampler handle for the native host.
///
/// # Safety
///
/// The returned pointer must be released by calling `rsnap_live_sampler_destroy`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_create() -> *mut RsnapLiveSamplerHandle {
	Box::into_raw(Box::new(RsnapLiveSamplerHandle { sampler: HostMacLiveSampler::new() }))
}

/// Creates a live sampler that keeps selected current-process windows capturable.
///
/// # Safety
///
/// `window_ids` must point to `window_id_count` valid `u32` values, or be null when
/// `window_id_count` is zero. The returned pointer must be released by calling
/// `rsnap_live_sampler_destroy`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_create_with_self_capture_exception_window_ids(
	window_ids: *const u32,
	window_id_count: usize,
) -> *mut RsnapLiveSamplerHandle {
	if window_id_count > 0 && window_ids.is_null() {
		return ptr::null_mut();
	}

	let exception_window_ids = if window_id_count == 0 {
		Vec::new()
	} else {
		unsafe { slice::from_raw_parts(window_ids, window_id_count) }.to_vec()
	};

	Box::into_raw(Box::new(RsnapLiveSamplerHandle {
		sampler: HostMacLiveSampler::with_self_capture_exception_window_ids(exception_window_ids),
	}))
}

/// Starts warming the live sampler for the requested monitor without blocking on the
/// first captured frame.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_prime_monitor(
	handle: *mut RsnapLiveSamplerHandle,
	monitor: RsnapMonitorRect,
) -> RsnapStatus {
	let Some(handle) = (unsafe { live_sampler_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	handle.sampler.prime_monitor(decode_overlay_monitor(monitor));

	RsnapStatus::Ok
}

/// Stops any active ScreenCaptureKit stream while retaining the live-sampler worker.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_reset(
	handle: *mut RsnapLiveSamplerHandle,
) -> RsnapStatus {
	let Some(handle) = (unsafe { live_sampler_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	handle.sampler.reset();

	RsnapStatus::Ok
}

/// Destroys an opaque session handle.
///
/// # Safety
///
/// The pointer must either be null or a pointer returned by `rsnap_session_create` that
/// has not already been destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_session_destroy(handle: *mut RsnapSessionHandle) {
	if let Some(handle) = NonNull::new(handle) {
		unsafe {
			drop(Box::from_raw(handle.as_ptr()));
		}
	}
}

/// Destroys an opaque live-sampler handle.
///
/// # Safety
///
/// The pointer must either be null or a pointer returned by `rsnap_live_sampler_create`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_destroy(handle: *mut RsnapLiveSamplerHandle) {
	if let Some(handle) = NonNull::new(handle) {
		unsafe {
			drop(Box::from_raw(handle.as_ptr()));
		}
	}
}

/// Enters live mode on the referenced session.
///
/// # Safety
///
/// `handle` must be null or a valid pointer returned by `rsnap_session_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_session_enter_live(handle: *mut RsnapSessionHandle) -> RsnapStatus {
	let Some(handle) = (unsafe { handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	handle.session.enter_live();

	RsnapStatus::Ok
}

/// Applies one host event to the referenced session.
///
/// # Safety
///
/// `handle` must be null or a valid pointer returned by `rsnap_session_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_session_handle_host_event(
	handle: *mut RsnapSessionHandle,
	event: RsnapHostEvent,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	handle.session.handle_host_event(decode_host_event(event));

	RsnapStatus::Ok
}

/// Applies one host report to the referenced session.
///
/// # Safety
///
/// `handle` must be null or a valid pointer returned by `rsnap_session_create`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_session_handle_host_report(
	handle: *mut RsnapSessionHandle,
	report: RsnapHostReport,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	handle.session.handle_host_report(decode_host_report(report));

	RsnapStatus::Ok
}

/// Copies the current scene snapshot into the provided output struct.
///
/// # Safety
///
/// `handle` must be null or a valid pointer returned by `rsnap_session_create`. `out_scene`
/// must be non-null and writable for one `RsnapSceneModel` value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_session_copy_scene_model(
	handle: *const RsnapSessionHandle,
	out_scene: *mut RsnapSceneModel,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle_ref(handle) }) else {
		return RsnapStatus::NullHandle;
	};
	let Some(out_scene) = NonNull::new(out_scene) else {
		return RsnapStatus::NullOutput;
	};

	unsafe {
		ptr::write(out_scene.as_ptr(), encode_scene_model(handle.session.scene_model()));
	}

	RsnapStatus::Ok
}

/// Pops the next queued host request into the provided output struct.
///
/// # Safety
///
/// `handle` must be null or a valid pointer returned by `rsnap_session_create`. `out_request`
/// must be non-null and writable for one `RsnapHostRequestValue` value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_session_take_next_request(
	handle: *mut RsnapSessionHandle,
	out_request: *mut RsnapHostRequestValue,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};
	let Some(out_request) = NonNull::new(out_request) else {
		return RsnapStatus::NullOutput;
	};
	let Some(request) = handle.session.pop_host_request() else {
		return RsnapStatus::Empty;
	};

	unsafe {
		ptr::write(out_request.as_ptr(), encode_host_request(request));
	}

	RsnapStatus::Ok
}

/// Samples the current live RGB value and optional loupe patch through the proven
/// Rust ScreenCaptureKit path.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`, and
/// `out_sample` must be a valid writable pointer.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_sample_cursor(
	handle: *mut RsnapLiveSamplerHandle,
	monitor: RsnapMonitorRect,
	point: RsnapPoint,
	patch_width_px: u32,
	patch_height_px: u32,
	out_sample: *mut RsnapLiveSample,
) -> RsnapStatus {
	let Some(handle) = (unsafe { live_sampler_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_sample.is_null() {
		return RsnapStatus::NullOutput;
	}

	let sample = handle.sampler.sample_cursor_with_metadata(
		decode_overlay_monitor(monitor),
		decode_overlay_point(point),
		patch_width_px,
		patch_height_px,
	);
	let Some(sample) = sample else {
		return RsnapStatus::Empty;
	};
	let mut out = RsnapLiveSample {
		has_frame_metadata: 1,
		frame_age_micros: sample.frame_age_micros,
		frame_seq: sample.frame_seq,
		stream_generation: sample.stream_generation,
		..Default::default()
	};

	if let Some(rgb) = sample.sample.rgb {
		out.rgb = RsnapRgb { r: rgb.r, g: rgb.g, b: rgb.b };
		out.has_rgb = 1;
	}
	if let Some(patch) = sample.sample.patch {
		let bytes = patch.as_raw();
		let len = bytes.len().min(RSNAP_LIVE_SAMPLE_PATCH_CAPACITY);

		out.patch_width = patch.width();
		out.patch_height = patch.height();
		out.patch_len = len as u32;

		out.patch_rgba[..len].copy_from_slice(&bytes[..len]);
	}

	unsafe {
		ptr::write(out_sample, out);
	}

	if out.has_rgb == 0 && out.patch_len == 0 {
		return RsnapStatus::Empty;
	}

	RsnapStatus::Ok
}

/// Peeks a cached RGBA region from the latest live sampler monitor frame without waiting
/// for a new capture.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`, and
/// `out_region` must be a valid writable pointer. The caller may first call with a null
/// `rgba` pointer and zero `capacity` to query the required size, then call again with a
/// writable buffer to receive the bytes.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_peek_region_rgba(
	handle: *mut RsnapLiveSamplerHandle,
	monitor: RsnapMonitorRect,
	rect: RsnapRect,
	out_region: *mut RsnapRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { live_sampler_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let region = handle.sampler.peek_region_rgba(
		decode_overlay_monitor(monitor),
		decode_overlay_point(RsnapPoint { x: rect.x, y: rect.y }),
		rect.width,
		rect.height,
	);
	let Some(region) = region else {
		unsafe {
			ptr::write(out_region, RsnapRgbaRegion::default());
		}

		return RsnapStatus::Empty;
	};
	let requested = unsafe { &mut *out_region };
	let len = region.rgba.len();

	if !requested.rgba.is_null() && requested.capacity >= len {
		unsafe {
			ptr::copy_nonoverlapping(region.rgba.as_ptr(), requested.rgba, len);
		}
	}

	unsafe {
		ptr::write(
			out_region,
			RsnapRgbaRegion {
				width: region.width,
				height: region.height,
				len,
				capacity: requested.capacity,
				rgba: requested.rgba,
			},
		);
	}

	RsnapStatus::Ok
}

/// Transfers ownership of a cached RGBA region from the latest live sampler monitor frame
/// to the caller.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`, and
/// `out_region` must be a valid writable pointer. The caller must later release the
/// returned buffer with `rsnap_owned_rgba_region_release`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_take_region_rgba(
	handle: *mut RsnapLiveSamplerHandle,
	monitor: RsnapMonitorRect,
	rect: RsnapRect,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { live_sampler_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let region = handle.sampler.peek_region_rgba(
		decode_overlay_monitor(monitor),
		decode_overlay_point(RsnapPoint { x: rect.x, y: rect.y }),
		rect.width,
		rect.height,
	);
	let Some(region) = region else {
		unsafe {
			ptr::write(out_region, RsnapOwnedRgbaRegion::default());
		}

		return RsnapStatus::Empty;
	};
	let mut rgba = region.rgba;
	let out = RsnapOwnedRgbaRegion {
		width: region.width,
		height: region.height,
		len: rgba.len(),
		capacity: rgba.capacity(),
		rgba: rgba.as_mut_ptr(),
	};

	mem::forget(rgba);

	unsafe {
		ptr::write(out_region, out);
	}

	RsnapStatus::Ok
}

/// Transfers ownership of the oldest queued RGBA region newer than `after_frame_seq`
/// to the caller, preserving live-stream frame order.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`,
/// `out_frame_seq` and `out_frame_age_micros` must be valid writable pointers, and
/// `out_region` must be a valid writable pointer. The caller must later release the
/// returned region buffer with `rsnap_owned_rgba_region_release`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_take_next_region_rgba_after_seq(
	handle: *mut RsnapLiveSamplerHandle,
	monitor: RsnapMonitorRect,
	rect: RsnapRect,
	after_frame_seq: u64,
	wait_for_fresh: u8,
	out_frame_seq: *mut u64,
	out_frame_age_micros: *mut u64,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { live_sampler_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_frame_seq.is_null() || out_frame_age_micros.is_null() || out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(frame) = handle.sampler.next_region_rgba_after_seq(
		decode_overlay_monitor(monitor),
		decode_overlay_point(RsnapPoint { x: rect.x, y: rect.y }),
		rect.width,
		rect.height,
		after_frame_seq,
		wait_for_fresh != 0,
	) else {
		unsafe {
			ptr::write(out_frame_seq, after_frame_seq);
			ptr::write(out_frame_age_micros, 0);
			ptr::write(out_region, RsnapOwnedRgbaRegion::default());
		}

		return RsnapStatus::Empty;
	};
	let mut rgba = frame.region.rgba;
	let out = RsnapOwnedRgbaRegion {
		width: frame.region.width,
		height: frame.region.height,
		len: rgba.len(),
		capacity: rgba.capacity(),
		rgba: rgba.as_mut_ptr(),
	};

	mem::forget(rgba);

	unsafe {
		ptr::write(out_frame_seq, frame.frame_seq);
		ptr::write(out_frame_age_micros, frame.frame_age_micros);
		ptr::write(out_region, out);
	}

	RsnapStatus::Ok
}

/// Transfers ownership of the oldest queued RGBA region newer than `after_frame_seq`
/// using a monitor-local pixel rectangle, preserving live-stream frame order.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`,
/// `out_frame_seq` and `out_frame_age_micros` must be valid writable pointers, and
/// `out_region` must be a valid writable pointer. The caller must later release the
/// returned region buffer with `rsnap_owned_rgba_region_release`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_take_next_region_rgba_pixels_after_seq(
	handle: *mut RsnapLiveSamplerHandle,
	monitor: RsnapMonitorRect,
	rect: RsnapPixelRect,
	after_frame_seq: u64,
	wait_for_fresh: u8,
	out_frame_seq: *mut u64,
	out_frame_age_micros: *mut u64,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { live_sampler_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_frame_seq.is_null() || out_frame_age_micros.is_null() || out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(frame) = handle.sampler.next_region_rgba_after_seq_pixels(
		decode_overlay_monitor(monitor),
		decode_pixel_rect(rect),
		after_frame_seq,
		wait_for_fresh != 0,
	) else {
		unsafe {
			ptr::write(out_frame_seq, after_frame_seq);
			ptr::write(out_frame_age_micros, 0);
			ptr::write(out_region, RsnapOwnedRgbaRegion::default());
		}

		return RsnapStatus::Empty;
	};
	let mut rgba = frame.region.rgba;
	let out = RsnapOwnedRgbaRegion {
		width: frame.region.width,
		height: frame.region.height,
		len: rgba.len(),
		capacity: rgba.capacity(),
		rgba: rgba.as_mut_ptr(),
	};

	mem::forget(rgba);

	unsafe {
		ptr::write(out_frame_seq, frame.frame_seq);
		ptr::write(out_frame_age_micros, frame.frame_age_micros);
		ptr::write(out_region, out);
	}

	RsnapStatus::Ok
}

/// Peeks the latest cached full-monitor RGBA snapshot from the live sampler without waiting
/// for a new capture.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`, and
/// `out_region` must be a valid writable pointer. The caller may first call with a null
/// `rgba` pointer and zero `capacity` to query the required size, then call again with a
/// writable buffer to receive the bytes.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_peek_latest_monitor_rgba(
	handle: *mut RsnapLiveSamplerHandle,
	monitor: RsnapMonitorRect,
	out_region: *mut RsnapRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { live_sampler_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let region = handle.sampler.peek_latest_monitor_rgba(decode_overlay_monitor(monitor));
	let Some(region) = region else {
		unsafe {
			ptr::write(out_region, RsnapRgbaRegion::default());
		}

		return RsnapStatus::Empty;
	};
	let requested = unsafe { &mut *out_region };
	let len = region.rgba.len();

	if !requested.rgba.is_null() && requested.capacity >= len {
		unsafe {
			ptr::copy_nonoverlapping(region.rgba.as_ptr(), requested.rgba, len);
		}
	}

	unsafe {
		ptr::write(
			out_region,
			RsnapRgbaRegion {
				width: region.width,
				height: region.height,
				len,
				capacity: requested.capacity,
				rgba: requested.rgba,
			},
		);
	}

	RsnapStatus::Ok
}

/// Transfers ownership of the latest cached full-monitor RGBA snapshot buffer to the caller.
///
/// This cache-only payload does not expose the original frame age or sequence, so callers must not
/// use it as the first frozen screenshot frame.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`, and
/// `out_region` must be a valid writable pointer. The caller must later release the
/// returned buffer with `rsnap_owned_rgba_region_release`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_take_latest_monitor_rgba(
	handle: *mut RsnapLiveSamplerHandle,
	monitor: RsnapMonitorRect,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { live_sampler_handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(region) = handle.sampler.peek_latest_monitor_rgba(decode_overlay_monitor(monitor))
	else {
		unsafe {
			ptr::write(out_region, RsnapOwnedRgbaRegion::default());
		}

		return RsnapStatus::Empty;
	};
	let mut rgba = region.rgba;
	let out = RsnapOwnedRgbaRegion {
		width: region.width,
		height: region.height,
		len: rgba.len(),
		capacity: rgba.capacity(),
		rgba: rgba.as_mut_ptr(),
	};

	mem::forget(rgba);

	unsafe {
		ptr::write(out_region, out);
	}

	RsnapStatus::Ok
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

unsafe fn handle_mut<'a>(handle: *mut RsnapSessionHandle) -> Option<&'a mut RsnapSessionHandle> {
	unsafe { handle.as_mut() }
}

unsafe fn handle_ref<'a>(handle: *const RsnapSessionHandle) -> Option<&'a RsnapSessionHandle> {
	unsafe { handle.as_ref() }
}

unsafe fn scroll_session_handle_mut<'a>(
	handle: *mut RsnapScrollSessionHandle,
) -> Option<&'a mut RsnapScrollSessionHandle> {
	unsafe { handle.as_mut() }
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

#[cfg(target_os = "macos")]
unsafe fn live_sampler_handle_mut<'a>(
	handle: *mut RsnapLiveSamplerHandle,
) -> Option<&'a mut RsnapLiveSamplerHandle> {
	unsafe { handle.as_mut() }
}

unsafe fn rgba_bytes<'a>(rgba: *const u8, rgba_len: usize) -> Option<&'a [u8]> {
	if rgba.is_null() || rgba_len == 0 {
		return None;
	}

	Some(unsafe { slice::from_raw_parts(rgba, rgba_len) })
}

fn decode_pixel_rect(rect: RsnapPixelRect) -> RectPoints {
	RectPoints::new(rect.x, rect.y, rect.width, rect.height)
}

fn decode_float_rect(rect: RsnapFloatRect) -> DisplayPointRect {
	DisplayPointRect::new(rect.x, rect.y, rect.width, rect.height)
}

unsafe fn decode_frozen_overlay_export_elements(
	elements: *const RsnapFrozenOverlayExportElement,
	elements_len: usize,
) -> Option<Vec<FrozenOverlayExportElement>> {
	if elements_len == 0 {
		return Some(Vec::new());
	}
	if elements.is_null() {
		return None;
	}

	let elements = unsafe { slice::from_raw_parts(elements, elements_len) };

	elements
		.iter()
		.map(|element| unsafe { decode_frozen_overlay_export_element(element) })
		.collect()
}

unsafe fn decode_frozen_overlay_export_element(
	element: &RsnapFrozenOverlayExportElement,
) -> Option<FrozenOverlayExportElement> {
	let color = decode_frozen_annotation_color(element.color);

	match element.kind {
		RsnapFrozenOverlayExportElementKind::Pen => {
			Some(FrozenOverlayExportElement::Pen(FrozenOverlayExportPen {
				points: unsafe {
					decode_frozen_overlay_points(element.points, element.points_len)
				}?,
				style: FrozenOverlayExportStrokeStyle {
					stroke_width_points: decode_f32(element.stroke_width_points)?,
					rgba: color,
				},
			}))
		},
		RsnapFrozenOverlayExportElementKind::Arrow => {
			Some(FrozenOverlayExportElement::Arrow(FrozenOverlayExportArrow {
				start: decode_frozen_overlay_point(element.start)?,
				end: decode_frozen_overlay_point(element.end)?,
				style: FrozenOverlayExportStrokeStyle {
					stroke_width_points: decode_f32(element.stroke_width_points)?,
					rgba: color,
				},
			}))
		},
		RsnapFrozenOverlayExportElementKind::Mosaic => {
			Some(FrozenOverlayExportElement::Mosaic(FrozenOverlayExportMosaic {
				rect: decode_float_rect(element.rect),
			}))
		},
		RsnapFrozenOverlayExportElementKind::Spotlight => {
			Some(FrozenOverlayExportElement::Spotlight(FrozenOverlayExportSpotlight {
				rect: decode_float_rect(element.rect),
				style: FrozenOverlayExportSpotlightStyle {
					border_width_points: decode_f32(element.border_width_points)?,
					border_rgba: color,
				},
			}))
		},
		RsnapFrozenOverlayExportElementKind::Text => {
			Some(FrozenOverlayExportElement::Text(FrozenOverlayExportText {
				anchor: decode_frozen_overlay_point(element.start)?,
				text: unsafe { decode_optional_utf8(element.text) }?,
				style: FrozenOverlayExportTextStyle {
					font_size_points: decode_f32(element.font_size_points)?,
					rgba: color,
				},
			}))
		},
	}
}

unsafe fn decode_frozen_overlay_points(
	points: *const RsnapFloatPoint,
	points_len: usize,
) -> Option<Vec<FrozenOverlayExportPoint>> {
	if points_len == 0 {
		return Some(Vec::new());
	}
	if points.is_null() {
		return None;
	}

	unsafe { slice::from_raw_parts(points, points_len) }
		.iter()
		.map(|point| decode_frozen_overlay_point(*point))
		.collect()
}

fn decode_frozen_overlay_point(point: RsnapFloatPoint) -> Option<FrozenOverlayExportPoint> {
	if point.x.is_finite() && point.y.is_finite() {
		Some(FrozenOverlayExportPoint::new(point.x, point.y))
	} else {
		None
	}
}

unsafe fn decode_optional_utf8(text: *const c_char) -> Option<String> {
	if text.is_null() {
		return Some(String::new());
	}

	unsafe { CStr::from_ptr(text) }.to_str().ok().map(ToOwned::to_owned)
}

fn decode_f32(value: f64) -> Option<f32> {
	(value.is_finite() && value >= f64::from(f32::MIN) && value <= f64::from(f32::MAX))
		.then_some(value as f32)
}

fn decode_frozen_annotation_color(color: RsnapFrozenAnnotationColor) -> [u8; 4] {
	match color {
		RsnapFrozenAnnotationColor::White => [255, 255, 255, 255],
		RsnapFrozenAnnotationColor::Yellow => [255, 219, 77, 255],
		RsnapFrozenAnnotationColor::Green => [92, 214, 149, 255],
		RsnapFrozenAnnotationColor::Blue => [102, 178, 255, 255],
		RsnapFrozenAnnotationColor::Red => [255, 107, 107, 255],
		RsnapFrozenAnnotationColor::Black => [24, 24, 24, 255],
	}
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

unsafe fn decode_bgra_frame<'a>(
	width: u32,
	height: u32,
	bytes_per_row: usize,
	bgra: *const u8,
	bgra_len: usize,
) -> Option<BgraFrameView<'a>> {
	if bgra.is_null() {
		return None;
	}

	let bytes = unsafe { slice::from_raw_parts(bgra, bgra_len) };
	let frame = BgraFrameView { width, height, bytes_per_row, bytes };

	frame.is_valid().then_some(frame)
}

fn encode_float_rect(rect: DisplayPointRect) -> RsnapFloatRect {
	RsnapFloatRect { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
}

fn encode_pixel_rect(rect: RectPoints) -> RsnapPixelRect {
	RsnapPixelRect { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
}

fn decode_capture_frame_source_kind(kind: RsnapCaptureFrameSourceKind) -> CaptureFrameSourceKind {
	match kind {
		RsnapCaptureFrameSourceKind::DragRegion => CaptureFrameSourceKind::DragRegion,
		RsnapCaptureFrameSourceKind::Window => CaptureFrameSourceKind::Window,
		RsnapCaptureFrameSourceKind::FullScreen => CaptureFrameSourceKind::FullScreen,
		RsnapCaptureFrameSourceKind::ScrollCapture => CaptureFrameSourceKind::ScrollCapture,
		RsnapCaptureFrameSourceKind::Unknown => CaptureFrameSourceKind::Unknown,
	}
}

fn decode_capture_frame_background_kind(
	kind: RsnapCaptureFrameBackgroundKind,
) -> CaptureFrameBackgroundKind {
	match kind {
		RsnapCaptureFrameBackgroundKind::SystemWallpaper => {
			CaptureFrameBackgroundKind::SystemWallpaper
		},
		RsnapCaptureFrameBackgroundKind::Aurora => CaptureFrameBackgroundKind::Aurora,
		RsnapCaptureFrameBackgroundKind::Graphite => CaptureFrameBackgroundKind::Graphite,
		RsnapCaptureFrameBackgroundKind::Linen => CaptureFrameBackgroundKind::Linen,
	}
}

fn decode_capture_frame_render_kind(kind: RsnapCaptureFrameRenderKind) -> CaptureFrameRenderKind {
	match kind {
		RsnapCaptureFrameRenderKind::FramedCapture => CaptureFrameRenderKind::FramedCapture,
		RsnapCaptureFrameRenderKind::WindowSnapshot => CaptureFrameRenderKind::WindowSnapshot,
	}
}

unsafe fn capture_frame_wallpaper_for_render(
	source: CaptureFrameRenderImageRef<'_>,
	screen_scale_factor: f64,
	source_kind: CaptureFrameSourceKind,
	background_kind: CaptureFrameBackgroundKind,
	wallpaper_path: *const c_char,
) -> Result<Option<RgbaExportImage>, ()> {
	if wallpaper_path.is_null() {
		return Ok(None);
	}

	let Some(plan) = rsnap_capture_core::capture_frame_plan(
		source.width(),
		source.height(),
		screen_scale_factor,
		source_kind,
	) else {
		return Ok(None);
	};
	let Some(request) = rsnap_capture_core::capture_frame_wallpaper_request_plan(
		background_kind,
		plan.canvas_width,
		plan.canvas_height,
	) else {
		return Ok(None);
	};
	let path = unsafe { CStr::from_ptr(wallpaper_path) }.to_str().map_err(|_| ())?;

	match rsnap_capture_core::capture_frame_wallpaper_png_thumbnail_cached(
		path,
		request.target_pixel_size,
	) {
		Ok(thumbnail) => Ok(thumbnail),
		Err(_err) => Ok(None),
	}
}

fn encode_capture_frame_plan(plan: CaptureFramePlan) -> RsnapCaptureFramePlan {
	RsnapCaptureFramePlan {
		canvas_width: plan.canvas_width,
		canvas_height: plan.canvas_height,
		image_rect: encode_float_rect(plan.image_rect),
		corner_radius: plan.corner_radius,
		shadows: plan.shadows.map(encode_capture_frame_shadow),
	}
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

fn encode_capture_frame_background_plan(
	plan: CaptureFrameBackgroundPlan,
) -> RsnapCaptureFrameBackgroundPlan {
	RsnapCaptureFrameBackgroundPlan {
		colors: plan.colors.map(encode_capture_frame_color_stop),
		locations: plan.locations,
		prefers_wallpaper: u8::from(plan.prefers_wallpaper),
		wallpaper_overlay_alpha: plan.wallpaper_overlay_alpha,
	}
}

fn encode_capture_frame_color_stop(color: CaptureFrameColorStop) -> RsnapCaptureFrameColorStop {
	RsnapCaptureFrameColorStop {
		red: color.red,
		green: color.green,
		blue: color.blue,
		alpha: color.alpha,
	}
}

fn encode_capture_frame_shadow(shadow: CaptureFrameShadow) -> RsnapCaptureFrameShadow {
	RsnapCaptureFrameShadow {
		offset_x: shadow.offset_x,
		offset_y: shadow.offset_y,
		blur: shadow.blur,
		alpha: shadow.alpha,
	}
}

fn encode_capture_frame_wallpaper_request(
	request: CaptureFrameWallpaperRequest,
) -> RsnapCaptureFrameWallpaperRequest {
	RsnapCaptureFrameWallpaperRequest {
		target_pixel_size: request.target_pixel_size,
		overlay_alpha: request.overlay_alpha,
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

fn decode_session_config(config: RsnapSessionConfig) -> SessionConfig {
	SessionConfig {
		platform: match config.platform {
			RsnapPlatformTag::MacOS => PlatformTag::MacOS,
			RsnapPlatformTag::Windows => PlatformTag::Windows,
			RsnapPlatformTag::Linux => PlatformTag::Linux,
			RsnapPlatformTag::Unsupported => PlatformTag::Unsupported,
		},
		allow_text_input: config.allow_text_input != 0,
		prefers_toolbar_above_selection: config.prefers_toolbar_above_selection != 0,
	}
}

fn decode_host_event(event: RsnapHostEvent) -> HostEvent {
	match event.kind {
		kind if kind == RsnapHostEventKind::SessionActivated as u32 => HostEvent::SessionActivated,
		kind if kind == RsnapHostEventKind::PointerMoved as u32 => HostEvent::PointerMoved {
			point: decode_optional_point(event.point, event.has_point)
				.unwrap_or_else(|| rsnap_capture_core::GlobalPoint::new(0, 0)),
			rgb: decode_optional_rgb(event.rgb, event.has_rgb),
			active_monitor: decode_optional_monitor(event.active_monitor, event.has_active_monitor),
			highlighted_window: decode_optional_window(
				event.highlighted_window,
				event.has_highlighted_window,
			),
		},
		kind if kind == RsnapHostEventKind::PrimaryInteractionStarted as u32 => {
			HostEvent::PrimaryInteractionStarted {
				point: decode_optional_point(event.point, event.has_point)
					.unwrap_or_else(|| rsnap_capture_core::GlobalPoint::new(0, 0)),
				active_monitor: decode_optional_monitor(
					event.active_monitor,
					event.has_active_monitor,
				),
				highlighted_window: decode_optional_window(
					event.highlighted_window,
					event.has_highlighted_window,
				),
			}
		},
		kind if kind == RsnapHostEventKind::PrimaryInteractionUpdated as u32 => {
			HostEvent::PrimaryInteractionUpdated {
				point: decode_optional_point(event.point, event.has_point)
					.unwrap_or_else(|| rsnap_capture_core::GlobalPoint::new(0, 0)),
				active_monitor: decode_optional_monitor(
					event.active_monitor,
					event.has_active_monitor,
				),
				highlighted_window: decode_optional_window(
					event.highlighted_window,
					event.has_highlighted_window,
				),
			}
		},
		kind if kind == RsnapHostEventKind::PrimaryInteractionCompleted as u32 => {
			HostEvent::PrimaryInteractionCompleted {
				point: decode_optional_point(event.point, event.has_point)
					.unwrap_or_else(|| rsnap_capture_core::GlobalPoint::new(0, 0)),
				active_monitor: decode_optional_monitor(
					event.active_monitor,
					event.has_active_monitor,
				),
				highlighted_window: decode_optional_window(
					event.highlighted_window,
					event.has_highlighted_window,
				),
			}
		},
		kind if kind == RsnapHostEventKind::CancelRequested as u32 => HostEvent::CancelRequested,
		kind if kind == RsnapHostEventKind::CopyRequested as u32 => HostEvent::CopyRequested,
		kind if kind == RsnapHostEventKind::SaveRequested as u32 => HostEvent::SaveRequested,
		kind if kind == RsnapHostEventKind::RecognizeTextRequested as u32 => {
			HostEvent::RecognizeTextRequested
		},
		kind if kind == RsnapHostEventKind::ToggleLoupe as u32 => HostEvent::ToggleLoupe,
		kind if kind == RsnapHostEventKind::ToolbarItemInvoked as u32 => {
			HostEvent::ToolbarItemInvoked {
				item: decode_toolbar_item_kind(event.toolbar_item_kind),
			}
		},
		_ => HostEvent::CancelRequested,
	}
}

fn decode_host_report(report: RsnapHostReport) -> HostReport {
	match report.kind {
		kind if kind == RsnapHostReportKind::FreezeSnapshotCommitted as u32 => {
			HostReport::FreezeSnapshotCommitted {
				selection: decode_optional_rect(report.selection, report.has_selection)
					.unwrap_or_default(),
			}
		},
		kind if kind == RsnapHostReportKind::HostEffectCompleted as u32 => {
			HostReport::HostEffectCompleted { effect: decode_effect_kind(report.effect_kind) }
		},
		kind if kind == RsnapHostReportKind::PermissionChanged as u32 => {
			HostReport::PermissionChanged {
				kind: decode_permission_kind(report.permission_kind),
				granted: report.granted != 0,
			}
		},
		kind if kind == RsnapHostReportKind::StatusMessage as u32 => HostReport::StatusMessage {
			message: decode_status_message(&report.status_message, report.status_message_len),
		},
		_ => HostReport::PermissionChanged {
			kind: decode_permission_kind(report.permission_kind),
			granted: report.granted != 0,
		},
	}
}

fn encode_scene_model(scene: &SceneModel) -> RsnapSceneModel {
	RsnapSceneModel {
		scene_kind: encode_scene_kind(scene.mode) as u32,
		cursor_intent: encode_cursor_intent(scene.cursor_intent) as u32,
		pointer: encode_point(scene.pointer.unwrap_or_default()),
		has_pointer: u8::from(scene.pointer.is_some()),
		active_monitor: scene.active_monitor.map_or_else(RsnapMonitorRect::default, encode_monitor),
		has_active_monitor: u8::from(scene.active_monitor.is_some()),
		highlighted_window: scene
			.highlighted_window
			.map_or_else(RsnapWindowRect::default, encode_window),
		has_highlighted_window: u8::from(scene.highlighted_window.is_some()),
		live_selection_preview: encode_rect(scene.live_selection_preview.unwrap_or_default()),
		has_live_selection_preview: u8::from(scene.live_selection_preview.is_some()),
		frozen_selection: encode_rect(scene.frozen_selection.unwrap_or_default()),
		has_frozen_selection: u8::from(scene.frozen_selection.is_some()),
		rgb: encode_rgb(scene.hud.rgb.unwrap_or_default()),
		has_rgb: u8::from(scene.hud.rgb.is_some()),
		loupe_visible: u8::from(scene.hud.loupe_visible),
		toolbar_item_count: scene.toolbar_items.len().min(RSNAP_TOOLBAR_ITEM_CAPACITY) as u32,
		toolbar_items: encode_toolbar_items(&scene.toolbar_items),
		status_message_len: scene
			.status_message
			.as_ref()
			.map_or(0, |message| message.len().min(RSNAP_STATUS_MESSAGE_CAPACITY) as u32),
		status_message: encode_status_message(scene.status_message.as_deref()),
	}
}

fn encode_toolbar_items(
	items: &[ToolbarItemModel],
) -> [RsnapToolbarItem; RSNAP_TOOLBAR_ITEM_CAPACITY] {
	let mut encoded = [RsnapToolbarItem::default(); RSNAP_TOOLBAR_ITEM_CAPACITY];

	for (index, item) in items.iter().take(RSNAP_TOOLBAR_ITEM_CAPACITY).enumerate() {
		encoded[index] = RsnapToolbarItem {
			kind: encode_toolbar_item_kind(item.kind) as u32,
			enabled: u8::from(item.enabled),
			selected: u8::from(item.selected),
			present: 1,
		};
	}

	encoded
}

fn encode_status_message(message: Option<&str>) -> [u8; RSNAP_STATUS_MESSAGE_CAPACITY] {
	let mut encoded = [0; RSNAP_STATUS_MESSAGE_CAPACITY];
	let Some(message) = message else {
		return encoded;
	};
	let bytes = message.as_bytes();
	let len = bytes.len().min(RSNAP_STATUS_MESSAGE_CAPACITY);

	encoded[..len].copy_from_slice(&bytes[..len]);

	encoded
}

fn decode_status_message(bytes: &[u8; RSNAP_STATUS_MESSAGE_CAPACITY], len: u32) -> String {
	let count = usize::try_from(len)
		.ok()
		.unwrap_or(RSNAP_STATUS_MESSAGE_CAPACITY)
		.min(RSNAP_STATUS_MESSAGE_CAPACITY);

	String::from_utf8_lossy(&bytes[..count]).into_owned()
}

fn encode_scene_kind(mode: CaptureMode) -> RsnapSceneKind {
	match mode {
		CaptureMode::Hidden => RsnapSceneKind::Hidden,
		CaptureMode::Live => RsnapSceneKind::Live,
		CaptureMode::Frozen => RsnapSceneKind::Frozen,
	}
}

fn encode_cursor_intent(intent: CursorIntent) -> RsnapCursorIntent {
	match intent {
		CursorIntent::Default => RsnapCursorIntent::Default,
		CursorIntent::Crosshair => RsnapCursorIntent::Crosshair,
		CursorIntent::Grab => RsnapCursorIntent::Grab,
		CursorIntent::Grabbing => RsnapCursorIntent::Grabbing,
		CursorIntent::ResizeNorth => RsnapCursorIntent::ResizeNorth,
		CursorIntent::ResizeSouth => RsnapCursorIntent::ResizeSouth,
		CursorIntent::ResizeEast => RsnapCursorIntent::ResizeEast,
		CursorIntent::ResizeWest => RsnapCursorIntent::ResizeWest,
		CursorIntent::ResizeNorthEast => RsnapCursorIntent::ResizeNorthEast,
		CursorIntent::ResizeNorthWest => RsnapCursorIntent::ResizeNorthWest,
		CursorIntent::ResizeSouthEast => RsnapCursorIntent::ResizeSouthEast,
		CursorIntent::ResizeSouthWest => RsnapCursorIntent::ResizeSouthWest,
		CursorIntent::Text => RsnapCursorIntent::Text,
	}
}

fn encode_toolbar_item_kind(kind: ToolbarItemKind) -> RsnapToolbarItemKind {
	match kind {
		ToolbarItemKind::Pointer => RsnapToolbarItemKind::Pointer,
		ToolbarItemKind::Pen => RsnapToolbarItemKind::Pen,
		ToolbarItemKind::Arrow => RsnapToolbarItemKind::Arrow,
		ToolbarItemKind::Text => RsnapToolbarItemKind::Text,
		ToolbarItemKind::Mosaic => RsnapToolbarItemKind::Mosaic,
		ToolbarItemKind::Spotlight => RsnapToolbarItemKind::Spotlight,
		ToolbarItemKind::Undo => RsnapToolbarItemKind::Undo,
		ToolbarItemKind::Redo => RsnapToolbarItemKind::Redo,
		ToolbarItemKind::AutoCenter => RsnapToolbarItemKind::AutoCenter,
		ToolbarItemKind::Scroll => RsnapToolbarItemKind::Scroll,
		ToolbarItemKind::Ocr => RsnapToolbarItemKind::Ocr,
		ToolbarItemKind::Copy => RsnapToolbarItemKind::Copy,
		ToolbarItemKind::Save => RsnapToolbarItemKind::Save,
	}
}

fn decode_toolbar_item_kind(kind: u32) -> ToolbarItemKind {
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

fn encode_host_request(request: HostRequest) -> RsnapHostRequestValue {
	match request {
		HostRequest::StartLiveCapture => RsnapHostRequestValue {
			kind: RsnapHostRequestKind::StartLiveCapture as u32,
			..RsnapHostRequestValue::default()
		},
		HostRequest::StopLiveCapture => RsnapHostRequestValue {
			kind: RsnapHostRequestKind::StopLiveCapture as u32,
			..RsnapHostRequestValue::default()
		},
		HostRequest::RequestFreezeSnapshot { selection, selection_editable } => {
			RsnapHostRequestValue {
				kind: RsnapHostRequestKind::RequestFreezeSnapshot as u32,
				selection: encode_rect(selection),
				has_selection: 1,
				selection_editable: u8::from(selection_editable),
			}
		},
		HostRequest::StartScrollCapture => RsnapHostRequestValue {
			kind: RsnapHostRequestKind::StartScrollCapture as u32,
			..RsnapHostRequestValue::default()
		},
		HostRequest::PerformHostEffect(effect) => RsnapHostRequestValue {
			kind: match effect {
				HostEffectKind::CopyCapture => RsnapHostRequestKind::CopyCapture,
				HostEffectKind::SaveCapture => RsnapHostRequestKind::SaveCapture,
				HostEffectKind::RecognizeText => RsnapHostRequestKind::RecognizeText,
			} as u32,
			..RsnapHostRequestValue::default()
		},
		HostRequest::RequestPermission(PermissionKind::ScreenRecording) => RsnapHostRequestValue {
			kind: RsnapHostRequestKind::RequestScreenRecordingPermission as u32,
			..RsnapHostRequestValue::default()
		},
	}
}

fn encode_scroll_observe_result(
	outcome: ScrollStitchObserveOutcome,
	export_width: u32,
	export_height: u32,
	session: &ScrollStitchSession,
) -> RsnapScrollObserveResult {
	let (kind, growth_rows) = match outcome {
		ScrollStitchObserveOutcome::NoChange => (RsnapScrollObserveOutcomeKind::NoChange, 0),
		ScrollStitchObserveOutcome::PreviewUpdated => {
			(RsnapScrollObserveOutcomeKind::PreviewUpdated, 0)
		},
		ScrollStitchObserveOutcome::Committed { growth_rows } => {
			(RsnapScrollObserveOutcomeKind::Committed, growth_rows)
		},
		ScrollStitchObserveOutcome::UnsupportedDirection => {
			(RsnapScrollObserveOutcomeKind::UnsupportedDirection, 0)
		},
	};

	RsnapScrollObserveResult {
		kind: kind as u32,
		growth_rows,
		export_width,
		export_height,
		current_viewport_top_y: session.current_viewport_top_y(),
	}
}

fn owned_region_from_scroll_image(image: ScrollStitchImage) -> RsnapOwnedRgbaRegion {
	owned_region_from_raw_rgba(image.width, image.height, image.rgba)
}

fn owned_region_from_raw_rgba(width: u32, height: u32, mut rgba: Vec<u8>) -> RsnapOwnedRgbaRegion {
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

fn owned_bytes_from_vec(mut bytes: Vec<u8>) -> RsnapOwnedBytes {
	let out =
		RsnapOwnedBytes { len: bytes.len(), capacity: bytes.capacity(), bytes: bytes.as_mut_ptr() };

	mem::forget(bytes);

	out
}

fn decode_effect_kind(effect_kind: u32) -> HostEffectKind {
	match effect_kind {
		kind if kind == RsnapHostEffectKind::CopyCapture as u32 => HostEffectKind::CopyCapture,
		kind if kind == RsnapHostEffectKind::SaveCapture as u32 => HostEffectKind::SaveCapture,
		kind if kind == RsnapHostEffectKind::RecognizeText as u32 => HostEffectKind::RecognizeText,
		_ => HostEffectKind::CopyCapture,
	}
}

fn decode_permission_kind(permission_kind: u32) -> PermissionKind {
	match permission_kind {
		kind if kind == RsnapPermissionKind::ScreenRecording as u32 => {
			PermissionKind::ScreenRecording
		},
		_ => PermissionKind::ScreenRecording,
	}
}

fn decode_optional_point(
	point: RsnapPoint,
	has_point: u8,
) -> Option<rsnap_capture_core::GlobalPoint> {
	(has_point != 0).then_some(rsnap_capture_core::GlobalPoint::new(point.x, point.y))
}

fn decode_optional_rgb(rgb: RsnapRgb, has_rgb: u8) -> Option<Rgb> {
	(has_rgb != 0).then_some(Rgb::new(rgb.r, rgb.g, rgb.b))
}

fn decode_optional_rect(rect: RsnapRect, has_rect: u8) -> Option<GlobalRect> {
	(has_rect != 0).then_some(GlobalRect::new(rect.x, rect.y, rect.width, rect.height))
}

fn decode_optional_monitor(
	monitor: RsnapMonitorRect,
	has_monitor: u8,
) -> Option<rsnap_capture_core::MonitorRect> {
	(has_monitor != 0).then_some(rsnap_capture_core::MonitorRect {
		id: monitor.id,
		origin: decode_point(monitor.origin),
		width: monitor.width,
		height: monitor.height,
		scale_factor_x1000: monitor.scale_factor_x1000,
	})
}

fn decode_optional_window(window: RsnapWindowRect, has_window: u8) -> Option<WindowRect> {
	(has_window != 0).then_some(WindowRect {
		window_id: (window.has_window_id != 0).then_some(window.window_id),
		x: window.x,
		y: window.y,
		width: window.width,
		height: window.height,
	})
}

fn encode_point(point: rsnap_capture_core::GlobalPoint) -> RsnapPoint {
	RsnapPoint { x: point.x, y: point.y }
}

fn decode_point(point: RsnapPoint) -> rsnap_capture_core::GlobalPoint {
	rsnap_capture_core::GlobalPoint::new(point.x, point.y)
}

#[cfg(target_os = "macos")]
fn decode_overlay_point(point: RsnapPoint) -> rsnap_overlay::session::GlobalPoint {
	rsnap_overlay::session::GlobalPoint::new(point.x, point.y)
}

fn encode_rgb(rgb: Rgb) -> RsnapRgb {
	RsnapRgb { r: rgb.r, g: rgb.g, b: rgb.b }
}

fn encode_rect(rect: GlobalRect) -> RsnapRect {
	RsnapRect { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
}

fn encode_monitor(monitor: rsnap_capture_core::MonitorRect) -> RsnapMonitorRect {
	RsnapMonitorRect {
		id: monitor.id,
		origin: encode_point(monitor.origin),
		width: monitor.width,
		height: monitor.height,
		scale_factor_x1000: monitor.scale_factor_x1000,
	}
}

#[cfg(target_os = "macos")]
fn decode_overlay_monitor(monitor: RsnapMonitorRect) -> rsnap_overlay::session::MonitorRect {
	rsnap_overlay::session::MonitorRect {
		id: monitor.id,
		origin: rsnap_overlay::session::GlobalPoint::new(monitor.origin.x, monitor.origin.y),
		width: monitor.width,
		height: monitor.height,
		scale_factor_x1000: monitor.scale_factor_x1000,
	}
}

fn encode_window(window: WindowRect) -> RsnapWindowRect {
	RsnapWindowRect {
		window_id: window.window_id.unwrap_or_default(),
		has_window_id: u8::from(window.window_id.is_some()),
		x: window.x,
		y: window.y,
		width: window.width,
		height: window.height,
	}
}

#[cfg(test)]
mod tests;
