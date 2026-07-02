//! Thin C ABI bridge for the native-host reset.
//!
//! The ABI surface is intentionally small in the first landing slice. It proves the
//! new host/core direction with an opaque session handle, FFI-safe config/event
//! structs, and copy-out scene/request snapshots.

mod abi;
mod capture_frame;
mod frozen_overlay;
#[cfg(target_os = "macos")]
mod live_sampler;
mod scroll_session;

#[cfg(target_os = "macos")]
pub use self::abi::RsnapLiveSamplerHandle;
pub use self::abi::{
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
	rsnap_frozen_overlay_edit_snapshot_release, rsnap_frozen_overlay_export_render_rgba,
};
#[cfg(target_os = "macos")]
pub use self::live_sampler::{
	rsnap_live_sampler_create, rsnap_live_sampler_create_with_self_capture_exception_window_ids,
	rsnap_live_sampler_destroy, rsnap_live_sampler_peek_latest_monitor_rgba,
	rsnap_live_sampler_peek_region_rgba, rsnap_live_sampler_prime_monitor,
	rsnap_live_sampler_reset, rsnap_live_sampler_sample_cursor,
	rsnap_live_sampler_take_latest_monitor_rgba,
	rsnap_live_sampler_take_next_region_rgba_after_seq,
	rsnap_live_sampler_take_next_region_rgba_pixels_after_seq, rsnap_live_sampler_take_region_rgba,
};
pub use self::scroll_session::{
	rsnap_scroll_session_create, rsnap_scroll_session_destroy,
	rsnap_scroll_session_observe_downward_frame,
	rsnap_scroll_session_observe_downward_frame_with_motion_hint,
	rsnap_scroll_session_take_export_rgba, rsnap_scroll_session_take_preview_rgba,
	rsnap_scroll_session_undo_last_append,
};

use std::mem;
use std::ptr::{self, NonNull};
use std::slice;

#[cfg(not(target_os = "macos"))]
use rsnap_overlay as _;

use self::abi::{RSNAP_STATUS_MESSAGE_CAPACITY, RSNAP_TOOLBAR_ITEM_CAPACITY};
use rsnap_capture_core::SceneModel;
use rsnap_capture_core::{
	self, AutoCenterImageError, CaptureMode, CaptureSessionCore, CursorIntent, DisplayPointRect,
	FrozenSelectionTransformInput, FrozenSelectionTransformKind, GlobalRect, HostEffectKind,
	HostEvent, HostReport, HostRequest, PermissionKind, PlatformTag, RectPoints, Rgb,
	ScrollMinimapInput, ScrollMinimapPlan, SessionConfig, ToolbarItemKind, ToolbarItemModel,
	WindowRect,
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

unsafe fn handle_mut<'a>(handle: *mut RsnapSessionHandle) -> Option<&'a mut RsnapSessionHandle> {
	unsafe { handle.as_mut() }
}

unsafe fn handle_ref<'a>(handle: *const RsnapSessionHandle) -> Option<&'a RsnapSessionHandle> {
	unsafe { handle.as_ref() }
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
