#[cfg(target_os = "macos")]
use std::sync::Arc;

use image::RgbaImage;
#[cfg(target_os = "macos")]
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::ElementState;
#[cfg(target_os = "macos")]
use winit::event::MouseButton;
#[cfg(target_os = "macos")]
use winit::event::WindowEvent;

#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::MacLiveFrameStream;
#[cfg(target_os = "macos")]
use crate::overlay::DeviceCursorPointSource;
#[cfg(target_os = "macos")]
use crate::overlay::FrozenCaptureSource;
#[cfg(target_os = "macos")]
use crate::overlay::FrozenToolbarTool;
#[cfg(target_os = "macos")]
use crate::overlay::MacOSNativeCaptureInputEvent;
#[cfg(target_os = "macos")]
use crate::overlay::OverlayKeyboardInputEvent;
#[cfg(target_os = "macos")]
use crate::overlay::PENDING_CLICK_HIT_TEST_TIMEOUT;
use crate::overlay::tests;
#[cfg(target_os = "macos")]
use crate::overlay::tests::WorkerResponse;
use crate::overlay::tests::{
	Duration, GlobalPoint, HudRedrawSummary, Instant, LoupeSample, MonitorRect, MonitorRectPoints,
	OverlayMode, OverlaySession, OverlayState, Pos2, Rect, RectPoints, Rgb, Vec2, WindowRenderer,
	hud_helpers,
};
#[cfg(target_os = "macos")]
use crate::overlay::tests::{
	HUD_PILL_CORNER_RADIUS_POINTS, HudPillGeometry, Ime, Key, LiveCursorSample,
	LiveSampleApplyResult, ModifiersState, NamedKey, OverlayExit, StartupLiveRgbPlan, WindowId,
	WindowListSnapshot,
};
#[cfg(target_os = "macos")]
use crate::overlay::{FrozenGlobalHotkey, PngAction};
use crate::overlay::{LiveCaptureInteraction, LiveClickCaptureTarget, OverlayControl};
#[cfg(target_os = "macos")]
use crate::state::{WindowHit, WindowRect};

#[cfg(target_os = "macos")]
fn single_window_list_snapshot(
	monitor: MonitorRect,
	capture_rect: RectPoints,
	window_id: u32,
) -> Arc<WindowListSnapshot> {
	Arc::new(WindowListSnapshot {
		captured_at: Instant::now(),
		windows: Arc::new(vec![WindowRect {
			window_id: Some(window_id),
			x: i64::from(monitor.origin.x) + i64::from(capture_rect.x),
			y: i64::from(monitor.origin.y) + i64::from(capture_rect.y),
			width: i64::from(capture_rect.width),
			height: i64::from(capture_rect.height),
		}]),
	})
}

#[cfg(target_os = "macos")]
#[test]
fn apply_live_cursor_sample_updates_rgb_and_loupe_state() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let cursor = GlobalPoint::new(120, 180);
	let patch = RgbaImage::from_pixel(3, 3, crate::overlay::tests::Rgba([10, 20, 30, 255]));
	let mut session = OverlaySession::new();

	session.cursor_monitor = Some(monitor);
	session.state.cursor = Some(cursor);
	session.state.alt_held = true;

	assert!(
		session
			.apply_live_cursor_sample_detail(
				monitor,
				cursor,
				LiveCursorSample { rgb: Some(Rgb::new(10, 20, 30)), patch: Some(patch.clone()) },
			)
			.any_changed()
	);
	assert_eq!(session.state.rgb, Some(Rgb::new(10, 20, 30)));
	assert_eq!(session.state.loupe.as_ref().map(|loupe| loupe.center), Some(cursor));
	assert_eq!(
		session.state.loupe.as_ref().map(|loupe| loupe.patch.dimensions()),
		Some(patch.dimensions())
	);
}

#[test]
fn frozen_toolbar_cursor_event_updates_frozen_cursor_context() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let cursor = GlobalPoint::new(120, 180);
	let patch = RgbaImage::from_pixel(400, 300, crate::overlay::tests::Rgba([10, 20, 30, 255]));
	let mut session = OverlaySession::new();

	session.state.commit_frozen_display_image(monitor, patch.clone());

	session.state.alt_held = true;

	session.note_frozen_toolbar_cursor_event(monitor, cursor);

	assert_eq!(session.last_event_cursor, Some((monitor, cursor)));
	assert_eq!(session.state.cursor, Some(cursor));
	assert_eq!(session.state.rgb, None);
	assert!(session.state.loupe.is_none());
}

#[test]
fn frozen_cursor_tracking_keeps_toolbar_hover_cursor_without_resampling_device_position() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let expected_cursor = GlobalPoint::new(540, 420);
	let stale_cursor = GlobalPoint::new(7, 8);
	let mut session = OverlaySession::new();

	session.session_active = true;
	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.toolbar_state.visible = true;
	session.toolbar_pointer_local = Some(Pos2::new(12.0, 10.0));
	session.state.cursor = Some(stale_cursor);
	session.cursor_monitor = Some(monitor);
	session.last_event_cursor = Some((monitor, expected_cursor));
	session.last_event_cursor_at = Some(Instant::now() - Duration::from_millis(500));
	session.last_frozen_cursor_poll_at = Instant::now() - Duration::from_secs(1);

	session.maybe_tick_frozen_cursor_tracking();

	assert_eq!(session.state.cursor, Some(expected_cursor));
	assert_eq!(session.cursor_monitor, Some(monitor));
}

#[cfg(target_os = "macos")]
#[test]
fn native_toolbar_pointer_move_starts_manual_drag_without_winit_window_events() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.session_active = true;
	session.state.monitor = Some(monitor);
	session.toolbar_state.visible = true;
	session.toolbar_window_visible = true;
	session.toolbar_left_button_down = true;
	session.toolbar_state.drag_start_eligible = true;
	session.toolbar_state.drag_anchor = Some(Pos2::new(8.0, 8.0));
	session.toolbar_outer_pos = Some(GlobalPoint::new(160, 140));

	assert!(matches!(
		session.handle_native_toolbar_pointer_moved(
			monitor,
			Pos2::new(40.0, 26.0),
			GlobalPoint::new(200, 166),
			Some(GlobalPoint::new(160, 140)),
		),
		OverlayControl::Continue
	));
	assert!(session.toolbar_state.dragging);
	assert_eq!(session.toolbar_pointer_local, Some(Pos2::new(40.0, 26.0)));
	assert_eq!(session.state.cursor, Some(GlobalPoint::new(200, 166)));
	assert!(session.pending_toolbar_outer_pos.is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn native_capture_input_ready_routes_toolbar_pointer_left_without_window_id() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.toolbar_state.visible = true;
	session.toolbar_pointer_local = Some(Pos2::new(12.0, 10.0));

	assert!(matches!(
		session.handle_native_capture_input_event(MacOSNativeCaptureInputEvent::ToolbarPointerLeft),
		OverlayControl::Continue
	));
	assert_eq!(session.toolbar_pointer_local, None);
}

#[cfg(target_os = "macos")]
#[test]
fn native_overlay_pointer_move_updates_live_hover_target_without_window_events() {
	let monitor = tests::test_monitor();
	let cursor = GlobalPoint::new(180, 220);
	let capture_rect = RectPoints::new(100, 120, 240, 320);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.state.monitor = Some(monitor);
	session.window_list_snapshot = Some(single_window_list_snapshot(monitor, capture_rect, 42));

	assert!(matches!(
		session.handle_native_capture_input_event(
			MacOSNativeCaptureInputEvent::OverlayPointerMoved { monitor, global: cursor }
		),
		OverlayControl::Continue
	));
	assert_eq!(session.state.cursor, Some(cursor));
	assert_eq!(session.last_event_cursor, Some((monitor, cursor)));
	assert_eq!(
		session.state.hovered_window_rect,
		Some(MonitorRectPoints { monitor_id: monitor.id, rect: capture_rect })
	);
	assert!(matches!(
		session.live_capture_interaction,
		LiveCaptureInteraction::HoverWindow {
			monitor: hover_monitor,
			target: LiveClickCaptureTarget {
				capture_rect: Some(target_rect),
				window_target: Some(window_target),
			},
		} if hover_monitor == monitor && target_rect == capture_rect && window_target.window_id == 42
	));
}

#[cfg(target_os = "macos")]
#[test]
fn native_overlay_click_uses_locked_target_and_finishes_display_first_handoff() {
	let monitor = tests::test_monitor();
	let press_global = GlobalPoint::new(180, 220);
	let release_global = GlobalPoint::new(181, 221);
	let capture_rect = RectPoints::new(100, 120, 240, 320);
	let (mut session, _original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.state.mode = OverlayMode::Live;
	session.state.monitor = Some(monitor);
	session.window_list_snapshot = Some(single_window_list_snapshot(monitor, capture_rect, 42));

	assert!(matches!(
		session.handle_native_capture_input_event(
			MacOSNativeCaptureInputEvent::OverlayMouseInput {
				monitor,
				global: press_global,
				button: MouseButton::Left,
				state: ElementState::Pressed,
			}
		),
		OverlayControl::Continue
	));

	// The live click target must stay locked to the mouse-down snapshot even if the cached
	// window list changes before release.
	session.window_list_snapshot = Some(Arc::new(WindowListSnapshot {
		captured_at: Instant::now(),
		windows: Arc::new(Vec::new()),
	}));

	assert!(matches!(
		session.handle_native_capture_input_event(
			MacOSNativeCaptureInputEvent::OverlayMouseInput {
				monitor,
				global: release_global,
				button: MouseButton::Left,
				state: ElementState::Released,
			}
		),
		OverlayControl::Continue
	));
	assert!(matches!(session.state.mode, OverlayMode::Live));
	assert_eq!(session.state.frozen_capture_rect, Some(capture_rect));
	assert_eq!(tests::session_pending_freeze_capture(&session), Some(monitor));
	assert_eq!(
		tests::session_pending_window_freeze_capture(&session),
		Some(crate::overlay::session_state::WindowFreezeCaptureTarget {
			monitor,
			window_id: 42,
			rect: capture_rect,
		})
	);
	assert!(matches!(
		session.live_capture_interaction,
		LiveCaptureInteraction::FrozenFromClick {
			monitor: frozen_monitor,
			target: LiveClickCaptureTarget {
				capture_rect: Some(target_rect),
				window_target: Some(window_target),
			},
		} if frozen_monitor == monitor && target_rect == capture_rect && window_target.window_id == 42
	));

	session
		.live_sample_stream
		.as_ref()
		.unwrap()
		.debug_set_self_capture_filter_complete(monitor.id, true);
	session.live_sample_stream.as_ref().unwrap().debug_store_test_snapshot_with_metadata(
		monitor,
		1,
		1,
		tests::fresh_live_stream_snapshot_captured_at(),
	);

	let _ = session.about_to_wait();

	assert!(matches!(session.state.mode, OverlayMode::Frozen));
	assert_eq!(session.state.frozen_capture_rect, Some(capture_rect));
	assert!(session.state.frozen_display_image.is_some());
	assert!(session.state.frozen_export_image.is_some());
	assert!(tests::session_export_authority_ready(&session));
	assert!(!session.capture_windows_hidden);
}

#[cfg(target_os = "macos")]
#[test]
fn native_overlay_drag_selection_commits_display_first_frozen_entry() {
	let monitor = tests::test_monitor();
	let press_global = GlobalPoint::new(180, 220);
	let drag_global = GlobalPoint::new(420, 460);
	let capture_rect = monitor
		.local_rect_from_points(press_global, drag_global)
		.expect("drag should produce a capture rect");
	let (mut session, _original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.state.mode = OverlayMode::Live;
	session.state.monitor = Some(monitor);

	session
		.live_sample_stream
		.as_ref()
		.unwrap()
		.debug_set_self_capture_filter_complete(monitor.id, true);
	session.live_sample_stream.as_ref().unwrap().debug_store_test_snapshot_with_metadata(
		monitor,
		2,
		1,
		tests::fresh_live_stream_snapshot_captured_at(),
	);

	assert!(matches!(
		session.handle_native_capture_input_event(
			MacOSNativeCaptureInputEvent::OverlayMouseInput {
				monitor,
				global: press_global,
				button: MouseButton::Left,
				state: ElementState::Pressed,
			}
		),
		OverlayControl::Continue
	));
	assert!(matches!(
		session.handle_native_capture_input_event(
			MacOSNativeCaptureInputEvent::OverlayPointerMoved { monitor, global: drag_global }
		),
		OverlayControl::Continue
	));
	assert_eq!(
		session.state.drag_rect,
		Some(MonitorRectPoints { monitor_id: monitor.id, rect: capture_rect })
	);
	assert!(matches!(
		session.handle_native_capture_input_event(
			MacOSNativeCaptureInputEvent::OverlayMouseInput {
				monitor,
				global: drag_global,
				button: MouseButton::Left,
				state: ElementState::Released,
			}
		),
		OverlayControl::Continue
	));
	assert!(matches!(session.state.mode, OverlayMode::Frozen));
	assert_eq!(session.state.frozen_capture_rect, Some(capture_rect));
	assert!(session.state.frozen_display_image.is_some());
	assert!(session.state.frozen_export_image.is_some());
	assert!(tests::session_export_authority_ready(&session));
	assert!(!session.capture_windows_hidden);
}

#[cfg(target_os = "macos")]
#[test]
fn native_capture_input_ready_routes_keyboard_input_to_frozen_text_edit() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(matches!(
		session.handle_native_capture_input_event(MacOSNativeCaptureInputEvent::KeyboardInput {
			monitor: Some(monitor),
			event: OverlayKeyboardInputEvent {
				logical_key: Key::Character(String::from("A").into()),
				text: Some(String::from("A")),
				state: ElementState::Pressed,
				repeat: false,
			},
		}),
		OverlayControl::Continue
	));
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some("A"));
}

#[cfg(target_os = "macos")]
#[test]
fn native_capture_input_ready_routes_ime_preedit_to_frozen_text_edit() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(matches!(
		session.handle_native_capture_input_event(MacOSNativeCaptureInputEvent::Ime {
			monitor: Some(monitor),
			event: Ime::Preedit(String::from("汉"), Some((0, 0))),
		}),
		OverlayControl::Continue
	));
	assert_eq!(
		session.frozen_text_edit.as_ref().and_then(|edit| edit.ime_preedit.as_deref()),
		Some("汉")
	);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_winit_ime_events_do_not_mutate_frozen_text_edit() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(matches!(
		session.handle_window_event(
			WindowId::from(7),
			&WindowEvent::Ime(Ime::Commit(String::from("A")))
		),
		OverlayControl::Continue
	));
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some(""));
}

#[cfg(target_os = "macos")]
#[test]
fn native_capture_input_ready_routes_scroll_capture_escape_without_winit_window_events() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.scroll_capture.active = true;

	assert!(matches!(
		session.handle_native_capture_input_event(MacOSNativeCaptureInputEvent::KeyboardInput {
			monitor: Some(monitor),
			event: OverlayKeyboardInputEvent {
				logical_key: Key::Named(NamedKey::Escape),
				text: None,
				state: ElementState::Pressed,
				repeat: false,
			},
		}),
		OverlayControl::Exit(OverlayExit::Cancelled)
	));
}

#[cfg(target_os = "macos")]
#[test]
fn apply_live_cursor_sample_detail_keeps_overlay_redraw_narrow_for_rgb_and_loupe_updates() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let cursor = GlobalPoint::new(120, 180);
	let patch = RgbaImage::from_pixel(3, 3, crate::overlay::tests::Rgba([10, 20, 30, 255]));
	let mut session = OverlaySession::new();

	session.cursor_monitor = Some(monitor);
	session.state.cursor = Some(cursor);
	session.state.alt_held = true;

	let apply = session.apply_live_cursor_sample_detail(
		monitor,
		cursor,
		LiveCursorSample { rgb: Some(Rgb::new(10, 20, 30)), patch: Some(patch) },
	);

	assert_eq!(
		apply,
		LiveSampleApplyResult { overlay_changed: false, hud_changed: true, loupe_changed: true }
	);
}

#[cfg(target_os = "macos")]
#[test]
fn live_sample_request_redraw_intent_only_redraws_immediate_hover_changes() {
	let session = OverlaySession::new();

	assert_eq!(
		session.live_sample_request_redraw_intent(false, true, true),
		LiveSampleApplyResult::default()
	);
	assert_eq!(
		session.live_sample_request_redraw_intent(true, true, true),
		LiveSampleApplyResult { overlay_changed: true, hud_changed: true, loupe_changed: false }
	);
}

#[cfg(target_os = "macos")]
#[test]
fn apply_loupe_activation_input_toggle_ignores_release_and_repeat() {
	let mut session = OverlaySession::new();

	assert!(session.apply_loupe_activation_input(true, false));
	assert!(session.state.alt_held);
	assert!(!session.apply_loupe_activation_input(true, true));
	assert!(session.state.alt_held);
	assert!(!session.apply_loupe_activation_input(false, false));
	assert!(session.state.alt_held);
	assert!(session.apply_loupe_activation_input(true, false));
	assert!(!session.state.alt_held);
}

#[cfg(target_os = "macos")]
#[test]
fn apply_loupe_activation_input_next_press_toggles_loupe_off() {
	let mut session = OverlaySession::new();

	assert!(session.apply_loupe_activation_input(true, false));
	assert!(session.state.alt_held);
	assert!(!session.apply_loupe_activation_input(false, false));
	assert!(session.state.alt_held);
	assert!(session.apply_loupe_activation_input(true, false));
	assert!(!session.state.alt_held);
}

#[cfg(target_os = "macos")]
#[test]
fn plain_character_shortcut_available_blocks_loupe_activation_key_while_pressed() {
	let mut session = OverlaySession::new();

	assert!(session.plain_character_shortcut_available());
	assert!(session.apply_loupe_activation_key_event(true, false));
	assert!(!session.plain_character_shortcut_available());
	assert!(!session.apply_loupe_activation_key_event(false, false));
	assert!(session.state.alt_held);
	assert!(session.plain_character_shortcut_available());
}

#[cfg(target_os = "macos")]
#[test]
fn clear_loupe_activation_on_focus_loss_releases_toggle_press_without_toggling_off() {
	let mut session = OverlaySession::new();

	assert!(session.apply_loupe_activation_key_event(true, false));
	assert!(session.state.alt_held);
	assert!(session.loupe_activation_key_down);

	session.clear_loupe_activation_on_focus_loss();

	assert!(session.state.alt_held);
	assert!(!session.loupe_activation_key_down);
	assert!(session.plain_character_shortcut_available());
}

#[cfg(target_os = "macos")]
#[test]
fn clear_loupe_activation_on_focus_loss_ignores_released_toggle_state() {
	let mut session = OverlaySession::new();

	assert!(session.apply_loupe_activation_key_event(true, false));
	assert!(session.state.alt_held);
	assert!(!session.apply_loupe_activation_key_event(false, false));
	assert!(!session.loupe_activation_key_down);

	session.clear_loupe_activation_on_focus_loss();

	assert!(session.state.alt_held);
	assert!(!session.loupe_activation_key_down);
	assert!(session.plain_character_shortcut_available());
}

#[cfg(target_os = "macos")]
#[test]
fn apply_loupe_activation_key_event_ignores_frozen_mode() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;

	assert!(!session.apply_loupe_activation_key_event(true, false));
	assert!(!session.state.alt_held);
	assert!(!session.loupe_activation_key_down);
}

#[cfg(target_os = "macos")]
#[test]
fn duplicate_loupe_activation_key_events_do_not_double_toggle() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;

	assert!(session.apply_loupe_activation_key_event(true, false));
	assert!(session.state.alt_held);
	assert!(session.loupe_activation_key_down);
	assert!(!session.apply_loupe_activation_key_event(true, false));
	assert!(session.state.alt_held);
	assert!(session.loupe_activation_key_down);
	assert!(!session.apply_loupe_activation_key_event(false, false));
	assert!(session.state.alt_held);
	assert!(!session.loupe_activation_key_down);
	assert!(!session.apply_loupe_activation_key_event(false, false));
	assert!(session.state.alt_held);
	assert!(!session.loupe_activation_key_down);
}

#[test]
fn live_drag_loupe_toggle_does_not_reopen_loupe() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.state.drag_rect =
		Some(MonitorRectPoints { monitor_id: 1, rect: RectPoints::new(100, 120, 240, 320) });

	session.set_alt_held(true);

	assert!(session.state.alt_held);
	assert!(!session.loupe_window_visible);
	assert!(session.state.loupe.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn loupe_activation_shortcut_available_requires_plain_tab() {
	let mut session = OverlaySession::new();

	assert!(session.loupe_activation_shortcut_available());

	session.keyboard_modifiers = ModifiersState::SHIFT;

	assert!(!session.loupe_activation_shortcut_available());

	session.keyboard_modifiers = ModifiersState::ALT;

	assert!(!session.loupe_activation_shortcut_available());

	session.keyboard_modifiers = ModifiersState::CONTROL;

	assert!(!session.loupe_activation_shortcut_available());

	session.keyboard_modifiers = ModifiersState::SUPER;

	assert!(!session.loupe_activation_shortcut_available());
}

#[cfg(target_os = "macos")]
#[test]
fn apply_loupe_activation_key_event_tracks_modified_tab_without_activating_loupe() {
	let mut session = OverlaySession::new();

	session.keyboard_modifiers = ModifiersState::SHIFT;

	assert!(!session.apply_loupe_activation_key_event(true, false));
	assert!(session.loupe_activation_key_down);
	assert!(!session.state.alt_held);
	assert!(!session.plain_character_shortcut_available());

	session.keyboard_modifiers = ModifiersState::default();

	assert!(!session.plain_character_shortcut_available());
	assert!(!session.apply_loupe_activation_key_event(false, false));
	assert!(!session.loupe_activation_key_down);
	assert!(session.plain_character_shortcut_available());
}

#[cfg(target_os = "macos")]
#[test]
fn pending_focus_loss_cleanup_does_not_clear_loupe_during_internal_focus_transfer() {
	let overlay_window_id = WindowId::from(1);
	let toolbar_window_id = WindowId::from(2);
	let mut session = OverlaySession::new();

	session.note_window_focus_change(overlay_window_id, true);

	assert!(session.apply_loupe_activation_key_event(true, false));
	assert!(session.state.alt_held);

	session.note_window_focus_change(overlay_window_id, false);
	session.note_window_focus_change(toolbar_window_id, true);
	session.maybe_clear_loupe_activation_after_focus_loss();

	assert!(session.state.alt_held);
	assert!(session.loupe_activation_key_down);
}

#[cfg(target_os = "macos")]
#[test]
fn pending_focus_loss_cleanup_releases_toggle_press_after_all_overlay_windows_blur() {
	let overlay_window_id = WindowId::from(1);
	let mut session = OverlaySession::new();

	session.note_window_focus_change(overlay_window_id, true);

	assert!(session.apply_loupe_activation_key_event(true, false));
	assert!(session.state.alt_held);

	session.note_window_focus_change(overlay_window_id, false);
	session.maybe_clear_loupe_activation_after_focus_loss();

	assert!(session.state.alt_held);
	assert!(!session.loupe_activation_key_down);
	assert!(session.plain_character_shortcut_available());
}

#[cfg(target_os = "macos")]
#[test]
fn initial_unfocused_window_blur_does_not_cancel_first_global_loupe_press() {
	let overlay_window_id = WindowId::from(1);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;

	session.note_window_focus_change(overlay_window_id, false);

	assert!(matches!(session.handle_global_loupe_hotkey(true), OverlayControl::Continue));
	assert!(session.state.alt_held);
	assert!(session.loupe_activation_key_down);

	session.maybe_clear_loupe_activation_after_focus_loss();

	assert!(session.state.alt_held);
	assert!(session.loupe_activation_key_down);
}

#[cfg(target_os = "macos")]
#[test]
fn live_loupe_keeps_a_dedicated_window_during_live_alt() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;

	assert!(!session.live_loupe_uses_hud_window());
	assert!(!session.live_loupe_renders_in_hud_window());

	session.state.mode = OverlayMode::Live;

	assert!(!session.live_loupe_uses_hud_window());
	assert!(!session.live_loupe_renders_in_hud_window());

	session.state.alt_held = true;

	assert!(!session.live_loupe_renders_in_hud_window());

	session.state.mode = OverlayMode::Frozen;

	assert!(!session.live_loupe_uses_hud_window());
	assert!(!session.live_loupe_renders_in_hud_window());
}

#[cfg(target_os = "macos")]
#[test]
fn hud_window_content_rect_stays_compact_for_live_alt() {
	let hud_pill = HudPillGeometry {
		rect: Rect::from_min_max(Pos2::new(14.0, 14.0), Pos2::new(200.0, 58.0)),
		radius_points: f32::from(HUD_PILL_CORNER_RADIUS_POINTS),
	};
	let loupe_tile = Rect::from_min_max(Pos2::new(14.0, 68.0), Pos2::new(246.0, 300.0));
	let live_rect = OverlaySession::hud_window_content_rect(
		OverlayMode::Live,
		true,
		hud_pill,
		Some(loupe_tile),
	);

	assert_eq!(live_rect, hud_pill.rect);

	let live_rect_without_hud_loupe = OverlaySession::hud_window_content_rect(
		OverlayMode::Live,
		false,
		hud_pill,
		Some(loupe_tile),
	);

	assert_eq!(live_rect_without_hud_loupe, hud_pill.rect);

	let frozen_rect = OverlaySession::hud_window_content_rect(
		OverlayMode::Frozen,
		true,
		hud_pill,
		Some(loupe_tile),
	);

	assert_eq!(frozen_rect, hud_pill.rect);
}

#[cfg(target_os = "macos")]
#[test]
fn live_alt_loupe_window_redraw_is_not_skipped() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.state.alt_held = true;

	assert!(!session.should_skip_loupe_redraw());

	session.state.alt_held = false;

	assert!(session.should_skip_loupe_redraw());

	session.state.alt_held = true;

	session.set_live_capture_interaction(LiveCaptureInteraction::DraggingSelection {
		monitor: MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		},
		press_global: GlobalPoint::new(100, 120),
		current_global: GlobalPoint::new(340, 440),
	});

	assert!(session.should_skip_loupe_redraw());
}

#[test]
fn frozen_loupe_window_redraw_is_skipped() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.alt_held = true;

	assert!(session.should_skip_loupe_redraw());
}

#[test]
fn live_overlay_selection_flow_repaint_active_only_for_hovered_window() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.cursor_monitor = Some(monitor);
	session.state.cursor = Some(GlobalPoint::new(120, 180));

	assert!(!session.live_overlay_selection_flow_repaint_active());

	session.state.hovered_window_rect = Some(MonitorRectPoints {
		monitor_id: monitor.id,
		rect: RectPoints::new(100, 120, 240, 320),
	});

	assert!(session.live_overlay_selection_flow_repaint_active());

	session.config.selection_flow_enabled = false;

	assert!(!session.live_overlay_selection_flow_repaint_active());

	session.config.selection_flow_enabled = true;
	session.state.hovered_window_rect = Some(MonitorRectPoints {
		monitor_id: monitor.id + 1,
		rect: RectPoints::new(100, 120, 240, 320),
	});

	assert!(!session.live_overlay_selection_flow_repaint_active());

	session.state.drag_rect = Some(MonitorRectPoints {
		monitor_id: monitor.id,
		rect: RectPoints::new(100, 120, 240, 320),
	});

	assert!(!session.live_overlay_selection_flow_repaint_active());
}

#[test]
fn live_drag_focus_rect_uses_large_drag_on_active_monitor() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1_000.0, 800.0));
	let mut state = OverlayState::new();

	state.drag_rect = Some(MonitorRectPoints {
		monitor_id: monitor.id,
		rect: RectPoints::new(100, 120, 240, 320),
	});

	assert_eq!(
		WindowRenderer::live_drag_focus_rect(&state, monitor, screen_rect),
		Some(Rect::from_min_size(Pos2::new(100.0, 120.0), Vec2::new(240.0, 320.0)))
	);

	state.drag_rect = Some(MonitorRectPoints {
		monitor_id: monitor.id + 1,
		rect: RectPoints::new(100, 120, 240, 320),
	});

	assert_eq!(WindowRenderer::live_drag_focus_rect(&state, monitor, screen_rect), None);
}

#[test]
fn live_drag_rect_activation_hides_auxiliary_windows() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let start = GlobalPoint::new(120, 180);
	let end = GlobalPoint::new(280, 360);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;

	session.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
		monitor,
		press_global: start,
		click_target: None,
		release_global: None,
		released: false,
	});

	session.hud_window_visible = true;
	session.loupe_window_visible = true;

	session.update_live_drag_rect(monitor, end);

	assert_eq!(
		session.state.drag_rect,
		Some(MonitorRectPoints {
			monitor_id: monitor.id,
			rect: RectPoints::new(120, 180, 160, 180),
		})
	);
	assert!(!session.hud_window_visible);
	assert!(!session.loupe_window_visible);
}

#[test]
fn live_press_pending_activation_hides_auxiliary_windows_immediately() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let start = GlobalPoint::new(120, 180);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.hud_window_visible = true;
	session.loupe_window_visible = true;

	session.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
		monitor,
		press_global: start,
		click_target: None,
		release_global: None,
		released: false,
	});

	assert!(!session.hud_window_visible);
	assert!(!session.loupe_window_visible);
}

#[test]
fn live_frozen_handoff_activation_hides_auxiliary_windows_immediately() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(120, 180, 160, 180);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.hud_window_visible = true;
	session.loupe_window_visible = true;

	session.set_live_capture_interaction(LiveCaptureInteraction::FrozenFromDrag {
		monitor,
		capture_rect,
	});

	assert!(!session.hud_window_visible);
	assert!(!session.loupe_window_visible);
}

#[cfg(target_os = "macos")]
#[test]
fn live_mouse_press_keeps_hovered_window_affordance_until_drag_or_release() {
	let monitor = tests::test_monitor();
	let hovered =
		MonitorRectPoints { monitor_id: monitor.id, rect: RectPoints::new(100, 120, 240, 320) };
	let press_global = GlobalPoint::new(180, 220);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.state.monitor = Some(monitor);

	session.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
		monitor,
		press_global,
		click_target: Some(LiveClickCaptureTarget {
			capture_rect: Some(hovered.rect),
			window_target: None,
		}),
		release_global: None,
		released: false,
	});

	assert_eq!(session.state.hovered_window_rect, Some(hovered));
	assert!(matches!(
		session.live_capture_interaction,
		LiveCaptureInteraction::PressPending { monitor: press_monitor, .. }
			if press_monitor == monitor
	));
}

#[cfg(target_os = "macos")]
#[test]
fn live_press_pending_keeps_hovered_window_across_worker_updates() {
	let monitor = tests::test_monitor();
	let cursor = GlobalPoint::new(180, 220);
	let hovered =
		MonitorRectPoints { monitor_id: monitor.id, rect: RectPoints::new(100, 120, 240, 320) };
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.cursor_monitor = Some(monitor);
	session.state.cursor = Some(cursor);

	session.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
		monitor,
		press_global: cursor,
		click_target: Some(LiveClickCaptureTarget {
			capture_rect: Some(hovered.rect),
			window_target: None,
		}),
		release_global: None,
		released: false,
	});

	let apply = session.apply_live_cursor_sample_detail(
		monitor,
		cursor,
		LiveCursorSample { rgb: None, patch: None },
	);

	assert!(!apply.overlay_changed);
	assert_eq!(session.state.hovered_window_rect, Some(hovered));

	let control = session.maybe_tick_worker_response_limiter(WorkerResponse::RefreshedWindowList {
		snapshot: Arc::new(WindowListSnapshot {
			captured_at: Instant::now(),
			windows: Arc::new(vec![]),
		}),
	});

	assert!(matches!(control, OverlayControl::Continue));
	assert_eq!(session.state.hovered_window_rect, Some(hovered));
}

#[test]
fn live_drag_activation_clears_hovered_window_affordance() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let start = GlobalPoint::new(120, 180);
	let end = GlobalPoint::new(280, 360);
	let hovered =
		MonitorRectPoints { monitor_id: monitor.id, rect: RectPoints::new(100, 120, 240, 320) };
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;

	session.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
		monitor,
		press_global: start,
		click_target: Some(LiveClickCaptureTarget {
			capture_rect: Some(hovered.rect),
			window_target: None,
		}),
		release_global: None,
		released: false,
	});
	session.update_live_drag_rect(monitor, end);

	assert_eq!(
		session.state.drag_rect,
		Some(MonitorRectPoints {
			monitor_id: monitor.id,
			rect: RectPoints::new(120, 180, 160, 180),
		})
	);
	assert!(session.state.hovered_window_rect.is_none());
}

#[test]
fn live_press_pending_small_jitter_keeps_hovered_window_affordance() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 240, 320);
	let press_global = GlobalPoint::new(180, 220);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;

	session.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
		monitor,
		press_global,
		click_target: Some(LiveClickCaptureTarget {
			capture_rect: Some(capture_rect),
			window_target: None,
		}),
		release_global: None,
		released: false,
	});
	session.update_live_drag_rect(monitor, GlobalPoint::new(183, 224));

	assert_eq!(
		session.state.hovered_window_rect,
		Some(MonitorRectPoints { monitor_id: monitor.id, rect: capture_rect })
	);
	assert!(session.state.drag_rect.is_none());
	assert!(matches!(
		session.live_capture_interaction,
		LiveCaptureInteraction::PressPending { monitor: press_monitor, .. }
			if press_monitor == monitor
	));
}

#[test]
fn live_release_click_uses_mouse_down_locked_target() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 240, 320);
	let press_global = GlobalPoint::new(180, 220);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.state.monitor = Some(monitor);
	session.state.cursor = Some(GlobalPoint::new(420, 440));

	session.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
		monitor,
		press_global,
		click_target: Some(LiveClickCaptureTarget {
			capture_rect: Some(capture_rect),
			window_target: None,
		}),
		release_global: None,
		released: false,
	});

	assert!(matches!(
		session.handle_left_mouse_input(winit::window::WindowId::from(1), ElementState::Released),
		OverlayControl::Continue
	));
	assert!(matches!(session.state.mode, OverlayMode::Live));
	assert_eq!(session.state.frozen_capture_rect, Some(capture_rect));
	assert_eq!(tests::session_pending_freeze_capture(&session), Some(monitor));
	assert!(matches!(
		session.live_capture_interaction,
		LiveCaptureInteraction::FrozenFromClick {
			monitor: frozen_monitor,
			target: LiveClickCaptureTarget {
				capture_rect: Some(target_rect),
				..
			},
		} if frozen_monitor == monitor && target_rect == capture_rect
	));
}

#[cfg(target_os = "macos")]
#[test]
fn released_press_pending_waits_for_async_hit_test_before_entering_frozen() {
	let monitor = tests::test_monitor();
	let press_global = GlobalPoint::new(180, 220);
	let capture_rect = RectPoints::new(100, 120, 240, 320);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.state.monitor = Some(monitor);
	session.pending_click_hit_test_request_id = Some(7);

	session.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
		monitor,
		press_global,
		click_target: None,
		release_global: Some(GlobalPoint::new(420, 440)),
		released: true,
	});

	let control = session.maybe_tick_worker_response_limiter(WorkerResponse::HitTestWindow {
		monitor,
		point: press_global,
		request_id: 7,
		hit: Some(WindowHit { window_id: Some(42), rect: capture_rect }),
	});

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.pending_click_hit_test_request_id.is_none());
	assert!(matches!(session.state.mode, OverlayMode::Live));
	assert_eq!(session.state.frozen_capture_rect, Some(capture_rect));
	assert_eq!(tests::session_pending_freeze_capture(&session), Some(monitor));
	assert_eq!(
		tests::session_pending_window_freeze_capture(&session),
		Some(crate::overlay::session_state::WindowFreezeCaptureTarget {
			monitor,
			window_id: 42,
			rect: capture_rect,
		})
	);
	assert!(matches!(
		session.live_capture_interaction,
		LiveCaptureInteraction::FrozenFromClick {
			monitor: frozen_monitor,
			target: LiveClickCaptureTarget {
				capture_rect: Some(target_rect),
				window_target: Some(window_target),
			},
		} if frozen_monitor == monitor && target_rect == capture_rect && window_target.window_id == 42
	));
}

#[cfg(target_os = "macos")]
#[test]
fn timed_out_click_hit_test_unlocks_press_pending_for_retry() {
	let monitor = tests::test_monitor();
	let cursor = GlobalPoint::new(180, 220);
	let capture_rect = RectPoints::new(100, 120, 240, 320);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.state.monitor = Some(monitor);
	session.state.cursor = Some(cursor);
	session.window_list_snapshot = Some(Arc::new(WindowListSnapshot {
		captured_at: Instant::now(),
		windows: Arc::new(vec![WindowRect {
			window_id: Some(42),
			x: i64::from(monitor.origin.x) + i64::from(capture_rect.x),
			y: i64::from(monitor.origin.y) + i64::from(capture_rect.y),
			width: i64::from(capture_rect.width),
			height: i64::from(capture_rect.height),
		}]),
	}));
	session.pending_click_hit_test_request_id = Some(7);
	session.pending_click_hit_test_requested_at =
		Some(Instant::now() - PENDING_CLICK_HIT_TEST_TIMEOUT - Duration::from_millis(1));

	session.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
		monitor,
		press_global: cursor,
		click_target: None,
		release_global: Some(cursor),
		released: true,
	});

	let control = session.about_to_wait();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.pending_click_hit_test_request_id.is_none());
	assert!(session.pending_click_hit_test_requested_at.is_none());
	assert!(matches!(
		session.live_capture_interaction,
		LiveCaptureInteraction::HoverWindow {
			monitor: hover_monitor,
			target: LiveClickCaptureTarget {
				capture_rect: Some(target_rect),
				window_target: Some(window_target),
			},
		} if hover_monitor == monitor && target_rect == capture_rect && window_target.window_id == 42
	));

	session.begin_live_capture_press(monitor, cursor);

	assert!(matches!(
		session.live_capture_interaction,
		LiveCaptureInteraction::PressPending {
			monitor: press_monitor,
			click_target: Some(LiveClickCaptureTarget {
				capture_rect: Some(target_rect),
				window_target: Some(window_target),
			}),
			released: false,
			..
		} if press_monitor == monitor && target_rect == capture_rect && window_target.window_id == 42
	));
}

#[cfg(target_os = "macos")]
#[test]
fn sync_live_sample_attempt_does_not_leave_pending_request() {
	let mut session = OverlaySession::new();

	session.note_live_cursor_sample_request_started(7);

	assert!(session.live_sample_request_pending());

	session.finish_sync_live_cursor_sample_attempt(7);

	assert!(!session.live_sample_request_pending());
	assert_eq!(session.latest_live_cursor_sample_request_id, Some(7));
	assert_eq!(session.applied_live_cursor_sample_request_id, Some(7));
}

#[cfg(target_os = "macos")]
#[test]
fn request_live_samples_for_cursor_primes_stream_setup_while_startup_aux_windows_pending() {
	let monitor = tests::test_monitor();
	let cursor = GlobalPoint::new(120, 180);
	let mut session = OverlaySession::new();

	session.live_sample_stream = Some(MacLiveFrameStream::new());
	session.startup_aux_window_creation_pending = true;

	assert!(!session.request_live_samples_for_cursor(monitor, cursor));
	assert_eq!(session.latest_live_cursor_sample_request_id, Some(1));
	assert_eq!(session.applied_live_cursor_sample_request_id, Some(1));
	assert_eq!(
		session.live_sample_stream.as_ref().and_then(MacLiveFrameStream::debug_last_request_kind),
		Some("prime_monitor_nonblocking")
	);
}

#[cfg(target_os = "macos")]
#[test]
fn prime_startup_live_stream_nonblocking_primes_stream_for_live_mode() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.live_sample_stream = Some(MacLiveFrameStream::new());

	session.prime_startup_live_stream_nonblocking(Some(monitor));

	assert_eq!(
		session.live_sample_stream.as_ref().and_then(MacLiveFrameStream::debug_last_request_kind),
		Some("prime_monitor_nonblocking")
	);
}

#[cfg(target_os = "macos")]
#[test]
fn begin_live_capture_press_prewarms_frozen_entry_stream_when_unprimed() {
	let monitor = tests::test_monitor();
	let cursor = GlobalPoint::new(120, 180);
	let mut session = OverlaySession::new();

	session.live_sample_stream = Some(MacLiveFrameStream::new());

	session.begin_live_capture_press(monitor, cursor);

	assert!(matches!(
		session.live_capture_interaction,
		LiveCaptureInteraction::PressPending { monitor: interaction_monitor, press_global, .. }
			if interaction_monitor == monitor && press_global == cursor
	));
	assert_eq!(
		session.live_sample_stream.as_ref().and_then(MacLiveFrameStream::debug_last_request_kind),
		Some("prime_monitor_nonblocking")
	);
}

#[cfg(target_os = "macos")]
#[test]
fn begin_live_capture_press_force_refreshes_existing_live_snapshot() {
	let monitor = tests::test_monitor();
	let cursor = GlobalPoint::new(120, 180);
	let stream = MacLiveFrameStream::new();
	let mut session = OverlaySession::new();

	stream.debug_store_test_snapshot(monitor, tests::fresh_live_stream_snapshot_captured_at());

	session.live_sample_stream = Some(stream);

	session.begin_live_capture_press(monitor, cursor);

	assert_eq!(
		session.live_sample_stream.as_ref().and_then(MacLiveFrameStream::debug_last_request_kind),
		Some("refresh_monitor_nonblocking_if_stale")
	);
}

#[test]
fn monitor_for_cursor_in_rects_finds_matching_monitor_without_windows() {
	let monitor_a = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let monitor_b = MonitorRect {
		id: 2,
		origin: GlobalPoint::new(1_000, 0),
		width: 1_200,
		height: 900,
		scale_factor_x1000: 2_000,
	};

	assert_eq!(
		OverlaySession::monitor_for_cursor_in_rects(
			&[monitor_a, monitor_b],
			GlobalPoint::new(42, 88)
		),
		Some(monitor_a)
	);
	assert_eq!(
		OverlaySession::monitor_for_cursor_in_rects(
			&[monitor_a, monitor_b],
			GlobalPoint::new(1_240, 120)
		),
		Some(monitor_b)
	);
	assert_eq!(
		OverlaySession::monitor_for_cursor_in_rects(
			&[monitor_a, monitor_b],
			GlobalPoint::new(2_400, 1_200)
		),
		None
	);
}

#[cfg(target_os = "macos")]
#[test]
fn resolve_device_cursor_point_prefers_recent_cursor_when_scaled_coordinates_are_ambiguous() {
	let monitor = tests::test_monitor_with_scale(1_000, 800, 2_000);
	let raw = GlobalPoint::new(190, 230);

	assert_eq!(
		OverlaySession::resolve_device_cursor_point_for_monitors(
			&[monitor],
			raw,
			Some(GlobalPoint::new(95, 115)),
		),
		Some((monitor, GlobalPoint::new(95, 115), DeviceCursorPointSource::DevicePixelsFallback))
	);
}

#[cfg(target_os = "macos")]
#[test]
fn resolve_device_cursor_point_keeps_direct_points_when_they_match_recent_cursor() {
	let monitor = tests::test_monitor_with_scale(1_000, 800, 2_000);
	let raw = GlobalPoint::new(190, 230);

	assert_eq!(
		OverlaySession::resolve_device_cursor_point_for_monitors(
			&[monitor],
			raw,
			Some(GlobalPoint::new(190, 230)),
		),
		Some((monitor, GlobalPoint::new(190, 230), DeviceCursorPointSource::DevicePoints))
	);
}

#[cfg(target_os = "macos")]
#[test]
fn overlay_window_event_global_position_rounds_fractional_scaled_positions() {
	let monitor = tests::test_monitor_with_scale(1_000, 800, 2_000);
	let window_size = PhysicalSize::new(2_000, 1_600);

	assert_eq!(
		OverlaySession::overlay_window_event_global_position(
			monitor,
			2.0,
			window_size,
			PhysicalPosition::new(301.9, 361.9),
		),
		GlobalPoint::new(151, 181)
	);
	assert_eq!(
		OverlaySession::overlay_window_event_global_position(
			monitor,
			2.0,
			window_size,
			PhysicalPosition::new(1_999.9, 1_599.9),
		),
		GlobalPoint::new(999, 799)
	);
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_selection_drag_uses_non_rounding_cursor_move_updates_without_shifting_cursor_context() {
	let monitor = tests::test_monitor_with_scale(1_000, 800, 2_000);
	let window_id = WindowId::from(1);
	let position = PhysicalPosition::new(601.9, 721.9);
	let window_size = PhysicalSize::new(2_000, 1_600);
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let event_global =
		OverlaySession::overlay_window_event_global_position(monitor, 2.0, window_size, position);
	let frozen_drag_global = OverlaySession::overlay_window_frozen_selection_drag_global_position(
		monitor,
		2.0,
		window_size,
		position,
	);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(150, 180)));
	assert_eq!(event_global, GlobalPoint::new(301, 361));
	assert_eq!(frozen_drag_global, GlobalPoint::new(300, 360));
	assert!(matches!(
		session.handle_cursor_moved_with_overlay_window(
			window_id,
			position,
			Some(monitor),
			monitor,
			2.0,
			window_size,
		),
		OverlayControl::Continue
	));
	assert_eq!(session.last_event_cursor, Some((monitor, event_global)));
	assert!(session.last_event_cursor_at.is_some());
	assert_eq!(session.cursor_monitor, Some(monitor));
	assert_eq!(session.state.cursor, Some(event_global));
	assert_eq!(session.state.frozen_capture_rect, Some(RectPoints::new(250, 300, 200, 240)));
	assert!(matches!(
		session.handle_frozen_left_mouse_input(monitor, ElementState::Released),
		OverlayControl::Continue
	));
	assert_eq!(session.last_event_cursor, Some((monitor, event_global)));
	assert_eq!(session.state.cursor, Some(event_global));
	assert_eq!(session.state.frozen_capture_rect, Some(RectPoints::new(250, 300, 200, 240)));
	assert_eq!(session.frozen_selection_drag, crate::overlay::FrozenSelectionDragState::default());
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_arrow_drag_updates_from_overlay_cursor_moved_events() {
	let monitor = tests::test_monitor_with_scale(1_000, 800, 2_000);
	let window_id = WindowId::from(1);
	let position = PhysicalPosition::new(801.9, 321.9);
	let window_size = PhysicalSize::new(2_000, 1_600);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 600, 400));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Arrow;

	assert!(session.begin_frozen_arrow_drag(GlobalPoint::new(200, 160)));
	assert!(matches!(
		session.handle_cursor_moved_with_overlay_window(
			window_id,
			position,
			Some(monitor),
			monitor,
			2.0,
			window_size,
		),
		OverlayControl::Continue
	));
	assert_eq!(
		session.active_frozen_arrow_preview().map(|annotation| (annotation.start, annotation.end)),
		Some((Pos2::new(200.0, 160.0), Pos2::new(401.0, 161.0)))
	);
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_spotlight_drag_updates_from_overlay_cursor_moved_events() {
	let monitor = tests::test_monitor_with_scale(1_000, 800, 2_000);
	let window_id = WindowId::from(1);
	let position = PhysicalPosition::new(1_601.9, 1_121.9);
	let window_size = PhysicalSize::new(2_000, 1_600);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 600, 400));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Spotlight;

	assert!(session.begin_frozen_spotlight_drag(GlobalPoint::new(200, 160)));
	assert!(matches!(
		session.handle_cursor_moved_with_overlay_window(
			window_id,
			position,
			Some(monitor),
			monitor,
			2.0,
			window_size,
		),
		OverlayControl::Continue
	));
	assert_eq!(session.frozen_spotlight_preview_rect, Some(RectPoints::new(200, 160, 500, 360)));
}

#[cfg(target_os = "macos")]
#[test]
fn startup_live_rgb_plan_keeps_focus_independent_from_seed_monitor() {
	let monitor = MonitorRect {
		id: 2,
		origin: GlobalPoint::new(1_000, 0),
		width: 1_200,
		height: 900,
		scale_factor_x1000: 2_000,
	};

	assert_eq!(
		OverlaySession::startup_live_rgb_plan(None),
		StartupLiveRgbPlan { focus_window: false, seed_monitor: None }
	);
	assert_eq!(
		OverlaySession::startup_live_rgb_plan(Some(monitor)),
		StartupLiveRgbPlan { focus_window: false, seed_monitor: Some(monitor) }
	);
}

#[cfg(target_os = "macos")]
#[test]
fn wants_global_cancel_hotkey_for_any_active_session_mode() {
	let mut session = OverlaySession::new();

	assert!(!session.wants_global_cancel_hotkey());

	session.session_active = true;
	session.state.mode = OverlayMode::Live;

	assert!(session.wants_global_cancel_hotkey());

	session.state.mode = OverlayMode::Frozen;

	assert!(session.wants_global_cancel_hotkey());
}

#[cfg(target_os = "macos")]
#[test]
fn wants_global_loupe_hotkey_only_in_live_mode() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;

	assert!(session.wants_global_loupe_hotkey());

	session.state.mode = OverlayMode::Frozen;

	assert!(!session.wants_global_loupe_hotkey());
}

#[cfg(target_os = "macos")]
#[test]
fn wants_global_frozen_hotkeys_only_in_plain_frozen_mode() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	assert!(!session.wants_global_frozen_hotkeys());

	session.session_active = true;
	session.state.mode = OverlayMode::Live;

	assert!(!session.wants_global_frozen_hotkeys());

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	assert!(session.wants_global_frozen_hotkeys());

	session.scroll_capture.active = true;

	assert!(!session.wants_global_frozen_hotkeys());

	session.scroll_capture.active = false;
	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(!session.wants_global_frozen_hotkeys());
}

#[cfg(target_os = "macos")]
#[test]
fn global_escape_hotkey_cancels_active_capture_modes() {
	for mode in [OverlayMode::Live, OverlayMode::Frozen] {
		let mut session = OverlaySession::new();

		session.state.mode = mode;

		assert!(matches!(
			session.handle_global_escape_hotkey(),
			OverlayControl::Exit(OverlayExit::Cancelled)
		));
	}
}

#[cfg(target_os = "macos")]
#[test]
fn global_loupe_hotkey_toggles_live_loupe_on_press() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;

	assert!(matches!(session.handle_global_loupe_hotkey(true), OverlayControl::Continue));
	assert!(session.state.alt_held);
	assert!(session.loupe_activation_key_down);
	assert!(matches!(session.handle_global_loupe_hotkey(false), OverlayControl::Continue));
	assert!(session.state.alt_held);
	assert!(!session.loupe_activation_key_down);
	assert!(matches!(session.handle_global_loupe_hotkey(true), OverlayControl::Continue));
	assert!(!session.state.alt_held);
	assert!(session.loupe_activation_key_down);
	assert!(matches!(session.handle_global_loupe_hotkey(false), OverlayControl::Continue));
	assert!(!session.state.alt_held);
	assert!(!session.loupe_activation_key_down);
}

#[cfg(target_os = "macos")]
#[test]
fn global_frozen_copy_hotkey_queues_copy_unless_text_edit_is_active() {
	let monitor = tests::test_monitor();

	for (label, active_text_edit, expected_action) in
		[("no active text edit", false, Some(PngAction::Copy)), ("active text edit", true, None)]
	{
		let mut session = OverlaySession::new();

		session.session_active = true;

		session.state.begin_freeze(monitor);

		tests::finish_frozen_ready_state(&mut session, monitor, tests::test_frozen_image());

		if active_text_edit {
			session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
			session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

			assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
		}

		assert!(matches!(
			session.handle_global_frozen_hotkey(FrozenGlobalHotkey::Copy),
			OverlayControl::Continue
		));
		assert_eq!(session.pending_png_action, expected_action, "{label}");
	}
}

#[test]
fn initialize_cursor_state_for_cursor_preserves_preseeded_live_rgb() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let cursor = GlobalPoint::new(120, 180);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.state.rgb = Some(Rgb::new(10, 20, 30));

	session.initialize_cursor_state_for_cursor(cursor, Some(monitor));

	assert_eq!(session.state.cursor, Some(cursor));
	assert_eq!(session.cursor_monitor, Some(monitor));
	assert_eq!(session.state.rgb, Some(Rgb::new(10, 20, 30)));
}

#[test]
fn initialize_cursor_state_for_cursor_clears_rgb_when_no_monitor_matches() {
	let cursor = GlobalPoint::new(2_400, 1_200);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.state.rgb = Some(Rgb::new(10, 20, 30));

	session.initialize_cursor_state_for_cursor(cursor, None);

	assert_eq!(session.state.cursor, Some(cursor));
	assert_eq!(session.cursor_monitor, None);
	assert_eq!(session.state.rgb, None);
}

#[test]
fn live_overlay_redraw_needed_for_cursor_update_only_for_monitor_or_drag_changes() {
	let monitor_a = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let monitor_b = MonitorRect {
		id: 2,
		origin: GlobalPoint::new(1_000, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let drag = Some(MonitorRectPoints {
		monitor_id: monitor_a.id,
		rect: RectPoints::new(100, 120, 240, 320),
	});

	assert!(!OverlaySession::live_overlay_redraw_needed_for_cursor_update(
		Some(monitor_a),
		monitor_a,
		None,
		None,
	));
	assert!(OverlaySession::live_overlay_redraw_needed_for_cursor_update(
		Some(monitor_a),
		monitor_a,
		None,
		drag,
	));
	assert!(OverlaySession::live_overlay_redraw_needed_for_cursor_update(
		Some(monitor_a),
		monitor_b,
		None,
		None,
	));
}

#[test]
fn live_hud_redraw_needed_for_cursor_update_tracks_cursor_or_monitor_changes() {
	let monitor_a = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let monitor_b = MonitorRect {
		id: 2,
		origin: GlobalPoint::new(1_000, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let cursor_a = GlobalPoint::new(120, 180);
	let cursor_b = GlobalPoint::new(140, 200);

	assert!(!OverlaySession::live_hud_redraw_needed_for_cursor_update(
		Some(cursor_a),
		cursor_a,
		Some(monitor_a),
		monitor_a,
	));
	assert!(OverlaySession::live_hud_redraw_needed_for_cursor_update(
		Some(cursor_a),
		cursor_b,
		Some(monitor_a),
		monitor_a,
	));
	assert!(OverlaySession::live_hud_redraw_needed_for_cursor_update(
		Some(cursor_a),
		cursor_a,
		Some(monitor_a),
		monitor_b,
	));
	assert!(OverlaySession::live_hud_redraw_needed_for_cursor_update(
		None, cursor_a, None, monitor_a,
	));
}

#[test]
fn live_hud_redraw_consumes_pending_move_without_size_change() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.pending_hud_outer_pos = Some(GlobalPoint::new(120, 180));

	assert!(session.should_try_pending_hud_window_move_on_redraw(&HudRedrawSummary::default()));
}

#[test]
fn frozen_hud_redraw_does_not_consume_pending_move_without_size_change() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.pending_hud_outer_pos = Some(GlobalPoint::new(120, 180));

	assert!(!session.should_try_pending_hud_window_move_on_redraw(&HudRedrawSummary::default()));
	assert!(session.should_try_pending_hud_window_move_on_redraw(&HudRedrawSummary {
		position_update_elapsed: Some(Duration::from_micros(1)),
		..HudRedrawSummary::default()
	}));
}

#[test]
fn hidden_hud_redraw_forces_pending_move_before_show() {
	let mut session = OverlaySession::new();

	session.hud_window_visible = false;
	session.pending_hud_outer_pos = Some(GlobalPoint::new(120, 180));

	assert!(session.should_force_pending_hud_window_move_before_redraw());

	session.hud_window_visible = true;

	assert!(!session.should_force_pending_hud_window_move_before_redraw());

	session.hud_window_visible = false;
	session.pending_hud_outer_pos = None;

	assert!(!session.should_force_pending_hud_window_move_before_redraw());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn capture_windows_hidden_skips_hud_redraw() {
	let mut session = OverlaySession::new();

	session.hud_window_visible = true;
	session.capture_windows_hidden = true;

	assert!(matches!(session.maybe_skip_hud_redraw(), Some(OverlayControl::Continue)));
	assert!(!session.hud_window_visible);
}

#[cfg(target_os = "macos")]
#[test]
fn capture_windows_hidden_allows_hud_redraw_for_error_message() {
	let mut session = OverlaySession::new();

	session.capture_windows_hidden = true;

	session.state.set_error("Preparing capture...");

	assert!(session.maybe_skip_hud_redraw().is_none());
}

#[test]
fn live_drag_skips_hud_redraw() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;

	session.set_live_capture_interaction(LiveCaptureInteraction::DraggingSelection {
		monitor,
		press_global: GlobalPoint::new(100, 120),
		current_global: GlobalPoint::new(340, 440),
	});

	session.hud_window_visible = true;

	assert!(matches!(session.maybe_skip_hud_redraw(), Some(OverlayControl::Continue)));
	assert!(!session.hud_window_visible);
}

#[test]
fn frozen_hud_redraw_skips_without_error_message() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.hud_window_visible = true;

	assert!(matches!(session.maybe_skip_hud_redraw(), Some(OverlayControl::Continue)));
	assert!(!session.hud_window_visible);
}

#[test]
fn live_cursor_update_tries_pending_follow_window_moves() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;

	assert!(!session.should_try_pending_follow_window_move_on_live_cursor_update());

	session.pending_hud_outer_pos = Some(GlobalPoint::new(120, 180));

	assert!(session.should_try_pending_follow_window_move_on_live_cursor_update());

	session.pending_hud_outer_pos = None;
	session.pending_loupe_outer_pos = Some(GlobalPoint::new(140, 220));

	assert!(session.should_try_pending_follow_window_move_on_live_cursor_update());
}

#[test]
fn frozen_cursor_update_does_not_try_pending_follow_window_moves() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.pending_hud_outer_pos = Some(GlobalPoint::new(120, 180));
	session.pending_loupe_outer_pos = Some(GlobalPoint::new(140, 220));

	assert!(!session.should_try_pending_follow_window_move_on_live_cursor_update());
}

#[cfg(target_os = "macos")]
#[test]
fn apply_live_cursor_sample_clears_existing_loupe_when_alt_is_released() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let cursor = GlobalPoint::new(120, 180);
	let mut session = OverlaySession::new();

	session.cursor_monitor = Some(monitor);
	session.state.cursor = Some(cursor);
	session.state.alt_held = true;

	let _ = session.apply_live_cursor_sample_detail(
		monitor,
		cursor,
		LiveCursorSample {
			rgb: Some(Rgb::new(10, 20, 30)),
			patch: Some(RgbaImage::from_pixel(
				3,
				3,
				crate::overlay::tests::Rgba([10, 20, 30, 255]),
			)),
		},
	);

	session.state.alt_held = false;

	assert!(
		session
			.apply_live_cursor_sample_detail(
				monitor,
				cursor,
				LiveCursorSample { rgb: None, patch: None },
			)
			.any_changed()
	);
	assert!(session.state.loupe.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn stabilized_live_hud_inner_size_keeps_live_width_from_shrinking() {
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.hud_inner_size_points = Some((826, 44));

	assert_eq!(
		OverlaySession::stabilized_live_hud_inner_size(
			OverlayMode::Live,
			session.hud_inner_size_points,
			(810, 44),
		),
		(826, 44)
	);
	assert_eq!(
		OverlaySession::stabilized_live_hud_inner_size(
			OverlayMode::Live,
			session.hud_inner_size_points,
			(780, 44),
		),
		(826, 44)
	);

	session.state.mode = OverlayMode::Frozen;

	assert_eq!(
		OverlaySession::stabilized_live_hud_inner_size(
			OverlayMode::Frozen,
			session.hud_inner_size_points,
			(810, 44),
		),
		(810, 44)
	);
}

#[test]
fn live_hud_position_text_uses_stable_monitor_width() {
	let monitor = MonitorRect {
		id: 5,
		origin: GlobalPoint::new(0, 0),
		width: 3_008,
		height: 1_692,
		scale_factor_x1000: 2_000,
	};
	let short = hud_helpers::format_live_hud_position_text(monitor, GlobalPoint::new(842, 846));
	let long = hud_helpers::format_live_hud_position_text(monitor, GlobalPoint::new(1_504, 1_320));

	assert_eq!(short.len(), long.len());
	assert_eq!(short, "x= 842, y= 846");
	assert_eq!(long, "x=1504, y=1320");
}

#[test]
fn live_hud_rgb_text_uses_fixed_width_placeholders() {
	let (missing_hex, missing_rgb) = hud_helpers::format_live_hud_rgb_text(None);
	let (hex, rgb) = hud_helpers::format_live_hud_rgb_text(Some(Rgb::new(7, 128, 255)));

	assert_eq!(missing_hex.len(), hex.len());
	assert_eq!(missing_rgb.len(), rgb.len());
	assert_eq!(missing_hex, "#??????");
	assert_eq!(missing_rgb, "RGB(???, ???, ???)");
	assert_eq!(rgb, "RGB(  7, 128, 255)");
}

#[test]
fn stable_live_loupe_side_prefers_configured_patch_side_over_runtime_patch() {
	for (patch_width, patch_height) in [(17, 19), (25, 25)] {
		let mut state = OverlayState::new();

		state.loupe_patch_side_px = 21;
		state.loupe = Some(LoupeSample {
			center: GlobalPoint::new(100, 120),
			patch: RgbaImage::from_pixel(patch_width, patch_height, image::Rgba([0, 0, 0, 255])),
		});

		assert_eq!(hud_helpers::stable_live_loupe_side_px(&state), 21);
	}
}

#[test]
fn stable_live_loupe_window_inner_size_matches_runtime_target() {
	assert_eq!(hud_helpers::stable_live_loupe_window_inner_size_points(21), (232, 232));
	assert_eq!(hud_helpers::stable_live_loupe_window_inner_size_points(1), (32, 32));
}
