use crate::abi::{
	RSNAP_STATUS_MESSAGE_CAPACITY, RSNAP_TOOLBAR_ITEM_CAPACITY, RsnapMonitorRect, RsnapPoint,
	RsnapRect, RsnapRgb, RsnapWindowRect,
};

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
