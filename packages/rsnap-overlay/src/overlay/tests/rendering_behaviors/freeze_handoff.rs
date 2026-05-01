#[cfg(target_os = "macos")]
use crate::overlay::tests::rendering_behaviors::{
	Arc, MonitorImageSnapshot, WindowCaptureAlphaMode,
};
use crate::overlay::tests::rendering_behaviors::{
	FrozenAnnotationStyleCapsulePlacement, FrozenCaptureSource, FrozenToolbarState,
	FrozenToolbarTool, GlobalPoint, HudTheme, Id, LayerId, MonitorRect, Order, OverlayControl,
	OverlayMode, OverlaySession, OverlayState, PngAction, Pos2, RawInput, Rect, RectPoints,
	SelectionDashedBorderCache, SelectionFlowGeometryCache, TOOLBAR_CAPTURE_GAP_PX,
	TOOLBAR_SCREEN_MARGIN_PX, ToolbarPlacement, Ui, Vec2, WindowFreezeCaptureTarget,
	WindowRenderer, WorkerErrorSource, WorkerResponse, overlay, tests,
};

#[cfg(target_os = "macos")]
#[test]
fn pending_freeze_capture_dispatches_even_with_seeded_preview() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());
	tests::set_session_pending_freeze_capture(&mut session, Some(monitor));

	assert!(session.should_dispatch_pending_freeze_capture(monitor));
}

#[cfg(target_os = "macos")]
#[test]
fn pending_freeze_capture_dispatches_when_previous_frozen_monitor_differs() {
	let previous_monitor = tests::test_monitor();
	let next_monitor = MonitorRect {
		id: previous_monitor.id + 1,
		origin: GlobalPoint::new(previous_monitor.width as i32, 0),
		..previous_monitor
	};
	let mut session = OverlaySession::new();

	session.state.begin_freeze(previous_monitor);

	tests::finish_frozen_display_state(&mut session, previous_monitor, tests::test_frozen_image());
	tests::set_session_pending_freeze_capture(&mut session, Some(next_monitor));

	assert!(session.should_dispatch_pending_freeze_capture(next_monitor));
}

#[cfg(target_os = "macos")]
#[test]
fn snapshot_background_capture_finishes_frozen_transition_immediately() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let frozen_image = tests::test_frozen_image();
	let snapshot = Arc::new(MonitorImageSnapshot {
		captured_at: tests::fresh_live_stream_snapshot_captured_at(),
		stream_generation: 1,
		monitor,
		image: Arc::new(frozen_image.clone()),
	});
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);

	tests::set_session_pending_freeze_capture(&mut session, Some(monitor));
	tests::set_session_pending_window_freeze_capture(
		&mut session,
		Some(WindowFreezeCaptureTarget { monitor, window_id: 11, rect: capture_rect }),
	);

	assert!(session.maybe_finish_frozen_capture_from_snapshot(
		monitor,
		tests::session_pending_window_freeze_capture(&session),
		None,
		Some(snapshot),
		"live_stream_snapshot",
	));
	assert!(tests::session_export_authority_ready(&session));
	assert!(tests::session_pending_freeze_capture(&session).is_none());
	assert!(tests::session_pending_window_freeze_capture(&session).is_none());
	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&frozen_image));
	assert!(session.toolbar_state.final_capture_ready);
}

#[cfg(target_os = "macos")]
#[test]
fn snapshot_matte_window_capture_keeps_authoritative_handoff_pending() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let snapshot = Arc::new(MonitorImageSnapshot {
		captured_at: tests::fresh_live_stream_snapshot_captured_at(),
		stream_generation: 1,
		monitor,
		image: Arc::new(tests::test_frozen_image()),
	});
	let window_target = WindowFreezeCaptureTarget { monitor, window_id: 11, rect: capture_rect };
	let mut session = OverlaySession::new();

	session.config.window_capture_alpha_mode = WindowCaptureAlphaMode::MatteDark;

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);

	tests::set_session_pending_freeze_capture(&mut session, Some(monitor));
	tests::set_session_pending_window_freeze_capture(&mut session, Some(window_target));

	assert!(!session.maybe_finish_frozen_capture_from_snapshot(
		monitor,
		Some(window_target),
		None,
		Some(snapshot),
		"live_stream_snapshot",
	));
	assert!(!tests::session_export_authority_ready(&session));
	assert_eq!(tests::session_pending_freeze_capture(&session), Some(monitor));
	assert_eq!(tests::session_pending_window_freeze_capture(&session), Some(window_target));
	assert!(session.state.frozen_display_image.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn stale_snapshot_does_not_finish_frozen_transition_immediately() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let snapshot = Arc::new(MonitorImageSnapshot {
		captured_at: tests::stale_live_stream_snapshot_captured_at(),
		stream_generation: 1,
		monitor,
		image: Arc::new(tests::test_frozen_image()),
	});
	let window_target = WindowFreezeCaptureTarget { monitor, window_id: 11, rect: capture_rect };
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);

	tests::set_session_pending_freeze_capture(&mut session, Some(monitor));
	tests::set_session_pending_window_freeze_capture(&mut session, Some(window_target));

	assert!(!session.maybe_finish_frozen_capture_from_snapshot(
		monitor,
		Some(window_target),
		None,
		Some(snapshot),
		"live_stream_snapshot",
	));
	assert!(!tests::session_export_authority_ready(&session));
	assert_eq!(tests::session_pending_freeze_capture(&session), Some(monitor));
	assert_eq!(tests::session_pending_window_freeze_capture(&session), Some(window_target));
	assert!(session.state.frozen_display_image.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn snapshot_seeded_preview_keeps_authoritative_handoff_pending() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let frozen_image = tests::test_frozen_image();
	let snapshot = Arc::new(MonitorImageSnapshot {
		captured_at: tests::fresh_live_stream_snapshot_captured_at(),
		stream_generation: 1,
		monitor,
		image: Arc::new(frozen_image.clone()),
	});
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);

	tests::set_session_pending_freeze_capture(&mut session, Some(monitor));

	assert!(session.maybe_seed_frozen_capture_preview_from_snapshot(
		monitor,
		None,
		Some(snapshot),
		"live_stream_snapshot_seeded_unverified",
	));
	assert!(!tests::session_export_authority_ready(&session));
	assert_eq!(tests::session_pending_freeze_capture(&session), Some(monitor));
	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&frozen_image));
	assert!(!session.toolbar_state.final_capture_ready);
}

#[cfg(target_os = "macos")]
#[test]
fn snapshot_seeded_preview_makes_toolbar_eligible_before_final_capture_ready() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let snapshot = Arc::new(MonitorImageSnapshot {
		captured_at: tests::fresh_live_stream_snapshot_captured_at(),
		stream_generation: 1,
		monitor,
		image: Arc::new(tests::test_frozen_image()),
	});
	let mut session = OverlaySession::new();

	session.toolbar_state.visible = true;

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);

	tests::set_session_pending_freeze_capture(&mut session, Some(monitor));

	assert!(session.maybe_seed_frozen_capture_preview_from_snapshot(
		monitor,
		None,
		Some(snapshot),
		"live_stream_snapshot_seeded_unverified",
	));
	assert!(session.frozen_preview_visible());
	assert!(!tests::session_export_authority_ready(&session));
	assert!(session.startup_aux_window_creation_pending);
}

#[cfg(target_os = "macos")]
#[test]
fn stale_snapshot_does_not_seed_frozen_preview() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let snapshot = Arc::new(MonitorImageSnapshot {
		captured_at: tests::stale_live_stream_snapshot_captured_at(),
		stream_generation: 1,
		monitor,
		image: Arc::new(tests::test_frozen_image()),
	});
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);

	tests::set_session_pending_freeze_capture(&mut session, Some(monitor));

	assert!(!session.maybe_seed_frozen_capture_preview_from_snapshot(
		monitor,
		None,
		Some(snapshot),
		"live_stream_snapshot_seeded_unverified",
	));
	assert_eq!(tests::session_pending_freeze_capture(&session), Some(monitor));
	assert!(!tests::session_export_authority_ready(&session));
	assert!(session.state.frozen_display_image.is_none());
	assert!(!session.toolbar_state.final_capture_ready);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn pending_freeze_capture_waits_for_empty_frozen_image_off_macos() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());
	tests::set_session_pending_freeze_capture(&mut session, Some(monitor));

	assert!(!session.should_dispatch_pending_freeze_capture(monitor));
}

#[test]
fn frozen_final_capture_ready_requires_no_pending_or_inflight_capture() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	assert!(!session.frozen_final_capture_ready());

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());
	tests::promote_session_export_authority_ready(&mut session);

	assert!(session.frozen_final_capture_ready());

	tests::set_session_pending_freeze_capture(&mut session, Some(monitor));

	assert!(!session.frozen_final_capture_ready());

	tests::set_session_pending_freeze_capture(&mut session, None);
	tests::set_session_inflight_freeze_capture(&mut session, Some(monitor));

	assert!(!session.frozen_final_capture_ready());
}

#[test]
fn frozen_preview_does_not_become_final_ready_when_capture_tracking_clears_without_success() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 220, 180);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	tests::set_session_inflight_freeze_capture(&mut session, Some(monitor));

	assert!(!session.frozen_final_capture_ready());
	assert!(!session.scroll_capture_selection_is_ready());

	// Emulate a preview-first failure where the authoritative capture tracking clears.
	tests::set_session_inflight_freeze_capture(&mut session, None);

	assert!(!session.frozen_final_capture_ready());
	assert!(!session.scroll_capture_selection_is_ready());

	session.begin_png_action(PngAction::Copy);

	assert_eq!(session.pending_png_action, None);
	assert!(session.pending_encode_png.is_none());
	assert_eq!(session.state.error_message.as_deref(), Some("Preparing capture..."));
}

#[test]
fn unrelated_worker_errors_do_not_clear_pending_freeze_capture_state() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::set_session_pending_freeze_capture(&mut session, Some(monitor));
	tests::set_session_pending_freeze_capture_armed(&mut session, true);
	tests::set_session_pending_window_freeze_capture(
		&mut session,
		Some(WindowFreezeCaptureTarget {
			monitor,
			window_id: 42,
			rect: RectPoints::new(10, 20, 30, 40),
		}),
	);

	let control = session.maybe_tick_worker_response_limiter(WorkerResponse::Error {
		source: WorkerErrorSource::RefreshWindowList,
		message: String::from("window refresh failed"),
	});

	assert!(matches!(control, OverlayControl::Continue));
	assert_eq!(tests::session_pending_freeze_capture(&session), Some(monitor));
	assert!(tests::session_frozen_capture_armed(&session));
	assert!(tests::session_inflight_freeze_capture(&session).is_none());
	assert!(tests::session_pending_window_freeze_capture(&session).is_some());
	assert_eq!(session.state.error_message.as_deref(), Some("window refresh failed"));
}

#[test]
fn frozen_base_toolbar_hud_pill_uses_half_height_corner_radius() {
	let ctx = tests::test_egui_context();
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));
	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));

	let toolbar_placement = session.config.toolbar_placement;
	let state = &session.state;
	let toolbar_state = &mut session.toolbar_state;
	let mut hud_pill = None;
	let _ = ctx.run_ui(
		RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|ui: &mut Ui| {
			WindowRenderer::render_frozen_toolbar_ui(
				ui.ctx(),
				state,
				monitor,
				HudTheme::Dark,
				toolbar_placement,
				false,
				false,
				1.0,
				0.0,
				0.0,
				Some(toolbar_state),
				None,
				&mut hud_pill,
			);
		},
	);
	let hud_pill = hud_pill.expect("base toolbar should render after readiness stabilizes");

	assert_eq!(hud_pill.radius_points, (hud_pill.rect.height() * 0.5).round());
}

#[test]
fn frozen_annotation_toolbar_hud_pill_keeps_standard_corner_radius() {
	let ctx = tests::test_egui_context();
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));
	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));

	let toolbar_placement = session.config.toolbar_placement;
	let state = &session.state;
	let toolbar_state = &mut session.toolbar_state;
	let mut hud_pill = None;
	let _ = ctx.run_ui(
		RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|ui: &mut Ui| {
			WindowRenderer::render_frozen_toolbar_ui(
				ui.ctx(),
				state,
				monitor,
				HudTheme::Dark,
				toolbar_placement,
				false,
				false,
				1.0,
				0.0,
				0.0,
				Some(toolbar_state),
				None,
				&mut hud_pill,
			);
		},
	);
	let hud_pill = hud_pill.expect("annotation toolbar should render after readiness stabilizes");

	assert_eq!(
		hud_pill.radius_points,
		f32::from(overlay::frozen_toolbar_corner_radius_u8(hud_pill.rect.height())),
	);
}

#[test]
fn frozen_annotation_toolbar_hud_pill_covers_full_toolbar_bounds() {
	let ctx = tests::test_egui_context();
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));
	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));

	let toolbar_placement = session.config.toolbar_placement;
	let state = &session.state;
	let toolbar_state = &mut session.toolbar_state;
	let mut hud_pill = None;
	let _ = ctx.run_ui(
		RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|ui: &mut Ui| {
			WindowRenderer::render_frozen_toolbar_ui(
				ui.ctx(),
				state,
				monitor,
				HudTheme::Dark,
				toolbar_placement,
				false,
				false,
				1.0,
				0.0,
				0.0,
				Some(toolbar_state),
				None,
				&mut hud_pill,
			);
		},
	);
	let hud_pill = hud_pill.expect("annotation toolbar should render after readiness stabilizes");

	assert_eq!(hud_pill.rect.size(), WindowRenderer::frozen_toolbar_size(&session.toolbar_state));
}

#[test]
fn scroll_capture_and_export_wait_for_authoritative_frozen_capture() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 220, 180);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());
	tests::promote_session_export_authority_ready(&mut session);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(session.scroll_capture_selection_is_ready());

	tests::set_session_inflight_freeze_capture(&mut session, Some(monitor));

	assert!(!session.scroll_capture_selection_is_ready());

	session.begin_png_action(PngAction::Copy);

	assert_eq!(session.pending_png_action, None);
	assert!(session.pending_encode_png.is_none());
	assert_eq!(session.state.error_message.as_deref(), Some("Preparing capture..."));
}

#[test]
fn frozen_top_toolbar_reserved_rect_uses_inside_fallback_slot() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 400,
		height: 160,
		scale_factor_x1000: 1_000,
	};
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let capture_rect_points = RectPoints::new(40, 20, 240, 110);
	let capture_rect = Rect::from_min_size(
		Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
		Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
	);
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(capture_rect_points);

	let toolbar_state = FrozenToolbarState::default();
	let reserved_rect = WindowRenderer::frozen_toolbar_reserved_rect(
		&state,
		monitor,
		screen_rect,
		ToolbarPlacement::Top,
		&toolbar_state,
	)
	.expect("top fallback slot should still be reserved");

	assert_eq!(reserved_rect.min.y, capture_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX);
	assert_eq!(reserved_rect.height(), WindowRenderer::frozen_toolbar_size(&toolbar_state).y);
}

#[test]
fn frozen_annotation_capsule_keeps_top_toolbar_default_slot_stable() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(160, 220, 260, 240);
	let mut session = OverlaySession::new();

	session.config.toolbar_placement = ToolbarPlacement::Top;

	let base_pos = session.frozen_toolbar_default_position_for_capture_rect(monitor, capture_rect);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	let styled_pos =
		session.frozen_toolbar_default_position_for_capture_rect(monitor, capture_rect);

	assert_eq!(styled_pos, base_pos);
}

#[test]
fn frozen_annotation_capsule_flips_above_without_moving_bottom_toolbar_anchor() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut toolbar_state = FrozenToolbarState {
		selected_tool: FrozenToolbarTool::Text,
		..FrozenToolbarState::default()
	};
	let primary_size = WindowRenderer::frozen_toolbar_primary_size(&toolbar_state);
	let anchor_y = screen_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - primary_size.y;
	let capture_rect = Rect::from_min_size(
		Pos2::new(160.0, anchor_y - TOOLBAR_CAPTURE_GAP_PX - 200.0),
		Vec2::new(260.0, 200.0),
	);
	let anchor = WindowRenderer::frozen_toolbar_default_window_pos(
		screen_rect,
		capture_rect,
		primary_size,
		WindowRenderer::frozen_toolbar_positioning_size(&toolbar_state),
		ToolbarPlacement::Bottom,
	);

	assert_eq!(anchor.y, anchor_y);
	assert!(anchor.y + primary_size.y + TOOLBAR_SCREEN_MARGIN_PX <= screen_rect.max.y);

	let mut below_toolbar_state = toolbar_state.clone();

	below_toolbar_state.annotation_style_capsule_placement =
		FrozenAnnotationStyleCapsulePlacement::Below;

	assert!(
		WindowRenderer::frozen_toolbar_window_rect(&below_toolbar_state, anchor).max.y
			> screen_rect.max.y
	);

	WindowRenderer::sync_frozen_annotation_style_capsule_placement(
		&mut toolbar_state,
		screen_rect,
		anchor,
	);

	assert_eq!(
		toolbar_state.annotation_style_capsule_placement,
		FrozenAnnotationStyleCapsulePlacement::Above
	);
	assert_eq!(WindowRenderer::frozen_toolbar_primary_rect(&toolbar_state, anchor).min, anchor);
	assert!(
		WindowRenderer::frozen_toolbar_window_rect(&toolbar_state, anchor).min.y < anchor.y,
		"style capsule should render above the stable primary anchor when bottom space is tight",
	);
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_annotation_capsule_flip_keeps_native_toolbar_outer_position_stable() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let startup_size = overlay::frozen_toolbar_window_startup_size_points();
	let mut session = OverlaySession::new();

	session.toolbar_inner_size_points =
		Some((startup_size.x.ceil().max(1.0) as u32, startup_size.y.ceil().max(1.0) as u32));

	let primary_size = WindowRenderer::frozen_toolbar_primary_size(&session.toolbar_state);
	let primary_anchor =
		Pos2::new(160.0, screen_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - primary_size.y);
	let base_outer = session.toolbar_outer_position_from_primary_anchor(monitor, primary_anchor);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	WindowRenderer::sync_frozen_annotation_style_capsule_placement(
		&mut session.toolbar_state,
		screen_rect,
		primary_anchor,
	);

	assert_eq!(
		session.toolbar_state.annotation_style_capsule_placement,
		FrozenAnnotationStyleCapsulePlacement::Above
	);

	let mut below_toolbar_state = session.toolbar_state.clone();

	below_toolbar_state.annotation_style_capsule_placement =
		FrozenAnnotationStyleCapsulePlacement::Below;

	assert!(
		WindowRenderer::frozen_toolbar_window_rect(&below_toolbar_state, primary_anchor).max.y
			> screen_rect.max.y
	);
	assert_eq!(
		session.toolbar_outer_position_from_primary_anchor(monitor, primary_anchor),
		base_outer
	);
}

#[test]
fn pending_frozen_display_handoff_affordance_keeps_window_scrim_visible() {
	let ctx = tests::test_egui_context();
	let layer = LayerId::new(Order::Foreground, Id::new("pending-window-handoff"));
	let painter = ctx.layer_painter(layer);
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();
	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();

	state.mode = OverlayMode::Live;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(RectPoints::new(100, 120, 240, 320));

	assert!(WindowRenderer::render_pending_frozen_display_handoff_affordance(
		&ctx,
		&painter,
		&state,
		monitor,
		Some(monitor),
		screen_rect,
		HudTheme::Light,
		true,
		1.0,
		FrozenCaptureSource::Window,
		&mut selection_flow_geometry_cache,
		&mut selection_dashed_border_cache,
	));
	assert_eq!(selection_dashed_border_cache.key, None);
}

#[test]
fn pending_frozen_display_handoff_affordance_keeps_drag_border_visible() {
	let ctx = tests::test_egui_context();
	let layer = LayerId::new(Order::Foreground, Id::new("pending-drag-handoff"));
	let painter = ctx.layer_painter(layer);
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();
	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();

	state.mode = OverlayMode::Live;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(RectPoints::new(100, 120, 240, 320));

	assert!(WindowRenderer::render_pending_frozen_display_handoff_affordance(
		&ctx,
		&painter,
		&state,
		monitor,
		Some(monitor),
		screen_rect,
		HudTheme::Light,
		false,
		1.0,
		FrozenCaptureSource::DragRegion,
		&mut selection_flow_geometry_cache,
		&mut selection_dashed_border_cache,
	));
	assert!(selection_dashed_border_cache.key.is_some());
}

#[test]
fn pending_frozen_display_handoff_affordance_applies_after_preview_commit_before_toolbar_draw() {
	let ctx = tests::test_egui_context();
	let layer = LayerId::new(Order::Foreground, Id::new("pending-frozen-preview-handoff"));
	let painter = ctx.layer_painter(layer);
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();
	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();

	state.begin_freeze(monitor);

	state.frozen_capture_rect = Some(RectPoints::new(100, 120, 240, 320));

	state.commit_frozen_display_image(monitor, tests::test_frozen_image());

	assert!(WindowRenderer::render_pending_frozen_display_handoff_affordance(
		&ctx,
		&painter,
		&state,
		monitor,
		Some(monitor),
		screen_rect,
		HudTheme::Light,
		false,
		1.0,
		FrozenCaptureSource::DragRegion,
		&mut selection_flow_geometry_cache,
		&mut selection_dashed_border_cache,
	));
	assert!(selection_dashed_border_cache.key.is_some());
}

#[test]
fn pending_frozen_display_handoff_affordance_skips_non_target_monitor() {
	let ctx = tests::test_egui_context();
	let layer = LayerId::new(Order::Foreground, Id::new("pending-off-monitor-handoff"));
	let painter = ctx.layer_painter(layer);
	let target_monitor = tests::test_monitor();
	let other_monitor = MonitorRect {
		id: target_monitor.id + 1,
		origin: GlobalPoint::new(target_monitor.width as i32, 0),
		..target_monitor
	};
	let screen_rect = Rect::from_min_size(
		Pos2::new(other_monitor.origin.x as f32, other_monitor.origin.y as f32),
		Vec2::new(other_monitor.width as f32, other_monitor.height as f32),
	);
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();
	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();

	state.mode = OverlayMode::Live;
	state.monitor = Some(target_monitor);
	state.frozen_capture_rect = Some(RectPoints::new(100, 120, 240, 320));

	assert!(!WindowRenderer::render_pending_frozen_display_handoff_affordance(
		&ctx,
		&painter,
		&state,
		other_monitor,
		Some(target_monitor),
		screen_rect,
		HudTheme::Light,
		true,
		1.0,
		FrozenCaptureSource::Window,
		&mut selection_flow_geometry_cache,
		&mut selection_dashed_border_cache,
	));
	assert_eq!(selection_dashed_border_cache.key, None);
}

#[test]
fn pending_frozen_display_handoff_affordance_uses_pending_monitor_when_state_monitor_is_unset() {
	let ctx = tests::test_egui_context();
	let layer = LayerId::new(Order::Foreground, Id::new("pending-unset-monitor-handoff"));
	let painter = ctx.layer_painter(layer);
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();
	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();

	state.mode = OverlayMode::Live;
	state.monitor = None;
	state.frozen_capture_rect = Some(RectPoints::new(100, 120, 240, 320));

	assert!(WindowRenderer::render_pending_frozen_display_handoff_affordance(
		&ctx,
		&painter,
		&state,
		monitor,
		Some(monitor),
		screen_rect,
		HudTheme::Light,
		true,
		1.0,
		FrozenCaptureSource::Window,
		&mut selection_flow_geometry_cache,
		&mut selection_dashed_border_cache,
	));
	assert_eq!(selection_dashed_border_cache.key, None);
}
