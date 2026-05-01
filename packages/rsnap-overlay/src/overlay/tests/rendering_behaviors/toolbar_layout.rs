#[cfg(target_os = "macos")]
use crate::overlay::tests::rendering_behaviors::FrozenCaptureSource;
use crate::overlay::tests::rendering_behaviors::{
	FrozenToolbarState, FrozenToolbarTool, GlobalPoint, HUD_LOUPE_STRIP_GAP_POINTS, HudTheme,
	MonitorRect, OverlayMode, OverlaySession, OverlayState, Pos2, RawInput, Rect, RectPoints,
	TOOLBAR_CAPTURE_GAP_PX, TOOLBAR_SCREEN_MARGIN_PX, ToolbarPlacement, Ui, Vec2, WindowRenderer,
	overlay, tests,
};

#[test]
fn toolbar_position_update_queues_pending_move_without_window() {
	let monitor = tests::test_monitor();
	#[cfg(target_os = "macos")]
	let primary_origin = overlay::frozen_toolbar_window_primary_origin();
	let mut session = OverlaySession::new();

	session.toolbar_inner_size_points = Some((460, 54));

	assert!(session.update_toolbar_outer_position(monitor, Pos2::new(120.0, 160.0)));
	assert_eq!(session.toolbar_state.floating_position, Some(Pos2::new(120.0, 160.0)));

	#[cfg(target_os = "macos")]
	let expected_outer = GlobalPoint::new(120, (160.0 - primary_origin.y).round() as i32);
	#[cfg(not(target_os = "macos"))]
	let expected_outer = GlobalPoint::new(120, 160);

	assert_eq!(session.toolbar_outer_pos, Some(expected_outer));
	assert_eq!(session.pending_toolbar_outer_pos, Some(expected_outer));
}

#[test]
fn toolbar_window_position_sync_updates_runtime_state_without_requeueing_in_bounds_move() {
	let monitor = tests::test_monitor();
	#[cfg(target_os = "macos")]
	let primary_origin = overlay::frozen_toolbar_window_primary_origin();
	let mut session = OverlaySession::new();

	session.toolbar_inner_size_points = Some((460, 54));

	assert!(session.sync_toolbar_outer_position_from_window(monitor, GlobalPoint::new(120, 160)));

	#[cfg(target_os = "macos")]
	let expected_anchor = Pos2::new(120.0, 160.0 + primary_origin.y);
	#[cfg(not(target_os = "macos"))]
	let expected_anchor = Pos2::new(120.0, 160.0);

	assert_eq!(session.toolbar_state.floating_position, Some(expected_anchor));
	assert_eq!(session.toolbar_outer_pos, Some(GlobalPoint::new(120, 160)));
	assert_eq!(session.pending_toolbar_outer_pos, None);
}

#[test]
fn toolbar_position_update_near_edge_clamps_with_runtime_positioning_geometry() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();
	let desired = Pos2::new(900.0, 760.0);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let startup_size = overlay::frozen_toolbar_window_startup_size_points();
	let positioning_size = WindowRenderer::frozen_toolbar_positioning_size(&session.toolbar_state);
	#[cfg(target_os = "macos")]
	let primary_origin = overlay::frozen_toolbar_window_primary_origin();
	#[cfg(not(target_os = "macos"))]
	let startup_window_size = Vec2::new(startup_size.x, startup_size.y);

	session.toolbar_inner_size_points =
		Some((startup_size.x.ceil().max(1.0) as u32, startup_size.y.ceil().max(1.0) as u32));

	let expected_toolbar_size = positioning_size;
	let expected = WindowRenderer::clamp_toolbar_position(
		screen_rect,
		expected_toolbar_size,
		desired,
		TOOLBAR_SCREEN_MARGIN_PX,
		TOOLBAR_SCREEN_MARGIN_PX,
	);

	#[cfg(not(target_os = "macos"))]
	assert_ne!(
		expected,
		WindowRenderer::clamp_toolbar_position(
			screen_rect,
			startup_window_size,
			desired,
			TOOLBAR_SCREEN_MARGIN_PX,
			TOOLBAR_SCREEN_MARGIN_PX,
		)
	);
	assert!(session.update_toolbar_outer_position(monitor, desired));
	assert_eq!(session.toolbar_state.floating_position, Some(expected));

	#[cfg(target_os = "macos")]
	let expected_outer =
		GlobalPoint::new(expected.x.round() as i32, (expected.y - primary_origin.y).round() as i32);
	#[cfg(not(target_os = "macos"))]
	let expected_outer = GlobalPoint::new(expected.x.round() as i32, expected.y.round() as i32);

	assert_eq!(session.toolbar_outer_pos, Some(expected_outer));
}

#[test]
fn toolbar_window_position_sync_near_edge_clamps_with_runtime_positioning_geometry() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();
	let desired_outer = GlobalPoint::new(900, 760);
	let desired_local = Pos2::new(desired_outer.x as f32, desired_outer.y as f32);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let startup_size = overlay::frozen_toolbar_window_startup_size_points();
	let positioning_size = WindowRenderer::frozen_toolbar_positioning_size(&session.toolbar_state);
	#[cfg(target_os = "macos")]
	let primary_origin = overlay::frozen_toolbar_window_primary_origin();
	#[cfg(not(target_os = "macos"))]
	let startup_window_size = Vec2::new(startup_size.x, startup_size.y);

	session.toolbar_inner_size_points =
		Some((startup_size.x.ceil().max(1.0) as u32, startup_size.y.ceil().max(1.0) as u32));

	let expected_toolbar_size = positioning_size;
	let expected = WindowRenderer::clamp_toolbar_position(
		screen_rect,
		expected_toolbar_size,
		desired_local,
		TOOLBAR_SCREEN_MARGIN_PX,
		TOOLBAR_SCREEN_MARGIN_PX,
	);

	#[cfg(not(target_os = "macos"))]
	assert_ne!(
		expected,
		WindowRenderer::clamp_toolbar_position(
			screen_rect,
			startup_window_size,
			desired_local,
			TOOLBAR_SCREEN_MARGIN_PX,
			TOOLBAR_SCREEN_MARGIN_PX,
		)
	);
	assert!(session.sync_toolbar_outer_position_from_window(monitor, desired_outer));
	assert_eq!(session.toolbar_state.floating_position, Some(expected));

	#[cfg(target_os = "macos")]
	let expected_outer =
		GlobalPoint::new(expected.x.round() as i32, (expected.y - primary_origin.y).round() as i32);
	#[cfg(not(target_os = "macos"))]
	let expected_outer = GlobalPoint::new(expected.x.round() as i32, expected.y.round() as i32);

	assert_eq!(session.toolbar_outer_pos, Some(expected_outer));
	assert_eq!(
		session.pending_toolbar_outer_pos,
		(session.toolbar_outer_pos != Some(desired_outer)).then_some(expected_outer)
	);
}

#[test]
fn toolbar_position_update_clamps_scroll_mode_pen_from_primary_anchor_width() {
	let monitor = tests::test_monitor();
	let desired = Pos2::new(900.0, 160.0);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let startup_size = overlay::frozen_toolbar_window_startup_size_points();
	let primary_toolbar_state = FrozenToolbarState {
		selected_tool: FrozenToolbarTool::Pen,
		scroll_capture_active: true,
		..FrozenToolbarState::default()
	};
	let primary_size = WindowRenderer::frozen_toolbar_primary_size(&primary_toolbar_state);
	let full_toolbar_size = WindowRenderer::frozen_toolbar_size(&primary_toolbar_state);
	#[cfg(target_os = "macos")]
	let primary_origin = overlay::frozen_toolbar_window_primary_origin();
	let mut session = OverlaySession::new();

	assert!(
		full_toolbar_size.x > primary_size.x,
		"regression setup requires the secondary style capsule to be wider than the primary pill",
	);

	session.toolbar_state = primary_toolbar_state;
	session.toolbar_inner_size_points =
		Some((startup_size.x.ceil().max(1.0) as u32, startup_size.y.ceil().max(1.0) as u32));

	let expected = WindowRenderer::clamp_toolbar_position(
		screen_rect,
		primary_size,
		desired,
		TOOLBAR_SCREEN_MARGIN_PX,
		TOOLBAR_SCREEN_MARGIN_PX,
	);
	let union_clamped = WindowRenderer::clamp_toolbar_position(
		screen_rect,
		full_toolbar_size,
		desired,
		TOOLBAR_SCREEN_MARGIN_PX,
		TOOLBAR_SCREEN_MARGIN_PX,
	);

	assert_ne!(expected, union_clamped);
	assert!(session.update_toolbar_outer_position(monitor, desired));
	assert_eq!(session.toolbar_state.floating_position, Some(expected));

	#[cfg(target_os = "macos")]
	let expected_outer =
		GlobalPoint::new(expected.x.round() as i32, (expected.y - primary_origin.y).round() as i32);
	#[cfg(not(target_os = "macos"))]
	let expected_outer = GlobalPoint::new(expected.x.round() as i32, expected.y.round() as i32);

	assert_eq!(session.toolbar_outer_pos, Some(expected_outer));
}

#[test]
fn toolbar_event_outer_position_uses_best_available_source() {
	let monitor = tests::test_monitor();
	let window_outer_pos = Some(GlobalPoint::new(220, 260));
	let cached_outer_pos = Some(GlobalPoint::new(340, 420));
	let floating_position = Some(Pos2::new(80.4, 90.6));
	#[cfg(target_os = "macos")]
	let primary_origin = overlay::frozen_toolbar_window_primary_origin();
	#[cfg(target_os = "macos")]
	let floating_outer_pos = GlobalPoint::new(80, (90.6 - primary_origin.y).round() as i32);
	#[cfg(not(target_os = "macos"))]
	let floating_outer_pos = GlobalPoint::new(80, 91);

	for (label, window, cached, floating, expected) in [
		(
			"window position wins",
			window_outer_pos,
			cached_outer_pos,
			floating_position,
			window_outer_pos,
		),
		(
			"cached position wins without window",
			None,
			cached_outer_pos,
			floating_position,
			cached_outer_pos,
		),
		(
			"floating position is last fallback",
			None,
			None,
			floating_position,
			Some(floating_outer_pos),
		),
	] {
		assert_eq!(
			OverlaySession::toolbar_event_outer_position_from_sources(
				monitor, window, cached, floating,
			),
			expected,
			"{label}",
		);
	}
}

#[test]
fn frozen_toolbar_primary_size_stays_stable_when_annotation_style_capsule_appears() {
	let mut toolbar_state = FrozenToolbarState::default();
	let base_primary_size = WindowRenderer::frozen_toolbar_primary_size(&toolbar_state);
	let base_window_size = WindowRenderer::frozen_toolbar_size(&toolbar_state);

	toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	let pen_primary_size = WindowRenderer::frozen_toolbar_primary_size(&toolbar_state);
	let pen_window_size = WindowRenderer::frozen_toolbar_size(&toolbar_state);

	assert_eq!(pen_primary_size, base_primary_size);
	assert!(pen_window_size.x >= base_window_size.x);
	assert!(pen_window_size.y > base_window_size.y);

	toolbar_state.selected_tool = FrozenToolbarTool::Text;

	let text_primary_size = WindowRenderer::frozen_toolbar_primary_size(&toolbar_state);
	let text_window_size = WindowRenderer::frozen_toolbar_size(&toolbar_state);

	assert_eq!(text_primary_size, base_primary_size);
	assert!(text_window_size.x >= base_window_size.x);
	assert!(text_window_size.y > base_window_size.y);
	assert!(pen_window_size.y >= text_window_size.y);
}

#[test]
fn render_frozen_toolbar_ui_keeps_runtime_drag_when_pointer_snapshot_is_missing() {
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

	session.toolbar_state.dragging = true;

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

	assert!(hud_pill.is_some(), "toolbar should still render once readiness stabilizes");
	assert!(
		session.toolbar_state.dragging,
		"rendering without a pointer snapshot must not clear runtime-managed drag state"
	);
}

#[cfg(target_os = "macos")]
#[test]
fn seeded_frozen_toolbar_default_slot_uses_positioning_window_size() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(760, 160, 160, 240);
	let startup_size = overlay::frozen_toolbar_window_startup_size_points();
	let mut session = OverlaySession::new();

	session.toolbar_inner_size_points =
		Some((startup_size.x.ceil().max(1.0) as u32, startup_size.y.ceil().max(1.0) as u32));

	session.seed_frozen_toolbar_default_position(monitor, capture_rect);

	assert_eq!(
		session.toolbar_state.default_slot_position,
		session.toolbar_state.floating_position
	);
}

#[test]
fn frozen_toolbar_default_position_centers_on_capture_rect_midpoint() {
	let monitor = tests::test_monitor_with_scale(400, 300, 2_000);
	let capture_rect = RectPoints::new(150, 100, 100, 60);
	let session = OverlaySession::new();
	let toolbar_size = WindowRenderer::frozen_toolbar_size(&session.toolbar_state);
	let toolbar_pos =
		session.frozen_toolbar_default_position_for_capture_rect(monitor, capture_rect);
	let toolbar_midpoint_x = toolbar_pos.x + toolbar_size.x * 0.5;
	let capture_midpoint_x = capture_rect.x as f32 + capture_rect.width as f32 * 0.5;

	assert_eq!(toolbar_midpoint_x, capture_midpoint_x);
}

#[test]
fn frozen_toolbar_default_position_places_or_falls_inside_for_configured_edge() {
	let toolbar_size = Vec2::new(460.0, 54.0);

	for (label, monitor, capture_rect, placement, expected_y) in [
		(
			"bottom placement fits below capture rect",
			Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)),
			Rect::from_min_size(Pos2::new(50.0, 100.0), Vec2::new(300.0, 200.0)),
			ToolbarPlacement::Bottom,
			100.0 + 200.0 + TOOLBAR_CAPTURE_GAP_PX,
		),
		(
			"bottom placement falls inside when below overflows",
			Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 600.0)),
			Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 560.0)),
			ToolbarPlacement::Bottom,
			560.0 - TOOLBAR_SCREEN_MARGIN_PX - toolbar_size.y,
		),
		(
			"top placement fits above capture rect",
			Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)),
			Rect::from_min_size(Pos2::new(50.0, 180.0), Vec2::new(300.0, 200.0)),
			ToolbarPlacement::Top,
			180.0 - TOOLBAR_CAPTURE_GAP_PX - toolbar_size.y,
		),
		(
			"top placement falls inside when above overflows",
			Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 600.0)),
			Rect::from_min_size(Pos2::new(0.0, 20.0), Vec2::new(500.0, 400.0)),
			ToolbarPlacement::Top,
			20.0 + TOOLBAR_SCREEN_MARGIN_PX,
		),
	] {
		let pos = WindowRenderer::frozen_toolbar_default_window_pos(
			monitor,
			capture_rect,
			toolbar_size,
			toolbar_size,
			placement,
		);
		let expected_x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(
			TOOLBAR_SCREEN_MARGIN_PX,
			(monitor.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX)
				.max(TOOLBAR_SCREEN_MARGIN_PX),
		);

		assert_eq!(pos.x, expected_x, "{label}");
		assert_eq!(pos.y, expected_y, "{label}");
	}
}

#[test]
fn frozen_toolbar_default_window_position_clamps_using_primary_anchor_width() {
	let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
	let capture_rect = Rect::from_min_size(Pos2::new(640.0, 120.0), Vec2::new(120.0, 220.0));
	let toolbar_primary_size = Vec2::new(268.0, 54.0);
	let secondary_union_size = Vec2::new(340.0, 82.0);
	let pos = WindowRenderer::frozen_toolbar_default_window_pos(
		monitor,
		capture_rect,
		toolbar_primary_size,
		toolbar_primary_size,
		ToolbarPlacement::Bottom,
	);
	let ideal_x = capture_rect.center().x - toolbar_primary_size.x / 2.0;
	let max_x = (monitor.max.x - toolbar_primary_size.x - TOOLBAR_SCREEN_MARGIN_PX)
		.max(TOOLBAR_SCREEN_MARGIN_PX);
	let secondary_union_max_x = (monitor.max.x - secondary_union_size.x - TOOLBAR_SCREEN_MARGIN_PX)
		.max(TOOLBAR_SCREEN_MARGIN_PX);

	assert!(ideal_x > max_x);
	assert_ne!(max_x, secondary_union_max_x);
	assert_eq!(pos.x, max_x);
	assert_eq!(pos.y, capture_rect.max.y + TOOLBAR_CAPTURE_GAP_PX);
}

#[test]
fn overlay_session_computes_frozen_toolbar_reserved_rect_without_inline_toolbar_state() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(RectPoints::new(200, 180, 200, 300));

	let reserved_rect = session
		.frozen_size_badge_toolbar_reserved_rect(monitor, screen_rect, true)
		.expect("overlay redraw should reserve the default toolbar slot");

	assert_eq!(reserved_rect.min.y, 480.0 + TOOLBAR_CAPTURE_GAP_PX);
	assert_eq!(
		reserved_rect.height(),
		WindowRenderer::frozen_toolbar_size(&session.toolbar_state).y
	);
}

#[test]
fn frozen_toolbar_reserved_rect_covers_secondary_annotation_capsule() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let capture_rect_points = RectPoints::new(200, 180, 200, 300);
	let capture_rect = Rect::from_min_size(
		Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
		Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
	);
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(capture_rect_points);

	let toolbar_state = FrozenToolbarState {
		selected_tool: FrozenToolbarTool::Text,
		..FrozenToolbarState::default()
	};
	let primary_size = WindowRenderer::frozen_toolbar_primary_size(&toolbar_state);
	let default_pos = WindowRenderer::frozen_toolbar_default_window_pos(
		screen_rect,
		capture_rect,
		primary_size,
		WindowRenderer::frozen_toolbar_size(&toolbar_state),
		ToolbarPlacement::Bottom,
	);
	let reserved_rect = WindowRenderer::frozen_toolbar_reserved_rect(
		&state,
		monitor,
		screen_rect,
		ToolbarPlacement::Bottom,
		&toolbar_state,
	)
	.expect("annotation style capsule should be reserved with the default slot");

	assert_eq!(reserved_rect.min, default_pos);
	assert_eq!(reserved_rect.size(), WindowRenderer::frozen_toolbar_size(&toolbar_state));
}

#[test]
fn frozen_toolbar_reserved_rect_uses_overlay_viewport_size() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 400,
		height: 260,
		scale_factor_x1000: 1_000,
	};
	let overlay_screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 120.0));
	let toolbar_window_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(92.0, 26.0));
	let capture_rect_points = RectPoints::new(60, 40, 220, 60);
	let capture_rect = Rect::from_min_size(
		Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
		Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
	);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(capture_rect_points);
	session.toolbar_state.layout_last_screen_size_points = Some(toolbar_window_rect.size());

	let toolbar_size = WindowRenderer::frozen_toolbar_size(&session.toolbar_state);
	let overlay_default_pos = WindowRenderer::frozen_toolbar_default_window_pos(
		overlay_screen_rect,
		capture_rect.intersect(overlay_screen_rect),
		toolbar_size,
		toolbar_size,
		session.config.toolbar_placement,
	);
	let toolbar_window_default_pos = WindowRenderer::frozen_toolbar_default_window_pos(
		toolbar_window_rect,
		capture_rect.intersect(toolbar_window_rect),
		toolbar_size,
		toolbar_size,
		session.config.toolbar_placement,
	);

	session.toolbar_state.floating_position = Some(overlay_default_pos);

	let reserved_rect = session
		.frozen_size_badge_toolbar_reserved_rect(monitor, overlay_screen_rect, true)
		.expect("overlay viewport-aligned toolbar slot should still be reserved");

	assert_ne!(overlay_default_pos, toolbar_window_default_pos);
	assert_eq!(reserved_rect.min, overlay_default_pos);
	assert_eq!(reserved_rect.size(), toolbar_size);
}

#[test]
fn frozen_toolbar_reserved_rect_skips_hidden_toolbar_slot() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(RectPoints::new(200, 180, 200, 300));

	assert_eq!(session.frozen_size_badge_toolbar_reserved_rect(monitor, screen_rect, false), None);
}

#[test]
fn frozen_toolbar_reserved_rect_waits_for_toolbar_birth_readiness() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(RectPoints::new(200, 180, 200, 300));
	session.toolbar_state.layout_last_screen_size_points = Some(screen_rect.size());
	session.toolbar_state.layout_stable_frames = 0;

	assert!(!session.frozen_toolbar_ready_for_draw(screen_rect));
	assert_eq!(
		session.frozen_size_badge_toolbar_reserved_rect(
			monitor,
			screen_rect,
			session.frozen_toolbar_ready_for_draw(screen_rect)
		),
		None
	);

	session.toolbar_state.layout_stable_frames = 1;

	assert!(session.frozen_toolbar_ready_for_draw(screen_rect));
	assert!(
		session
			.frozen_size_badge_toolbar_reserved_rect(
				monitor,
				screen_rect,
				session.frozen_toolbar_ready_for_draw(screen_rect)
			)
			.is_some()
	);
}

#[test]
fn frozen_toolbar_ready_for_draw_ignores_preseeded_position_until_viewport_stabilizes() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	assert!(session.toolbar_state.floating_position.is_some());
	assert_eq!(session.toolbar_state.layout_last_screen_size_points, None);
	assert_eq!(session.toolbar_state.layout_stable_frames, 0);
	assert!(!session.frozen_toolbar_ready_for_draw(screen_rect));
	assert_eq!(
		session.frozen_size_badge_toolbar_reserved_rect(
			monitor,
			screen_rect,
			session.frozen_toolbar_ready_for_draw(screen_rect)
		),
		None
	);
}

#[test]
fn frozen_toolbar_ready_for_draw_recovers_after_preseeded_position_is_sampled() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));
	assert_eq!(session.toolbar_state.layout_last_screen_size_points, Some(screen_rect.size()));
	assert_eq!(session.toolbar_state.layout_stable_frames, 0);
	assert!(!session.frozen_toolbar_ready_for_draw(screen_rect));
	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));
	assert_eq!(session.toolbar_state.layout_stable_frames, 1);
	assert!(session.frozen_toolbar_ready_for_draw(screen_rect));
}

#[test]
fn render_frozen_toolbar_ui_waits_for_readiness_before_first_visible_frame() {
	let ctx = tests::test_egui_context();
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();
	let toolbar_placement = session.config.toolbar_placement;

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	assert!(session.toolbar_state.visible);
	assert_eq!(session.toolbar_state.layout_last_screen_size_points, None);
	assert_eq!(session.toolbar_state.layout_stable_frames, 0);

	for frame in 0..2 {
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

		assert!(
			hud_pill.is_none(),
			"frame {frame} should not draw the toolbar before readiness stabilizes"
		);
	}

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

	assert!(hud_pill.is_some(), "third frame should draw the stabilized toolbar");
}

#[test]
fn frozen_toolbar_reserved_rect_restores_near_default_slot() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let capture_rect = Rect::from_min_size(Pos2::new(200.0, 180.0), Vec2::new(200.0, 300.0));
	let mut state = OverlayState::new();
	let mut toolbar_state = FrozenToolbarState::default();
	let toolbar_size = WindowRenderer::frozen_toolbar_size(&toolbar_state);
	let default_pos = WindowRenderer::frozen_toolbar_default_window_pos(
		screen_rect,
		capture_rect,
		toolbar_size,
		toolbar_size,
		ToolbarPlacement::Bottom,
	);
	let restored_pos = default_pos + Vec2::new(0.4, -0.35);

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(RectPoints::new(200, 180, 200, 300));
	toolbar_state.visible = true;
	toolbar_state.floating_position = Some(restored_pos);

	assert_eq!(
		WindowRenderer::frozen_toolbar_reserved_rect(
			&state,
			monitor,
			screen_rect,
			ToolbarPlacement::Bottom,
			&toolbar_state,
		),
		Some(Rect::from_min_size(restored_pos, toolbar_size))
	);
}

#[test]
fn frozen_toolbar_overlay_viewport_sample_recovers_from_toolbar_window_pollution() {
	let monitor = tests::test_monitor();
	let overlay_screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let toolbar_window_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(92.0, 26.0));
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(RectPoints::new(200, 180, 200, 300));
	session.toolbar_state.layout_last_screen_size_points = Some(toolbar_window_rect.size());
	session.toolbar_state.layout_stable_frames = 1;

	assert!(!session.frozen_toolbar_ready_for_draw(overlay_screen_rect));
	assert!(!session.advance_frozen_toolbar_readiness_sample(overlay_screen_rect));
	assert_eq!(
		session.toolbar_state.layout_last_screen_size_points,
		Some(overlay_screen_rect.size())
	);
	assert_eq!(session.toolbar_state.layout_stable_frames, 0);
	assert_eq!(
		session.frozen_size_badge_toolbar_reserved_rect(
			monitor,
			overlay_screen_rect,
			session.frozen_toolbar_ready_for_draw(overlay_screen_rect)
		),
		None
	);
	assert!(!session.advance_frozen_toolbar_readiness_sample(overlay_screen_rect));
	assert_eq!(session.toolbar_state.layout_stable_frames, 1);
	assert_eq!(
		session.frozen_size_badge_toolbar_reserved_rect(
			monitor,
			overlay_screen_rect,
			session.frozen_toolbar_ready_for_draw(overlay_screen_rect)
		),
		Some(
			WindowRenderer::frozen_toolbar_reserved_rect(
				&session.state,
				monitor,
				overlay_screen_rect,
				session.config.toolbar_placement,
				&session.toolbar_state,
			)
			.expect("reserved rect after overlay viewport stabilization")
		)
	);
}

#[test]
fn live_loupe_default_position_hangs_below_hud_or_falls_above_on_overflow() {
	for (label, monitor_height, hud_outer, expected_y) in [
		("space below hud", 600, GlobalPoint::new(220, 120), 120 + 52 + HUD_LOUPE_STRIP_GAP_POINTS),
		(
			"below hud overflows",
			500,
			GlobalPoint::new(220, 300),
			300 - HUD_LOUPE_STRIP_GAP_POINTS - 232,
		),
	] {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 800,
			height: monitor_height,
			scale_factor_x1000: 1_000,
		};
		let pos = OverlaySession::live_loupe_default_position(
			monitor,
			Some(GlobalPoint::new(100, 100)),
			Some(hud_outer),
			Some(52),
			232,
			232,
		)
		.unwrap();

		assert_eq!(pos.x, hud_outer.x, "{label}");
		assert_eq!(pos.y, expected_y, "{label}");
	}
}

#[test]
fn scroll_toolbar_compacts_to_two_buttons() {
	let frozen_toolbar_size = WindowRenderer::frozen_toolbar_size(&FrozenToolbarState::default());
	let scroll_toolbar_size = WindowRenderer::frozen_toolbar_size(&FrozenToolbarState {
		scroll_capture_active: true,
		..FrozenToolbarState::default()
	});

	assert!(scroll_toolbar_size.x < frozen_toolbar_size.x);
	assert_eq!(scroll_toolbar_size.y, frozen_toolbar_size.y);
}

#[cfg(target_os = "macos")]
#[test]
fn drag_region_toolbar_size_stays_stable_while_final_capture_readiness_changes() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(120, 160, 320, 240));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	session.sync_frozen_toolbar_state();

	let pending_toolbar_size = WindowRenderer::frozen_toolbar_size(&session.toolbar_state);
	let pending_tools = WindowRenderer::frozen_toolbar_tools(&session.toolbar_state);

	assert!(!session.toolbar_state.final_capture_ready);
	assert!(pending_tools.contains(&FrozenToolbarTool::AutoCenter));
	assert!(pending_tools.contains(&FrozenToolbarTool::Scroll));

	tests::promote_session_export_authority_ready(&mut session);

	session.sync_frozen_toolbar_state();

	let ready_toolbar_size = WindowRenderer::frozen_toolbar_size(&session.toolbar_state);
	let ready_tools = WindowRenderer::frozen_toolbar_tools(&session.toolbar_state);

	assert!(session.toolbar_state.final_capture_ready);
	assert!(ready_tools.contains(&FrozenToolbarTool::AutoCenter));
	assert!(ready_tools.contains(&FrozenToolbarTool::Scroll));
	assert_eq!(pending_toolbar_size, ready_toolbar_size);
}

#[cfg(target_os = "macos")]
#[test]
fn drag_region_toolbar_recenters_when_auto_center_appears_after_preview_commit() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	let seeded_pos = session
		.toolbar_state
		.floating_position
		.expect("toolbar should seed before frozen preview is ready");
	let seeded_size = WindowRenderer::frozen_toolbar_size(&session.toolbar_state);
	let capture_midpoint_x = capture_rect.x as f32 + capture_rect.width as f32 * 0.5;

	assert!(!session.toolbar_state.auto_center_available);
	assert_eq!(seeded_pos.x + seeded_size.x * 0.5, capture_midpoint_x);

	session.commit_frozen_preview(monitor, tests::test_frozen_image(), None);
	session.sync_frozen_toolbar_state();

	let ready_size = WindowRenderer::frozen_toolbar_size(&session.toolbar_state);

	assert!(session.toolbar_state.auto_center_available);
	assert!(ready_size.x > seeded_size.x);
	assert!(session.maybe_recenter_frozen_toolbar_default_slot(monitor));

	let recentered_pos =
		session.toolbar_state.floating_position.expect("toolbar should keep a default position");

	assert_eq!(recentered_pos.x + ready_size.x * 0.5, capture_midpoint_x);
	assert_eq!(session.toolbar_state.default_slot_position, Some(recentered_pos));
}

#[cfg(target_os = "macos")]
#[test]
fn late_toolbar_width_change_preserves_manual_toolbar_move() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	let seeded_default_pos = session
		.toolbar_state
		.floating_position
		.expect("toolbar should seed before frozen preview is ready");
	let moved_pos = seeded_default_pos + Vec2::new(24.0, 0.0);

	session.toolbar_state.floating_position = Some(moved_pos);

	session.commit_frozen_preview(monitor, tests::test_frozen_image(), None);
	session.sync_frozen_toolbar_state();

	assert!(!session.maybe_recenter_frozen_toolbar_default_slot(monitor));
	assert_eq!(session.toolbar_state.floating_position, Some(moved_pos));
	assert_eq!(
		session.toolbar_state.default_slot_position,
		Some(session.frozen_toolbar_default_position_for_capture_rect(monitor, capture_rect))
	);
}
#[test]
fn auto_center_toolbar_tool_only_appears_when_available() {
	let default_tools = WindowRenderer::frozen_toolbar_tools(&FrozenToolbarState::default());
	let auto_center_tools = WindowRenderer::frozen_toolbar_tools(&FrozenToolbarState {
		auto_center_available: true,
		..FrozenToolbarState::default()
	});

	assert!(!default_tools.contains(&FrozenToolbarTool::AutoCenter));
	assert!(auto_center_tools.contains(&FrozenToolbarTool::AutoCenter));

	#[cfg(target_os = "macos")]
	{
		assert!(default_tools.contains(&FrozenToolbarTool::Ocr));
		assert!(auto_center_tools.contains(&FrozenToolbarTool::Ocr));
	}
}

#[test]
fn toolbar_window_startup_size_covers_every_tool_permutation() {
	let startup_size = overlay::frozen_toolbar_window_startup_size_points();
	let toolbar_states = [
		FrozenToolbarState::default(),
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Pen,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Text,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState { auto_center_available: true, ..FrozenToolbarState::default() },
		FrozenToolbarState { scroll_capture_available: true, ..FrozenToolbarState::default() },
		FrozenToolbarState {
			auto_center_available: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Pen,
			auto_center_available: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Text,
			auto_center_available: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			scroll_capture_active: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
	];

	for toolbar_state in toolbar_states {
		let toolbar_size = WindowRenderer::frozen_toolbar_size(&toolbar_state);

		assert!(
			startup_size.x >= toolbar_size.x,
			"startup width {} should cover toolbar width {} for {toolbar_state:?}",
			startup_size.x,
			toolbar_size.x
		);
		assert!(
			startup_size.y >= toolbar_size.y,
			"startup height {} should cover toolbar height {} for {toolbar_state:?}",
			startup_size.y,
			toolbar_size.y
		);
	}
}
