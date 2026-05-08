//! Thin C ABI bridge for the native-host reset.
//!
//! The ABI surface is intentionally small in the first landing slice. It proves the
//! new host/core direction with an opaque session handle, FFI-safe config/event
//! structs, and copy-out scene/request snapshots.

use std::mem;
use std::ptr::{self, NonNull};
use std::slice;

#[cfg(not(target_os = "macos"))]
use rsnap_overlay as _;

use rsnap_capture_core::SceneModel;
use rsnap_capture_core::{
	self, CaptureFrameBackgroundKind, CaptureFrameBackgroundPlan, CaptureFrameColorStop,
	CaptureFramePlan, CaptureFrameShadow, CaptureFrameSourceKind, CaptureMode, CaptureSessionCore,
	CursorIntent, DisplayPointRect, GlobalRect, HostEffectKind, HostEvent, HostReport, HostRequest,
	PermissionKind, PlatformTag, RectPoints, Rgb, RgbaExportImage, SessionConfig, ToolbarItemKind,
	ToolbarItemModel, WindowRect,
};
#[cfg(target_os = "macos")]
use rsnap_overlay::host_live_sampling_macos::HostMacLiveSampler;
use rsnap_overlay::scroll_stitching::{
	ScrollStitchImage, ScrollStitchObserveOutcome, ScrollStitchSession,
};

/// ABI version exported by the thin C host bridge.
pub const RSNAP_HOST_FFI_ABI_VERSION: u32 = 23;

const RSNAP_TOOLBAR_ITEM_CAPACITY: usize = 16;
const RSNAP_STATUS_MESSAGE_CAPACITY: usize = 256;
const RSNAP_LIVE_SAMPLE_PATCH_CAPACITY: usize = 4_096;

/// Opaque session handle owned by the native host through the C ABI.
pub struct RsnapSessionHandle {
	session: CaptureSessionCore,
}

/// Opaque scroll-capture stitching handle owned by the native host through the C ABI.
pub struct RsnapScrollSessionHandle {
	session: ScrollStitchSession,
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
	/// The provided input payload was invalid.
	InvalidInput = 4,
}

/// FFI-safe live cursor sample copied out of the native Rust sampler.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapLiveSample {
	/// Sampled RGB value.
	pub rgb: RsnapRgb,
	/// Non-zero when `rgb` is present.
	pub has_rgb: u8,
	/// Non-zero when frame provenance fields are present.
	pub has_frame_metadata: u8,
	/// Age of the sampled ScreenCaptureKit frame in microseconds.
	pub frame_age_micros: u64,
	/// Monotonic sequence of the sampled ScreenCaptureKit frame.
	pub frame_seq: u64,
	/// Live stream generation that produced the sampled frame.
	pub stream_generation: u64,
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
			has_frame_metadata: 0,
			frame_age_micros: 0,
			frame_seq: 0,
			stream_generation: 0,
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

/// FFI-safe owned byte buffer retained by Rust until explicitly freed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapOwnedBytes {
	/// Byte count in `bytes`.
	pub len: usize,
	/// Reserved buffer capacity in bytes.
	pub capacity: usize,
	/// Owned byte buffer.
	pub bytes: *mut u8,
}
impl Default for RsnapOwnedBytes {
	fn default() -> Self {
		Self { len: 0, capacity: 0, bytes: ptr::null_mut() }
	}
}

/// FFI-safe pixel-space rectangle.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapPixelRect {
	/// Left coordinate in pixels.
	pub x: u32,
	/// Top coordinate in pixels.
	pub y: u32,
	/// Rectangle width in pixels.
	pub width: u32,
	/// Rectangle height in pixels.
	pub height: u32,
}

/// FFI-safe display-space rectangle.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RsnapFloatRect {
	/// Left coordinate in display points.
	pub x: f64,
	/// Top coordinate in display points.
	pub y: f64,
	/// Rectangle width in display points.
	pub width: f64,
	/// Rectangle height in display points.
	pub height: f64,
}

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

/// FFI-safe scroll-capture observation discriminator.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapScrollObserveOutcomeKind {
	/// The candidate did not change committed output.
	NoChange = 0,
	/// Preview-only state changed.
	PreviewUpdated = 1,
	/// Downward growth was committed.
	Committed = 2,
	/// The candidate proved motion in a direction not appended by this wrapper.
	UnsupportedDirection = 3,
}

/// FFI-safe result for one scroll-capture frame observation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapScrollObserveResult {
	/// Observation outcome.
	pub kind: u32,
	/// Appended row count when `kind` is committed.
	pub growth_rows: u32,
	/// Current committed export width in pixels.
	pub export_width: u32,
	/// Current committed export height in pixels.
	pub export_height: u32,
	/// Current committed viewport top in pixels.
	pub current_viewport_top_y: i32,
}
impl Default for RsnapScrollObserveResult {
	fn default() -> Self {
		Self {
			kind: RsnapScrollObserveOutcomeKind::NoChange as u32,
			growth_rows: 0,
			export_width: 0,
			export_height: 0,
			current_viewport_top_y: 0,
		}
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
	/// Start native scroll capture.
	StartScrollCapture = 9,
}

/// FFI-safe queued host request.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapHostRequestValue {
	/// Request kind.
	pub kind: u32,
	/// Optional selection payload for frozen handoff requests.
	pub selection: RsnapRect,
	/// Non-zero when `selection` is populated.
	pub has_selection: u8,
	/// Non-zero when the frozen selection may be moved or resized after commit.
	pub selection_editable: u8,
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
	let export = handle.session.export_image();

	unsafe {
		ptr::write(out_result, encode_scroll_observe_result(outcome, &export, &handle.session));
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
	let export = handle.session.export_image();
	let kind = if did_undo {
		ScrollStitchObserveOutcome::PreviewUpdated
	} else {
		ScrollStitchObserveOutcome::NoChange
	};

	unsafe {
		ptr::write(out_result, encode_scroll_observe_result(kind, &export, &handle.session));
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

fn encode_capture_frame_plan(plan: CaptureFramePlan) -> RsnapCaptureFramePlan {
	RsnapCaptureFramePlan {
		canvas_width: plan.canvas_width,
		canvas_height: plan.canvas_height,
		image_rect: encode_float_rect(plan.image_rect),
		corner_radius: plan.corner_radius,
		shadows: plan.shadows.map(encode_capture_frame_shadow),
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
	export: &ScrollStitchImage,
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
		export_width: export.width,
		export_height: export.height,
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
mod tests {
	use std::ptr;

	use crate::{
		RSNAP_HOST_FFI_ABI_VERSION, RSNAP_STATUS_MESSAGE_CAPACITY, RsnapCaptureFrameBackgroundKind,
		RsnapCaptureFrameBackgroundPlan, RsnapCaptureFrameColorStop, RsnapCaptureFramePlan,
		RsnapCaptureFrameSourceKind, RsnapCursorIntent, RsnapFloatRect, RsnapHostEvent,
		RsnapHostEventKind, RsnapHostReport, RsnapHostReportKind, RsnapHostRequestKind,
		RsnapHostRequestValue, RsnapMonitorRect, RsnapOwnedBytes, RsnapPixelRect, RsnapPlatformTag,
		RsnapPoint, RsnapRect, RsnapRgb, RsnapSceneKind, RsnapSceneModel, RsnapSessionConfig,
		RsnapSessionHandle, RsnapStatus, RsnapWindowRect,
	};
	#[cfg(target_os = "macos")]
	use crate::{RsnapOwnedRgbaRegion, RsnapScrollObserveOutcomeKind, RsnapScrollObserveResult};

	fn default_config() -> RsnapSessionConfig {
		RsnapSessionConfig {
			platform: RsnapPlatformTag::MacOS,
			allow_text_input: 1,
			prefers_toolbar_above_selection: 0,
		}
	}

	fn scroll_frame(width: u32, height: u32, top_row: u32) -> Vec<u8> {
		let mut rgba = Vec::with_capacity((width * height * 4) as usize);

		for y in 0..height {
			let document_row = top_row + y;

			for x in 0..width {
				rgba.push(((document_row * 17 + x * 13) % 251) as u8);
				rgba.push(((document_row * 29 + x * 7) % 251) as u8);
				rgba.push(((document_row * 5 + x * 31) % 251) as u8);
				rgba.push(255);
			}
		}

		rgba
	}

	fn png_dimensions(png: &[u8]) -> (u32, u32) {
		assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
		let width = u32::from_be_bytes(png[16..20].try_into().expect("PNG width bytes"));
		let height = u32::from_be_bytes(png[20..24].try_into().expect("PNG height bytes"));

		(width, height)
	}

	#[test]
	fn ffi_session_enters_live_and_emits_request() {
		let handle = unsafe { crate::rsnap_session_create(default_config()) };
		let mut request =
			RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };
		let mut scene = RsnapSceneModel::default();

		assert_eq!(unsafe { crate::rsnap_session_enter_live(handle) }, RsnapStatus::Ok);
		assert_eq!(
			unsafe { crate::rsnap_session_take_next_request(handle, &mut request) },
			RsnapStatus::Ok
		);
		assert_eq!(request.kind, RsnapHostRequestKind::StartLiveCapture as u32);
		assert_eq!(
			unsafe { crate::rsnap_session_copy_scene_model(handle, &mut scene) },
			RsnapStatus::Ok
		);
		assert_eq!(scene.scene_kind, RsnapSceneKind::Live as u32);
		assert_eq!(scene.cursor_intent, RsnapCursorIntent::Default as u32);

		unsafe { crate::rsnap_session_destroy(handle) };
	}

	#[test]
	fn ffi_session_applies_freeze_report() {
		let handle = unsafe { crate::rsnap_session_create(default_config()) };
		let mut scene = RsnapSceneModel::default();

		assert_eq!(unsafe { crate::rsnap_session_enter_live(handle) }, RsnapStatus::Ok);

		let mut request =
			RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };
		let _ = unsafe { crate::rsnap_session_take_next_request(handle, &mut request) };

		assert_eq!(
			unsafe {
				crate::rsnap_session_handle_host_report(
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
		assert_eq!(
			unsafe { crate::rsnap_session_copy_scene_model(handle, &mut scene) },
			RsnapStatus::Ok
		);
		assert_eq!(scene.scene_kind, RsnapSceneKind::Frozen as u32);
		assert_eq!(scene.has_frozen_selection, 1);

		unsafe { crate::rsnap_session_destroy(handle) };
	}

	#[test]
	fn ffi_session_tracks_pointer_updates() {
		let handle = unsafe { crate::rsnap_session_create(default_config()) };
		let mut scene = RsnapSceneModel::default();
		let _ = unsafe { crate::rsnap_session_enter_live(handle) };
		let mut request =
			RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };
		let _ = unsafe { crate::rsnap_session_take_next_request(handle, &mut request) };

		assert_eq!(
			unsafe {
				crate::rsnap_session_handle_host_event(
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
							width: 1_440,
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
		assert_eq!(
			unsafe { crate::rsnap_session_copy_scene_model(handle, &mut scene) },
			RsnapStatus::Ok
		);
		assert_eq!(scene.has_pointer, 1);
		assert_eq!(scene.pointer.x, 50);
		assert_eq!(scene.has_rgb, 1);
		assert_eq!(scene.has_active_monitor, 1);
		assert_eq!(scene.active_monitor.id, 9);
		assert_eq!(scene.has_highlighted_window, 1);
		assert_eq!(scene.highlighted_window.window_id, 42);

		unsafe { crate::rsnap_session_destroy(handle) };
	}

	#[test]
	fn destroy_allows_null() {
		let handle: *mut RsnapSessionHandle = ptr::null_mut();

		unsafe { crate::rsnap_session_destroy(handle) };
	}

	#[test]
	fn ffi_export_rgba_to_png_returns_owned_png() {
		let rgba = scroll_frame(4, 4, 0);
		let mut png = RsnapOwnedBytes::default();

		assert_eq!(
			unsafe { crate::rsnap_export_rgba_to_png(4, 4, rgba.as_ptr(), rgba.len(), &mut png) },
			RsnapStatus::Ok
		);
		assert!(png.len > 0);
		assert_eq!(
			png_dimensions(unsafe { std::slice::from_raw_parts(png.bytes, png.len) }),
			(4, 4)
		);

		unsafe {
			crate::rsnap_owned_bytes_release(&mut png);
		}
		assert!(png.bytes.is_null());
		assert_eq!(png.len, 0);
		assert_eq!(png.capacity, 0);
	}

	#[test]
	fn ffi_export_rgba_crop_to_png_crops_dimensions() {
		let rgba = scroll_frame(4, 4, 0);
		let mut png = RsnapOwnedBytes::default();
		let crop = RsnapPixelRect { x: 1, y: 0, width: 2, height: 3 };

		assert_eq!(
			unsafe {
				crate::rsnap_export_rgba_crop_to_png(
					4,
					4,
					rgba.as_ptr(),
					rgba.len(),
					crop,
					&mut png,
				)
			},
			RsnapStatus::Ok
		);
		assert_eq!(
			png_dimensions(unsafe { std::slice::from_raw_parts(png.bytes, png.len) }),
			(2, 3)
		);

		unsafe {
			crate::rsnap_owned_bytes_release(&mut png);
		}
	}

	#[test]
	fn ffi_export_rgba_crop_to_png_rejects_out_of_bounds_crop() {
		let rgba = scroll_frame(4, 4, 0);
		let mut png = RsnapOwnedBytes::default();
		let crop = RsnapPixelRect { x: 3, y: 3, width: 2, height: 2 };

		assert_eq!(
			unsafe {
				crate::rsnap_export_rgba_crop_to_png(
					4,
					4,
					rgba.as_ptr(),
					rgba.len(),
					crop,
					&mut png,
				)
			},
			RsnapStatus::InvalidInput
		);
		assert!(png.bytes.is_null());
	}

	#[test]
	fn ffi_frozen_display_crop_rect_returns_core_pixel_rect() {
		let mut out_rect = RsnapPixelRect::default();
		let status = unsafe {
			crate::rsnap_frozen_display_crop_rect(
				2880,
				1800,
				RsnapFloatRect { x: 0.0, y: 0.0, width: 1440.0, height: 900.0 },
				RsnapFloatRect { x: 100.0, y: 200.0, width: 300.0, height: 150.0 },
				&mut out_rect,
			)
		};

		assert_eq!(status, RsnapStatus::Ok);
		assert_eq!(out_rect, RsnapPixelRect { x: 200, y: 1100, width: 600, height: 300 });
	}

	#[test]
	fn ffi_frozen_display_crop_rect_returns_empty_for_outside_selection() {
		let mut out_rect = RsnapPixelRect::default();
		let status = unsafe {
			crate::rsnap_frozen_display_crop_rect(
				200,
				200,
				RsnapFloatRect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 },
				RsnapFloatRect { x: 120.0, y: 10.0, width: 10.0, height: 20.0 },
				&mut out_rect,
			)
		};

		assert_eq!(status, RsnapStatus::Empty);
	}

	#[test]
	fn ffi_frozen_mosaic_light_privacy_patch_returns_rgba_region() {
		let mut patch = RsnapOwnedRgbaRegion::default();
		let status = unsafe {
			crate::rsnap_frozen_mosaic_light_privacy_patch_rgba(
				100,
				80,
				RsnapFloatRect { x: 4.2, y: 9.1, width: 28.4, height: 21.0 },
				&mut patch,
			)
		};

		assert_eq!(status, RsnapStatus::Ok);
		assert_eq!(patch.width, 3);
		assert_eq!(patch.height, 3);
		assert_eq!(patch.len, 36);
		let bytes = unsafe { std::slice::from_raw_parts(patch.rgba, patch.len) };
		assert_eq!(&bytes[..12], &[211, 211, 211, 255, 205, 205, 205, 255, 202, 201, 199, 255]);

		unsafe {
			crate::rsnap_owned_rgba_region_release(&mut patch);
		}
	}

	#[test]
	fn ffi_frozen_mosaic_light_privacy_patch_returns_empty_for_outside_rect() {
		let mut patch = RsnapOwnedRgbaRegion::default();
		let status = unsafe {
			crate::rsnap_frozen_mosaic_light_privacy_patch_rgba(
				100,
				80,
				RsnapFloatRect { x: 120.0, y: 10.0, width: 10.0, height: 20.0 },
				&mut patch,
			)
		};

		assert_eq!(status, RsnapStatus::Empty);
	}

	#[test]
	fn ffi_capture_frame_plan_returns_core_geometry() {
		let mut plan = RsnapCaptureFramePlan::default();
		let status = unsafe {
			crate::rsnap_capture_frame_plan(
				320,
				180,
				2.0,
				RsnapCaptureFrameSourceKind::Window,
				&mut plan,
			)
		};

		assert_eq!(status, RsnapStatus::Ok);
		assert_eq!(plan.canvas_width, 416.0);
		assert_eq!(plan.canvas_height, 276.0);
		assert_eq!(
			plan.image_rect,
			RsnapFloatRect { x: 48.0, y: 48.0, width: 320.0, height: 180.0 }
		);
		assert_eq!(plan.corner_radius, 9.9);
		assert_eq!(plan.shadows[0].blur, 80.0);
		assert_eq!(plan.shadows[1].offset_y, -22.0);
	}

	#[test]
	fn ffi_capture_frame_aspect_fill_crop_rect_returns_core_rect() {
		let mut rect = RsnapFloatRect::default();
		let status = unsafe {
			crate::rsnap_capture_frame_aspect_fill_crop_rect(1600, 900, 1000.0, 1000.0, &mut rect)
		};

		assert_eq!(status, RsnapStatus::Ok);
		assert_eq!(rect, RsnapFloatRect { x: 350.0, y: 0.0, width: 900.0, height: 900.0 });
	}

	#[test]
	fn ffi_capture_frame_background_plan_returns_core_preset() {
		let mut plan = RsnapCaptureFrameBackgroundPlan::default();
		let status = unsafe {
			crate::rsnap_capture_frame_background_plan(
				RsnapCaptureFrameBackgroundKind::Graphite,
				&mut plan,
			)
		};

		assert_eq!(status, RsnapStatus::Ok);
		assert_eq!(plan.prefers_wallpaper, 0);
		assert_eq!(plan.wallpaper_overlay_alpha, 0.0);
		assert_eq!(plan.locations, [0.0, 0.54, 1.0]);
		assert_eq!(
			plan.colors[0],
			RsnapCaptureFrameColorStop { red: 0.08, green: 0.09, blue: 0.11, alpha: 1.0 }
		);
		assert_eq!(
			plan.colors[2],
			RsnapCaptureFrameColorStop { red: 0.56, green: 0.59, blue: 0.64, alpha: 1.0 }
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn ffi_scroll_session_observes_downward_frame_and_exports() {
		let base = scroll_frame(16, 96, 0);
		let moved = scroll_frame(16, 96, 24);
		let handle =
			unsafe { crate::rsnap_scroll_session_create(16, 96, base.as_ptr(), base.len(), 16) };

		assert!(!handle.is_null());

		let mut result = RsnapScrollObserveResult::default();
		let observe_status = unsafe {
			crate::rsnap_scroll_session_observe_downward_frame(
				handle,
				16,
				96,
				moved.as_ptr(),
				moved.len(),
				&mut result,
			)
		};

		assert_eq!(observe_status, RsnapStatus::Ok);
		assert_eq!(result.kind, RsnapScrollObserveOutcomeKind::Committed as u32);
		assert_eq!(result.growth_rows, 24);
		assert_eq!(result.export_width, 16);
		assert_eq!(result.export_height, 120);
		assert_eq!(result.current_viewport_top_y, 24);

		let mut export = RsnapOwnedRgbaRegion::default();

		assert_eq!(
			unsafe { crate::rsnap_scroll_session_take_export_rgba(handle, &mut export) },
			RsnapStatus::Ok
		);
		assert_eq!(export.width, 16);
		assert_eq!(export.height, 120);
		assert_eq!(export.len, 16 * 120 * 4);

		unsafe {
			crate::rsnap_owned_rgba_region_release(&mut export);
			crate::rsnap_scroll_session_destroy(handle);
		}
	}

	#[test]
	fn ffi_click_freeze_request_carries_fixed_selection_payload() {
		let handle = unsafe { crate::rsnap_session_create(default_config()) };
		let mut request =
			RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };

		assert_eq!(unsafe { crate::rsnap_session_enter_live(handle) }, RsnapStatus::Ok);

		let _ = unsafe { crate::rsnap_session_take_next_request(handle, &mut request) };

		assert_eq!(
			unsafe {
				crate::rsnap_session_handle_host_event(
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
							width: 1_440,
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
			unsafe { crate::rsnap_session_take_next_request(handle, &mut request) },
			RsnapStatus::Ok
		);
		assert_eq!(request.kind, RsnapHostRequestKind::RequestFreezeSnapshot as u32);
		assert_eq!(request.has_selection, 1);
		assert_eq!(request.selection, RsnapRect { x: 20, y: 30, width: 60, height: 80 });
		assert_eq!(request.selection_editable, 0);

		unsafe { crate::rsnap_session_destroy(handle) };
	}

	#[test]
	fn ffi_drag_freeze_request_carries_editable_selection_payload() {
		let handle = unsafe { crate::rsnap_session_create(default_config()) };
		let mut request =
			RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };

		assert_eq!(unsafe { crate::rsnap_session_enter_live(handle) }, RsnapStatus::Ok);

		let _ = unsafe { crate::rsnap_session_take_next_request(handle, &mut request) };

		assert_eq!(
			unsafe {
				crate::rsnap_session_handle_host_event(
					handle,
					RsnapHostEvent {
						kind: RsnapHostEventKind::PrimaryInteractionStarted as u32,
						point: RsnapPoint { x: 80, y: 110 },
						has_point: 1,
						rgb: RsnapRgb::default(),
						has_rgb: 0,
						active_monitor: RsnapMonitorRect {
							id: 9,
							origin: RsnapPoint { x: 0, y: 0 },
							width: 1_440,
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
			unsafe {
				crate::rsnap_session_handle_host_event(
					handle,
					RsnapHostEvent {
						kind: RsnapHostEventKind::PrimaryInteractionCompleted as u32,
						point: RsnapPoint { x: 140, y: 190 },
						has_point: 1,
						rgb: RsnapRgb::default(),
						has_rgb: 0,
						active_monitor: RsnapMonitorRect {
							id: 9,
							origin: RsnapPoint { x: 0, y: 0 },
							width: 1_440,
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
			unsafe { crate::rsnap_session_take_next_request(handle, &mut request) },
			RsnapStatus::Ok
		);
		assert_eq!(request.kind, RsnapHostRequestKind::RequestFreezeSnapshot as u32);
		assert_eq!(request.has_selection, 1);
		assert_eq!(request.selection, RsnapRect { x: 80, y: 110, width: 60, height: 80 });
		assert_eq!(request.selection_editable, 1);

		unsafe { crate::rsnap_session_destroy(handle) };
	}

	#[test]
	fn abi_version_matches_constant() {
		assert_eq!(crate::rsnap_host_ffi_abi_version(), RSNAP_HOST_FFI_ABI_VERSION);
	}
}
