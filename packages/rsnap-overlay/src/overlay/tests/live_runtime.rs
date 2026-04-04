#![allow(clippy::wildcard_imports)]

use super::*;

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
	let patch = image::RgbaImage::from_pixel(3, 3, Rgba([10, 20, 30, 255]));
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
	let patch = image::RgbaImage::from_pixel(3, 3, Rgba([10, 20, 30, 255]));
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

	session.config.alt_activation = AltActivationMode::Toggle;

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
fn apply_loupe_activation_input_hold_tracks_pressed_state() {
	let mut session = OverlaySession::new();

	session.config.alt_activation = AltActivationMode::Hold;

	assert!(session.apply_loupe_activation_input(true, false));
	assert!(session.state.alt_held);
	assert!(!session.apply_loupe_activation_input(true, false));
	assert!(session.state.alt_held);
	assert!(session.apply_loupe_activation_input(false, false));
	assert!(!session.state.alt_held);
}

#[cfg(target_os = "macos")]
#[test]
fn plain_character_shortcut_available_blocks_loupe_activation_key_while_pressed() {
	let mut session = OverlaySession::new();

	session.config.alt_activation = AltActivationMode::Toggle;

	assert!(session.plain_character_shortcut_available());
	assert!(session.apply_loupe_activation_key_event(true, false));
	assert!(!session.plain_character_shortcut_available());
	assert!(!session.apply_loupe_activation_key_event(false, false));
	assert!(session.state.alt_held);
	assert!(session.plain_character_shortcut_available());
}

#[cfg(target_os = "macos")]
#[test]
fn clear_loupe_activation_on_focus_loss_releases_hold_mode_state() {
	let mut session = OverlaySession::new();

	session.config.alt_activation = AltActivationMode::Hold;

	assert!(session.apply_loupe_activation_key_event(true, false));
	assert!(session.state.alt_held);
	assert!(session.loupe_activation_key_down);

	session.clear_loupe_activation_on_focus_loss();

	assert!(!session.state.alt_held);
	assert!(!session.loupe_activation_key_down);
	assert!(session.plain_character_shortcut_available());
}

#[cfg(target_os = "macos")]
#[test]
fn clear_loupe_activation_on_focus_loss_releases_toggle_press_without_toggling_off() {
	let mut session = OverlaySession::new();

	session.config.alt_activation = AltActivationMode::Toggle;

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

	session.config.alt_activation = AltActivationMode::Hold;

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
fn pending_focus_loss_cleanup_clears_loupe_after_all_overlay_windows_blur() {
	let overlay_window_id = WindowId::from(1);
	let mut session = OverlaySession::new();

	session.config.alt_activation = AltActivationMode::Hold;

	session.note_window_focus_change(overlay_window_id, true);

	assert!(session.apply_loupe_activation_key_event(true, false));
	assert!(session.state.alt_held);

	session.note_window_focus_change(overlay_window_id, false);
	session.maybe_clear_loupe_activation_after_focus_loss();

	assert!(!session.state.alt_held);
	assert!(!session.loupe_activation_key_down);
	assert!(session.plain_character_shortcut_available());
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
	let mut state = crate::state::OverlayState::new();

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
		StartupLiveRgbPlan { focus_window: true, seed_monitor: None }
	);
	assert_eq!(
		OverlaySession::startup_live_rgb_plan(Some(monitor)),
		StartupLiveRgbPlan { focus_window: true, seed_monitor: Some(monitor) }
	);
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
			patch: Some(image::RgbaImage::from_pixel(3, 3, Rgba([10, 20, 30, 255]))),
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
fn stable_live_loupe_side_prefers_configured_patch_side() {
	let mut state = crate::state::OverlayState::new();

	state.loupe_patch_side_px = 21;
	state.loupe = Some(LoupeSample {
		center: GlobalPoint::new(100, 120),
		patch: RgbaImage::from_pixel(17, 19, image::Rgba([0, 0, 0, 255])),
	});

	assert_eq!(hud_helpers::stable_live_loupe_side_px(&state), 21);
}

#[test]
fn stable_live_loupe_side_ignores_larger_runtime_patch() {
	let mut state = crate::state::OverlayState::new();

	state.loupe_patch_side_px = 21;
	state.loupe = Some(LoupeSample {
		center: GlobalPoint::new(100, 120),
		patch: RgbaImage::from_pixel(25, 25, image::Rgba([0, 0, 0, 255])),
	});

	assert_eq!(hud_helpers::stable_live_loupe_side_px(&state), 21);
}

#[test]
fn stable_live_loupe_window_inner_size_matches_runtime_target() {
	assert_eq!(hud_helpers::stable_live_loupe_window_inner_size_points(21), (232, 232));
	assert_eq!(hud_helpers::stable_live_loupe_window_inner_size_points(1), (32, 32));
}
