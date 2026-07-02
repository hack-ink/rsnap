//! FFI-safe ABI types and constants shared by the native-host bridge.

use std::os::raw::c_char;
use std::ptr;

use rsnap_capture_core::CaptureSessionCore;
use rsnap_overlay::frozen_edit::FrozenOverlayEditSession;
#[cfg(target_os = "macos")]
use rsnap_overlay::host_live_sampling_macos::HostMacLiveSampler;
use rsnap_overlay::scroll_stitching::ScrollStitchSession;

/// ABI version exported by the thin C host bridge.
pub const RSNAP_HOST_FFI_ABI_VERSION: u32 = 36;

/// Maximum frozen toolbar items copied into one scene snapshot.
pub(crate) const RSNAP_TOOLBAR_ITEM_CAPACITY: usize = 16;
/// Maximum UTF-8 status-message bytes copied into fixed-size ABI payloads.
pub(crate) const RSNAP_STATUS_MESSAGE_CAPACITY: usize = 256;
/// Maximum live-sampler loupe patch bytes copied into one cursor sample.
pub(crate) const RSNAP_LIVE_SAMPLE_PATCH_CAPACITY: usize = 4_096;

/// Opaque session handle owned by the native host through the C ABI.
pub struct RsnapSessionHandle {
	pub(crate) session: CaptureSessionCore,
}

/// Opaque scroll-capture stitching handle owned by the native host through the C ABI.
pub struct RsnapScrollSessionHandle {
	pub(crate) session: ScrollStitchSession,
}

/// Opaque frozen-overlay edit handle owned by the native host through the C ABI.
pub struct RsnapFrozenOverlayEditSessionHandle {
	pub(crate) session: FrozenOverlayEditSession,
}

#[cfg(target_os = "macos")]
/// Opaque live-sampler handle owned by the native host through the C ABI.
pub struct RsnapLiveSamplerHandle {
	pub(crate) sampler: HostMacLiveSampler,
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

/// FFI-safe display-space point.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RsnapFloatPoint {
	/// X coordinate in display points.
	pub x: f64,
	/// Y coordinate in display points.
	pub y: f64,
}

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

/// FFI-safe capture-frame render mode.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapCaptureFrameRenderKind {
	/// Draw shadows and rounded clipping around the capture.
	FramedCapture = 0,
	/// Draw a floating full-window snapshot without added clipping.
	WindowSnapshot = 1,
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

/// FFI-safe platform wallpaper thumbnail request.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RsnapCaptureFrameWallpaperRequest {
	/// Maximum thumbnail dimension requested from the platform image pipeline.
	pub target_pixel_size: u32,
	/// Overlay alpha applied after drawing the wallpaper thumbnail.
	pub overlay_alpha: f64,
}

/// FFI-safe scroll-capture minimap layout plan.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RsnapScrollMinimapPlan {
	/// Outer minimap frame.
	pub frame: RsnapFloatRect,
	/// Preview image frame inside `frame`.
	pub image_frame: RsnapFloatRect,
	/// Non-zero when `viewport_frame` contains a visible marker.
	pub has_viewport_frame: u8,
	/// Viewport marker frame inside `image_frame`.
	pub viewport_frame: RsnapFloatRect,
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
