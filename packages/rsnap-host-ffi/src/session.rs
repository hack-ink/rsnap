use std::ptr::{self, NonNull};

use crate::abi::{
	RSNAP_STATUS_MESSAGE_CAPACITY, RSNAP_TOOLBAR_ITEM_CAPACITY, RsnapCursorIntent,
	RsnapHostEffectKind, RsnapHostEvent, RsnapHostEventKind, RsnapHostReport, RsnapHostReportKind,
	RsnapHostRequestKind, RsnapHostRequestValue, RsnapMonitorRect, RsnapPermissionKind,
	RsnapPlatformTag, RsnapPoint, RsnapRect, RsnapRgb, RsnapSceneKind, RsnapSceneModel,
	RsnapSessionConfig, RsnapSessionHandle, RsnapStatus, RsnapToolbarItem, RsnapToolbarItemKind,
	RsnapWindowRect,
};
use rsnap_capture_core::{
	CaptureMode, CaptureSessionCore, CursorIntent, GlobalRect, HostEffectKind, HostEvent,
	HostReport, HostRequest, PermissionKind, PlatformTag, Rgb, SceneModel, SessionConfig,
	ToolbarItemKind, ToolbarItemModel, WindowRect,
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

unsafe fn handle_mut<'a>(handle: *mut RsnapSessionHandle) -> Option<&'a mut RsnapSessionHandle> {
	unsafe { handle.as_mut() }
}

unsafe fn handle_ref<'a>(handle: *const RsnapSessionHandle) -> Option<&'a RsnapSessionHandle> {
	unsafe { handle.as_ref() }
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
				item: crate::decode_toolbar_item_kind(event.toolbar_item_kind),
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
