//! Thin C ABI bridge for the native-host reset.
//!
//! The ABI surface is intentionally small in the first landing slice. It proves the
//! new host/core direction with an opaque session handle, FFI-safe config/event
//! structs, and copy-out scene/request snapshots.

use std::ptr::{self, NonNull};

use rsnap_capture_core::{
	CaptureMode, CaptureSessionCore, CursorIntent, GlobalPoint, GlobalRect, HostEffectKind,
	HostEvent, HostReport, HostRequest, MonitorRect, PermissionKind, PlatformTag, Rgb,
	SessionConfig, ToolbarItemKind, ToolbarItemModel, WindowRect,
};
#[cfg(target_os = "macos")]
use rsnap_overlay::{
	host_live_sampling_macos::HostMacLiveSampler,
	session::{GlobalPoint as OverlayGlobalPoint, MonitorRect as OverlayMonitorRect},
};

/// ABI version exported by the thin C host bridge.
pub const RSNAP_HOST_FFI_ABI_VERSION: u32 = 13;
const RSNAP_TOOLBAR_ITEM_CAPACITY: usize = 16;
const RSNAP_STATUS_MESSAGE_CAPACITY: usize = 256;
const RSNAP_LIVE_SAMPLE_PATCH_CAPACITY: usize = 4096;

/// Opaque session handle owned by the native host through the C ABI.
pub struct RsnapSessionHandle {
	session: CaptureSessionCore,
}

#[cfg(target_os = "macos")]
/// Opaque live-sampler handle owned by the native host through the C ABI.
pub struct RsnapLiveSamplerHandle {
	sampler: HostMacLiveSampler,
}

/// Result code returned by FFI entry points.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapStatus {
	/// The operation succeeded.
	Ok = 0,
	/// The provided session handle was null.
	NullHandle = 1,
	/// The provided output pointer was null.
	NullOutput = 2,
	/// No queued value was available.
	Empty = 3,
}

/// FFI-safe live cursor sample copied out of the native Rust sampler.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapLiveSample {
	/// Sampled RGB value.
	pub rgb: RsnapRgb,
	/// Non-zero when `rgb` is present.
	pub has_rgb: u8,
	/// Sampled loupe patch width in pixels.
	pub patch_width: u32,
	/// Sampled loupe patch height in pixels.
	pub patch_height: u32,
	/// Byte count copied into `patch_rgba`.
	pub patch_len: u32,
	/// Optional RGBA patch bytes in row-major order.
	pub patch_rgba: [u8; RSNAP_LIVE_SAMPLE_PATCH_CAPACITY],
}

impl Default for RsnapLiveSample {
	fn default() -> Self {
		Self {
			rgb: RsnapRgb::default(),
			has_rgb: 0,
			patch_width: 0,
			patch_height: 0,
			patch_len: 0,
			patch_rgba: [0; RSNAP_LIVE_SAMPLE_PATCH_CAPACITY],
		}
	}
}

/// FFI-safe owned RGBA image region copied out of the cached live sampler frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapRgbaRegion {
	/// Region width in pixels.
	pub width: u32,
	/// Region height in pixels.
	pub height: u32,
	/// Byte count in `rgba`.
	pub len: usize,
	/// Caller-provided buffer capacity in bytes.
	pub capacity: usize,
	/// Caller-provided RGBA byte buffer in row-major order.
	pub rgba: *mut u8,
}

impl Default for RsnapRgbaRegion {
	fn default() -> Self {
		Self { width: 0, height: 0, len: 0, capacity: 0, rgba: ptr::null_mut() }
	}
}

/// FFI-safe owned RGBA image region whose buffer is retained by Rust until explicitly freed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapOwnedRgbaRegion {
	/// Region width in pixels.
	pub width: u32,
	/// Region height in pixels.
	pub height: u32,
	/// Byte count in `rgba`.
	pub len: usize,
	/// Reserved buffer capacity in bytes.
	pub capacity: usize,
	/// Owned RGBA byte buffer in row-major order.
	pub rgba: *mut u8,
}

impl Default for RsnapOwnedRgbaRegion {
	fn default() -> Self {
		Self { width: 0, height: 0, len: 0, capacity: 0, rgba: ptr::null_mut() }
	}
}

/// FFI-safe platform tag.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapPlatformTag {
	/// Native macOS host.
	MacOS = 0,
	/// Future native Windows host.
	Windows = 1,
	/// Future native Linux host.
	Linux = 2,
	/// Unsupported or test-only host.
	Unsupported = 3,
}

/// FFI-safe session configuration.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapSessionConfig {
	/// Platform family that owns the host.
	pub platform: RsnapPlatformTag,
	/// Non-zero when native text input is available.
	pub allow_text_input: u8,
	/// Non-zero when the host prefers the toolbar above the frozen selection.
	pub prefers_toolbar_above_selection: u8,
}

/// FFI-safe global point.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapPoint {
	/// Global X coordinate.
	pub x: i32,
	/// Global Y coordinate.
	pub y: i32,
}

/// FFI-safe RGB sample.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapRgb {
	/// Red channel.
	pub r: u8,
	/// Green channel.
	pub g: u8,
	/// Blue channel.
	pub b: u8,
}

/// FFI-safe global rectangle.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapRect {
	/// Global left coordinate.
	pub x: i32,
	/// Global top coordinate.
	pub y: i32,
	/// Rectangle width.
	pub width: u32,
	/// Rectangle height.
	pub height: u32,
}

/// FFI-safe monitor rectangle snapshot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapMonitorRect {
	/// Stable monitor identifier.
	pub id: u32,
	/// Monitor origin in global points.
	pub origin: RsnapPoint,
	/// Monitor width in points.
	pub width: u32,
	/// Monitor height in points.
	pub height: u32,
	/// Monitor pixel scale factor in thousandths.
	pub scale_factor_x1000: u32,
}

/// FFI-safe highlighted window snapshot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapWindowRect {
	/// Window identifier when one exists.
	pub window_id: u32,
	/// Non-zero when `window_id` is present.
	pub has_window_id: u8,
	/// Global left coordinate.
	pub x: i64,
	/// Global top coordinate.
	pub y: i64,
	/// Window width in points.
	pub width: i64,
	/// Window height in points.
	pub height: i64,
}

/// FFI-safe host event discriminator.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapHostEventKind {
	/// Capture session became active.
	SessionActivated = 0,
	/// Pointer position changed.
	PointerMoved = 1,
	/// Cancel was requested.
	CancelRequested = 3,
	/// Copy was requested.
	CopyRequested = 4,
	/// Save was requested.
	SaveRequested = 5,
	/// Text recognition was requested.
	RecognizeTextRequested = 6,
	/// Loupe visibility was toggled.
	ToggleLoupe = 7,
	/// A frozen toolbar item was invoked.
	ToolbarItemInvoked = 8,
	/// A primary interaction began in live mode.
	PrimaryInteractionStarted = 9,
	/// A primary interaction updated in live mode.
	PrimaryInteractionUpdated = 10,
	/// A primary interaction completed in live mode.
	PrimaryInteractionCompleted = 11,
}

/// FFI-safe frozen toolbar item kind.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapToolbarItemKind {
	/// Pointer tool.
	Pointer = 0,
	/// Pen tool.
	Pen = 1,
	/// Arrow tool.
	Arrow = 2,
	/// Text tool.
	Text = 3,
	/// Mosaic tool.
	Mosaic = 4,
	/// Spotlight tool.
	Spotlight = 5,
	/// Undo action.
	Undo = 6,
	/// Redo action.
	Redo = 7,
	/// Auto-center action.
	AutoCenter = 8,
	/// Scroll capture action.
	Scroll = 9,
	/// OCR action.
	Ocr = 10,
	/// Copy action.
	Copy = 11,
	/// Save action.
	Save = 12,
}

/// FFI-safe host event payload.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapHostEvent {
	/// Event discriminator.
	pub kind: u32,
	/// Pointer location when the event carries one.
	pub point: RsnapPoint,
	/// Non-zero when `point` is present.
	pub has_point: u8,
	/// RGB sample when the event carries one.
	pub rgb: RsnapRgb,
	/// Non-zero when `rgb` is present.
	pub has_rgb: u8,
	/// Current active monitor when the event carries one.
	pub active_monitor: RsnapMonitorRect,
	/// Non-zero when `active_monitor` is present.
	pub has_active_monitor: u8,
	/// Highlighted live window when the event carries one.
	pub highlighted_window: RsnapWindowRect,
	/// Non-zero when `highlighted_window` is present.
	pub has_highlighted_window: u8,
	/// Toolbar item kind when the event carries one.
	pub toolbar_item_kind: u32,
}

/// FFI-safe host report discriminator.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapHostReportKind {
	/// Frozen snapshot committed.
	FreezeSnapshotCommitted = 0,
	/// Host effect completed.
	HostEffectCompleted = 1,
	/// Permission state changed.
	PermissionChanged = 2,
	/// Host surfaced a status message.
	StatusMessage = 3,
}

/// FFI-safe host effect kind.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapHostEffectKind {
	/// Copy capture to the clipboard.
	CopyCapture = 0,
	/// Save capture to disk.
	SaveCapture = 1,
	/// Recognize text from the capture.
	RecognizeText = 2,
}

/// FFI-safe permission kind.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapPermissionKind {
	/// Screen recording or equivalent display capture access.
	ScreenRecording = 0,
	/// Accessibility access.
	Accessibility = 1,
	/// Input monitoring access.
	InputMonitoring = 2,
}

/// FFI-safe host report payload.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapHostReport {
	/// Report discriminator.
	pub kind: u32,
	/// Frozen selection rectangle when the report carries one.
	pub selection: RsnapRect,
	/// Non-zero when `selection` is present.
	pub has_selection: u8,
	/// Host effect kind when the report carries one.
	pub effect_kind: u32,
	/// Permission kind when the report carries one.
	pub permission_kind: u32,
	/// Non-zero when the permission is granted.
	pub granted: u8,
	/// UTF-8 status message byte count copied into `status_message`.
	pub status_message_len: u32,
	/// Optional UTF-8 status message bytes.
	pub status_message: [u8; RSNAP_STATUS_MESSAGE_CAPACITY],
}

/// FFI-safe scene kind.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapSceneKind {
	/// Capture UI is hidden.
	Hidden = 0,
	/// Live targeting is active.
	Live = 1,
	/// Frozen mode is active.
	Frozen = 2,
}

/// FFI-safe cursor intent.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapCursorIntent {
	/// Default cursor.
	Default = 0,
	/// Crosshair cursor.
	Crosshair = 1,
	/// Grab cursor.
	Grab = 2,
	/// Grabbing cursor.
	Grabbing = 3,
	/// North resize cursor.
	ResizeNorth = 4,
	/// South resize cursor.
	ResizeSouth = 5,
	/// East resize cursor.
	ResizeEast = 6,
	/// West resize cursor.
	ResizeWest = 7,
	/// North-east resize cursor.
	ResizeNorthEast = 8,
	/// North-west resize cursor.
	ResizeNorthWest = 9,
	/// South-east resize cursor.
	ResizeSouthEast = 10,
	/// South-west resize cursor.
	ResizeSouthWest = 11,
	/// Text cursor.
	Text = 12,
}

/// FFI-safe scene snapshot copy.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapSceneModel {
	/// Scene kind.
	pub scene_kind: u32,
	/// Cursor intent.
	pub cursor_intent: u32,
	/// Current pointer position.
	pub pointer: RsnapPoint,
	/// Non-zero when `pointer` is present.
	pub has_pointer: u8,
	/// Current active monitor.
	pub active_monitor: RsnapMonitorRect,
	/// Non-zero when `active_monitor` is present.
	pub has_active_monitor: u8,
	/// Highlighted live window.
	pub highlighted_window: RsnapWindowRect,
	/// Non-zero when `highlighted_window` is present.
	pub has_highlighted_window: u8,
	/// Live drag preview rectangle.
	pub live_selection_preview: RsnapRect,
	/// Non-zero when `live_selection_preview` is present.
	pub has_live_selection_preview: u8,
	/// Frozen selection rectangle.
	pub frozen_selection: RsnapRect,
	/// Non-zero when `frozen_selection` is present.
	pub has_frozen_selection: u8,
	/// Current RGB sample.
	pub rgb: RsnapRgb,
	/// Non-zero when `rgb` is present.
	pub has_rgb: u8,
	/// Non-zero when the loupe should be visible.
	pub loupe_visible: u8,
	/// Toolbar item count copied into `toolbar_items`.
	pub toolbar_item_count: u32,
	/// Frozen toolbar items.
	pub toolbar_items: [RsnapToolbarItem; RSNAP_TOOLBAR_ITEM_CAPACITY],
	/// UTF-8 status message byte count copied into `status_message`.
	pub status_message_len: u32,
	/// Optional UTF-8 status message bytes.
	pub status_message: [u8; RSNAP_STATUS_MESSAGE_CAPACITY],
}

impl Default for RsnapSceneModel {
	fn default() -> Self {
		Self {
			scene_kind: 0,
			cursor_intent: 0,
			pointer: RsnapPoint::default(),
			has_pointer: 0,
			active_monitor: RsnapMonitorRect::default(),
			has_active_monitor: 0,
			highlighted_window: RsnapWindowRect::default(),
			has_highlighted_window: 0,
			live_selection_preview: RsnapRect::default(),
			has_live_selection_preview: 0,
			frozen_selection: RsnapRect::default(),
			has_frozen_selection: 0,
			rgb: RsnapRgb::default(),
			has_rgb: 0,
			loupe_visible: 0,
			toolbar_item_count: 0,
			toolbar_items: [RsnapToolbarItem::default(); RSNAP_TOOLBAR_ITEM_CAPACITY],
			status_message_len: 0,
			status_message: [0; RSNAP_STATUS_MESSAGE_CAPACITY],
		}
	}
}

/// One FFI-safe frozen toolbar item.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapToolbarItem {
	/// Item kind.
	pub kind: u32,
	/// Non-zero when the item is enabled.
	pub enabled: u8,
	/// Non-zero when the item is selected.
	pub selected: u8,
	/// Non-zero when the slot is populated.
	pub present: u8,
}

/// FFI-safe host request kind.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapHostRequestKind {
	/// Start native live capture.
	StartLiveCapture = 0,
	/// Stop native live capture.
	StopLiveCapture = 1,
	/// Request a frozen snapshot.
	RequestFreezeSnapshot = 2,
	/// Copy capture effect.
	CopyCapture = 3,
	/// Save capture effect.
	SaveCapture = 4,
	/// Recognize text effect.
	RecognizeText = 5,
	/// Request screen recording permission.
	RequestScreenRecordingPermission = 6,
	/// Request accessibility permission.
	RequestAccessibilityPermission = 7,
	/// Request input monitoring permission.
	RequestInputMonitoringPermission = 8,
}

/// FFI-safe queued host request.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapHostRequestValue {
	/// Request kind.
	pub kind: u32,
	/// Optional selection payload for frozen handoff requests.
	pub selection: RsnapRect,
	/// Non-zero when `selection` is populated.
	pub has_selection: u8,
}

impl Default for RsnapHostRequestValue {
	fn default() -> Self {
		Self {
			kind: 0,
			selection: RsnapRect::default(),
			has_selection: 0,
		}
	}
}

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
	Box::into_raw(Box::new(RsnapLiveSamplerHandle {
		sampler: HostMacLiveSampler::new(),
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

	let sample = handle.sampler.sample_cursor(
		decode_overlay_monitor(monitor),
		decode_overlay_point(point),
		patch_width_px,
		patch_height_px,
	);
	let mut out = RsnapLiveSample::default();
	if let Some(rgb) = sample.rgb {
		out.rgb = RsnapRgb { r: rgb.r, g: rgb.g, b: rgb.b };
		out.has_rgb = 1;
	}
	if let Some(patch) = sample.patch {
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
	std::mem::forget(rgba);
	unsafe {
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

	let Some(region) = handle.sampler.peek_latest_monitor_rgba(decode_overlay_monitor(monitor)) else {
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
	std::mem::forget(rgba);
	unsafe {
		ptr::write(out_region, out);
	}

	RsnapStatus::Ok
}

/// Releases a buffer previously returned by `rsnap_live_sampler_take_latest_monitor_rgba`.
///
/// # Safety
///
/// `region` must point to a struct returned by `rsnap_live_sampler_take_latest_monitor_rgba`
/// that has not already been released.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_owned_rgba_region_release(
	region: *mut RsnapOwnedRgbaRegion,
) {
	let Some(region) = (unsafe { region.as_mut() }) else {
		return;
	};
	if !region.rgba.is_null() && region.capacity > 0 {
		let _ = unsafe { Vec::from_raw_parts(region.rgba, region.len, region.capacity) };
	}
	*region = RsnapOwnedRgbaRegion::default();
}

unsafe fn handle_mut<'a>(handle: *mut RsnapSessionHandle) -> Option<&'a mut RsnapSessionHandle> {
	unsafe { handle.as_mut() }
}

unsafe fn handle_ref<'a>(handle: *const RsnapSessionHandle) -> Option<&'a RsnapSessionHandle> {
	unsafe { handle.as_ref() }
}

#[cfg(target_os = "macos")]
unsafe fn live_sampler_handle_mut<'a>(
	handle: *mut RsnapLiveSamplerHandle,
) -> Option<&'a mut RsnapLiveSamplerHandle> {
	unsafe { handle.as_mut() }
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
				.unwrap_or_else(|| GlobalPoint::new(0, 0)),
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
					.unwrap_or_else(|| GlobalPoint::new(0, 0)),
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
					.unwrap_or_else(|| GlobalPoint::new(0, 0)),
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
					.unwrap_or_else(|| GlobalPoint::new(0, 0)),
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

fn encode_scene_model(scene: &rsnap_capture_core::SceneModel) -> RsnapSceneModel {
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
		HostRequest::RequestFreezeSnapshot { selection } => RsnapHostRequestValue {
			kind: RsnapHostRequestKind::RequestFreezeSnapshot as u32,
			selection: encode_rect(selection),
			has_selection: 1,
		},
		HostRequest::PerformHostEffect(effect) => RsnapHostRequestValue {
			kind: match effect {
				HostEffectKind::CopyCapture => RsnapHostRequestKind::CopyCapture,
				HostEffectKind::SaveCapture => RsnapHostRequestKind::SaveCapture,
				HostEffectKind::RecognizeText => RsnapHostRequestKind::RecognizeText,
			} as u32,
			..RsnapHostRequestValue::default()
		},
		HostRequest::RequestPermission(permission) => RsnapHostRequestValue {
			kind: match permission {
				PermissionKind::ScreenRecording => {
					RsnapHostRequestKind::RequestScreenRecordingPermission
				},
				PermissionKind::Accessibility => RsnapHostRequestKind::RequestAccessibilityPermission,
				PermissionKind::InputMonitoring => {
					RsnapHostRequestKind::RequestInputMonitoringPermission
				},
			} as u32,
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
		kind if kind == RsnapPermissionKind::Accessibility as u32 => PermissionKind::Accessibility,
		kind if kind == RsnapPermissionKind::InputMonitoring as u32 => {
			PermissionKind::InputMonitoring
		},
		_ => PermissionKind::ScreenRecording,
	}
}

fn decode_optional_point(point: RsnapPoint, has_point: u8) -> Option<GlobalPoint> {
	(has_point != 0).then_some(GlobalPoint::new(point.x, point.y))
}

fn decode_optional_rgb(rgb: RsnapRgb, has_rgb: u8) -> Option<Rgb> {
	(has_rgb != 0).then_some(Rgb::new(rgb.r, rgb.g, rgb.b))
}

fn decode_optional_rect(rect: RsnapRect, has_rect: u8) -> Option<GlobalRect> {
	(has_rect != 0).then_some(GlobalRect::new(rect.x, rect.y, rect.width, rect.height))
}

fn decode_optional_monitor(monitor: RsnapMonitorRect, has_monitor: u8) -> Option<MonitorRect> {
	(has_monitor != 0).then_some(MonitorRect {
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

fn encode_point(point: GlobalPoint) -> RsnapPoint {
	RsnapPoint { x: point.x, y: point.y }
}

fn decode_point(point: RsnapPoint) -> GlobalPoint {
	GlobalPoint::new(point.x, point.y)
}

#[cfg(target_os = "macos")]
fn decode_overlay_point(point: RsnapPoint) -> OverlayGlobalPoint {
	OverlayGlobalPoint::new(point.x, point.y)
}

fn encode_rgb(rgb: Rgb) -> RsnapRgb {
	RsnapRgb { r: rgb.r, g: rgb.g, b: rgb.b }
}

fn encode_rect(rect: GlobalRect) -> RsnapRect {
	RsnapRect { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
}

fn encode_monitor(monitor: MonitorRect) -> RsnapMonitorRect {
	RsnapMonitorRect {
		id: monitor.id,
		origin: encode_point(monitor.origin),
		width: monitor.width,
		height: monitor.height,
		scale_factor_x1000: monitor.scale_factor_x1000,
	}
}

#[cfg(target_os = "macos")]
fn decode_overlay_monitor(monitor: RsnapMonitorRect) -> OverlayMonitorRect {
	OverlayMonitorRect {
		id: monitor.id,
		origin: OverlayGlobalPoint::new(monitor.origin.x, monitor.origin.y),
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
mod tests {
	use std::ptr;

	use super::{
		RSNAP_HOST_FFI_ABI_VERSION, RSNAP_STATUS_MESSAGE_CAPACITY, RsnapCursorIntent,
		RsnapHostEvent, RsnapHostEventKind, RsnapHostReport, RsnapHostReportKind,
		RsnapHostRequestKind, RsnapHostRequestValue, RsnapMonitorRect, RsnapPlatformTag,
		RsnapSceneKind, RsnapSceneModel, RsnapSessionConfig, RsnapSessionHandle, RsnapStatus,
		RsnapWindowRect,
		rsnap_host_ffi_abi_version, rsnap_session_copy_scene_model, rsnap_session_create,
		rsnap_session_destroy, rsnap_session_enter_live, rsnap_session_handle_host_event,
		rsnap_session_handle_host_report, rsnap_session_take_next_request,
	};
	use super::{RsnapPoint, RsnapRect, RsnapRgb};

	fn default_config() -> RsnapSessionConfig {
		RsnapSessionConfig {
			platform: RsnapPlatformTag::MacOS,
			allow_text_input: 1,
			prefers_toolbar_above_selection: 0,
		}
	}

	#[test]
	fn ffi_session_enters_live_and_emits_request() {
		let handle = unsafe { rsnap_session_create(default_config()) };
		let mut request = RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };
		let mut scene = RsnapSceneModel::default();

		assert_eq!(unsafe { rsnap_session_enter_live(handle) }, RsnapStatus::Ok);
		assert_eq!(
			unsafe { rsnap_session_take_next_request(handle, &mut request) },
			RsnapStatus::Ok
		);
		assert_eq!(request.kind, RsnapHostRequestKind::StartLiveCapture as u32);
		assert_eq!(unsafe { rsnap_session_copy_scene_model(handle, &mut scene) }, RsnapStatus::Ok);
		assert_eq!(scene.scene_kind, RsnapSceneKind::Live as u32);
		assert_eq!(scene.cursor_intent, RsnapCursorIntent::Default as u32);

		unsafe { rsnap_session_destroy(handle) };
	}

	#[test]
	fn ffi_session_applies_freeze_report() {
		let handle = unsafe { rsnap_session_create(default_config()) };
		let mut scene = RsnapSceneModel::default();

		assert_eq!(unsafe { rsnap_session_enter_live(handle) }, RsnapStatus::Ok);
		let mut request = RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };
		let _ = unsafe { rsnap_session_take_next_request(handle, &mut request) };

		assert_eq!(
			unsafe {
				rsnap_session_handle_host_report(
					handle,
					RsnapHostReport {
						kind: RsnapHostReportKind::FreezeSnapshotCommitted as u32,
						selection: RsnapRect { x: 20, y: 30, width: 100, height: 60 },
						has_selection: 1,
						effect_kind: 0,
						permission_kind: 0,
						granted: 0,
						status_message_len: 0,
						status_message: [0; RSNAP_STATUS_MESSAGE_CAPACITY],
					},
				)
			},
			RsnapStatus::Ok
		);
		assert_eq!(unsafe { rsnap_session_copy_scene_model(handle, &mut scene) }, RsnapStatus::Ok);
		assert_eq!(scene.scene_kind, RsnapSceneKind::Frozen as u32);
		assert_eq!(scene.has_frozen_selection, 1);

		unsafe { rsnap_session_destroy(handle) };
	}

	#[test]
	fn ffi_session_tracks_pointer_updates() {
		let handle = unsafe { rsnap_session_create(default_config()) };
		let mut scene = RsnapSceneModel::default();

		let _ = unsafe { rsnap_session_enter_live(handle) };
		let mut request = RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };
		let _ = unsafe { rsnap_session_take_next_request(handle, &mut request) };
		assert_eq!(
			unsafe {
				rsnap_session_handle_host_event(
					handle,
					RsnapHostEvent {
						kind: RsnapHostEventKind::PointerMoved as u32,
						point: RsnapPoint { x: 50, y: 60 },
						has_point: 1,
						rgb: RsnapRgb { r: 1, g: 2, b: 3 },
						has_rgb: 1,
						active_monitor: RsnapMonitorRect {
							id: 9,
							origin: RsnapPoint { x: 0, y: 0 },
							width: 1440,
							height: 900,
							scale_factor_x1000: 2_000,
						},
						has_active_monitor: 1,
						highlighted_window: RsnapWindowRect {
							window_id: 42,
							has_window_id: 1,
							x: 20,
							y: 30,
							width: 500,
							height: 400,
						},
						has_highlighted_window: 1,
						toolbar_item_kind: 0,
					},
				)
			},
			RsnapStatus::Ok
		);
		assert_eq!(unsafe { rsnap_session_copy_scene_model(handle, &mut scene) }, RsnapStatus::Ok);
		assert_eq!(scene.has_pointer, 1);
		assert_eq!(scene.pointer.x, 50);
		assert_eq!(scene.has_rgb, 1);
		assert_eq!(scene.has_active_monitor, 1);
		assert_eq!(scene.active_monitor.id, 9);
		assert_eq!(scene.has_highlighted_window, 1);
		assert_eq!(scene.highlighted_window.window_id, 42);

		unsafe { rsnap_session_destroy(handle) };
	}

	#[test]
	fn destroy_allows_null() {
		let handle: *mut RsnapSessionHandle = ptr::null_mut();

		unsafe { rsnap_session_destroy(handle) };
	}

	#[test]
	fn ffi_freeze_request_carries_selection_payload() {
		let handle = unsafe { rsnap_session_create(default_config()) };
		let mut request = RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };

		assert_eq!(unsafe { rsnap_session_enter_live(handle) }, RsnapStatus::Ok);
		let _ = unsafe { rsnap_session_take_next_request(handle, &mut request) };

		assert_eq!(
			unsafe {
				rsnap_session_handle_host_event(
					handle,
					RsnapHostEvent {
						kind: RsnapHostEventKind::PrimaryInteractionCompleted as u32,
						point: RsnapPoint { x: 80, y: 110 },
						has_point: 1,
						rgb: RsnapRgb::default(),
						has_rgb: 0,
						active_monitor: RsnapMonitorRect {
							id: 9,
							origin: RsnapPoint { x: 0, y: 0 },
							width: 1440,
							height: 900,
							scale_factor_x1000: 2_000,
						},
						has_active_monitor: 1,
						highlighted_window: RsnapWindowRect {
							window_id: 42,
							has_window_id: 1,
							x: 20,
							y: 30,
							width: 60,
							height: 80,
						},
						has_highlighted_window: 1,
						toolbar_item_kind: 0,
					},
				)
			},
			RsnapStatus::Ok
		);
		assert_eq!(
			unsafe { rsnap_session_take_next_request(handle, &mut request) },
			RsnapStatus::Ok
		);
		assert_eq!(request.kind, RsnapHostRequestKind::RequestFreezeSnapshot as u32);
		assert_eq!(request.has_selection, 1);
		assert_eq!(request.selection, RsnapRect { x: 20, y: 30, width: 60, height: 80 });

		unsafe { rsnap_session_destroy(handle) };
	}

	#[test]
	fn abi_version_matches_constant() {
		assert_eq!(rsnap_host_ffi_abi_version(), RSNAP_HOST_FFI_ABI_VERSION);
	}
}
