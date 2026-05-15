#[cfg(target_os = "macos")]
use crate::overlay::tests::rendering_behaviors::OverlaySession;
use crate::overlay::tests::rendering_behaviors::{
	FrozenCaptureSource, FrozenToolbarState, GlobalPoint, HudTheme, Id, LayerId, MonitorRectPoints,
	Order, OverlayMode, OverlayState, Pos2, Rect, RectPoints, SELECTION_SIZE_BADGE_GAP_PX,
	SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX, SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX,
	SelectionDashedBorderCache, SelectionFlowGeometryCache, SelectionSizeBadgeTarget,
	TOOLBAR_CAPTURE_GAP_PX, Vec2, WindowRenderer, tests,
};

#[test]
fn selection_size_badge_rect_uses_visible_slot_for_common_edges() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));

	for (label, capture_rect, expected_min_y, expected_max_y, expected_min_x) in [
		(
			"fits below",
			Rect::from_min_size(Pos2::new(120.0, 160.0), Vec2::new(320.0, 240.0)),
			Some(160.0 + 240.0 + SELECTION_SIZE_BADGE_GAP_PX),
			None,
			None,
		),
		(
			"falls inside when no space below",
			Rect::from_min_size(Pos2::new(120.0, 420.0), Vec2::new(320.0, 160.0)),
			None,
			Some(420.0 + 160.0 - SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX),
			None,
		),
		(
			"clamps left edge",
			Rect::from_min_size(Pos2::new(0.0, 160.0), Vec2::new(40.0, 120.0)),
			None,
			None,
			Some(screen_rect.min.x),
		),
		(
			"clamps narrow near-left capture",
			Rect::from_min_size(Pos2::new(20.0, 160.0), Vec2::new(40.0, 120.0)),
			None,
			None,
			Some(screen_rect.min.x),
		),
		(
			"keeps tiny bottom capture visible",
			Rect::from_min_size(Pos2::new(120.0, 588.0), Vec2::new(140.0, 12.0)),
			None,
			Some(screen_rect.max.y),
			None,
		),
	] {
		let badge_rect = WindowRenderer::selection_size_badge_rect(
			screen_rect,
			capture_rect,
			Vec2::new(92.0, 26.0),
		);

		if let Some(expected_min_y) = expected_min_y {
			assert_eq!(badge_rect.min.y, expected_min_y, "{label}");
		}
		if let Some(expected_max_y) = expected_max_y {
			assert_eq!(badge_rect.max.y, expected_max_y, "{label}");
		}
		if let Some(expected_min_x) = expected_min_x {
			assert_eq!(badge_rect.min.x, expected_min_x, "{label}");
			assert!(badge_rect.max.x > capture_rect.max.x, "{label}");
		} else {
			assert_eq!(badge_rect.max.x, capture_rect.max.x, "{label}");
		}

		if label == "falls inside when no space below" {
			assert!(
				badge_rect.max.y <= screen_rect.max.y - SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX,
				"{label}",
			);
		}
		if label == "keeps tiny bottom capture visible" {
			assert!(badge_rect.min.y < capture_rect.min.y, "{label}");
			assert!(badge_rect.min.y >= screen_rect.min.y, "{label}");
		}
	}
}

#[test]
fn frozen_drag_region_selection_size_badge_uses_above_slot_when_bottom_toolbar_slot_overlaps() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let capture_rect = Rect::from_min_size(Pos2::new(200.0, 180.0), Vec2::new(200.0, 300.0));
	let reserved_rect = Rect::from_min_size(
		Pos2::new(200.0, 180.0 + 300.0 + TOOLBAR_CAPTURE_GAP_PX),
		WindowRenderer::frozen_toolbar_size(&FrozenToolbarState::default()),
	);
	let badge_rect =
		WindowRenderer::selection_size_badge_rect_preferring_outside_with_reserved_rect(
			screen_rect,
			capture_rect,
			Vec2::new(92.0, 26.0),
			Some(reserved_rect),
		);

	assert_eq!(badge_rect.max.x, capture_rect.max.x);
	assert_eq!(badge_rect.max.y, capture_rect.min.y - SELECTION_SIZE_BADGE_GAP_PX);
	assert!(!badge_rect.intersects(reserved_rect));
}

#[test]
fn frozen_window_selection_size_badge_keeps_inside_fallback_when_bottom_toolbar_slot_overlaps() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let capture_rect = Rect::from_min_size(Pos2::new(200.0, 180.0), Vec2::new(200.0, 300.0));
	let reserved_rect = Rect::from_min_size(
		Pos2::new(200.0, 180.0 + 300.0 + TOOLBAR_CAPTURE_GAP_PX),
		WindowRenderer::frozen_toolbar_size(&FrozenToolbarState::default()),
	);
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

	assert_eq!(badge_rect.max.x, capture_rect.max.x);
	assert_eq!(badge_rect.max.y, capture_rect.max.y - SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX);
	assert!(!badge_rect.intersects(reserved_rect));
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_toolbar_badge_visibility_waits_for_first_toolbar_draw() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.sync_frozen_toolbar_state();

	assert!(session.maybe_recenter_frozen_toolbar_default_slot(monitor));
	assert!(!session.toolbar_window_visible);
	assert!(!session.should_hide_toolbar_window(monitor));
	assert!(!session.frozen_toolbar_badge_visibility(monitor, screen_rect, false));
	assert_eq!(session.toolbar_state.layout_last_screen_size_points, Some(screen_rect.size()));
	assert_eq!(session.toolbar_state.layout_stable_frames, 0);
	assert!(!session.frozen_toolbar_badge_visibility(monitor, screen_rect, false));
	assert_eq!(session.toolbar_state.layout_stable_frames, 1);

	session.toolbar_window_visible = true;

	assert!(!session.frozen_toolbar_badge_visibility(monitor, screen_rect, false));

	session.toolbar_window_drawn_once = true;

	assert!(!session.frozen_toolbar_badge_visibility(monitor, screen_rect, false));

	session.toolbar_badge_slot_ready = true;

	assert!(session.frozen_toolbar_badge_visibility(monitor, screen_rect, false));
	assert!(session.frozen_size_badge_toolbar_reserved_rect(monitor, screen_rect, true).is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_toolbar_badge_visibility_resets_for_new_frozen_transition() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.toolbar_window_drawn_once = true;

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	assert!(!session.toolbar_window_drawn_once);
	assert!(!session.toolbar_badge_slot_ready);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.sync_frozen_toolbar_state();

	assert!(session.maybe_recenter_frozen_toolbar_default_slot(monitor));
	assert!(!session.frozen_toolbar_badge_visibility(monitor, screen_rect, false));
	assert_eq!(session.toolbar_state.layout_stable_frames, 0);
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_toolbar_badge_visibility_waits_for_overlay_frame_after_first_toolbar_draw() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.sync_frozen_toolbar_state();

	assert!(session.maybe_recenter_frozen_toolbar_default_slot(monitor));
	assert!(!session.frozen_toolbar_badge_visibility(monitor, screen_rect, false));
	assert!(!session.frozen_toolbar_badge_visibility(monitor, screen_rect, false));

	session.toolbar_window_visible = true;
	session.toolbar_window_drawn_once = true;

	assert!(!session.frozen_toolbar_badge_visibility(monitor, screen_rect, false));

	session.toolbar_badge_slot_ready = true;

	assert!(session.frozen_toolbar_badge_visibility(monitor, screen_rect, false));
}

#[test]
fn selection_size_badge_reserved_rect_prefers_upper_band_when_bottom_space_is_reserved() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 220.0));
	let capture_rect = Rect::from_min_size(Pos2::new(40.0, 40.0), Vec2::new(200.0, 150.0));
	let reserved_rect = Rect::from_min_size(Pos2::new(80.0, 140.0), Vec2::new(120.0, 40.0));
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

	assert_eq!(
		badge_rect.min.y,
		reserved_rect.min.y - SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX - 26.0
	);
	assert!(!badge_rect.intersects(reserved_rect));
}

#[test]
fn selection_size_badge_reserved_rect_keeps_preferred_inside_when_top_space_is_clear() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 200.0));
	let capture_rect = Rect::from_min_size(Pos2::new(40.0, 20.0), Vec2::new(200.0, 150.0));
	let reserved_rect = Rect::from_min_size(Pos2::new(80.0, 28.0), Vec2::new(120.0, 40.0));
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

	assert_eq!(badge_rect.max.y, capture_rect.max.y - SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX);
	assert!(!badge_rect.intersects(reserved_rect));
}

#[test]
fn selection_size_badge_reserved_rect_falls_above_capture_when_inside_space_is_exhausted() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 220.0));
	let capture_rect = Rect::from_min_size(Pos2::new(40.0, 170.0), Vec2::new(120.0, 50.0));
	let reserved_rect = Rect::from_min_size(Pos2::new(40.0, 178.0), Vec2::new(120.0, 40.0));
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

	assert_eq!(badge_rect.max.x, capture_rect.max.x);
	assert_eq!(badge_rect.max.y, capture_rect.min.y - SELECTION_SIZE_BADGE_GAP_PX);
	assert!(!badge_rect.intersects(reserved_rect));
}

#[test]
fn selection_size_badge_reserved_rect_uses_above_slot_at_top_edge_when_visible() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 112.0));
	let capture_rect = Rect::from_min_size(Pos2::new(40.0, 34.0), Vec2::new(120.0, 50.0));
	let reserved_rect = Rect::from_min_size(Pos2::new(40.0, 42.0), Vec2::new(120.0, 40.0));
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

	assert_eq!(badge_rect.min.y, screen_rect.min.y);
	assert_eq!(badge_rect.max.y, capture_rect.min.y - SELECTION_SIZE_BADGE_GAP_PX);
	assert!(!badge_rect.intersects(reserved_rect));
}

#[test]
fn selection_size_badge_reserved_rect_accepts_overlap_when_no_non_overlapping_slot_exists() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 52.0));
	let capture_rect = Rect::from_min_size(Pos2::new(40.0, 20.0), Vec2::new(120.0, 32.0));
	let reserved_rect = Rect::from_min_size(Pos2::new(40.0, 22.0), Vec2::new(120.0, 24.0));
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

	assert_eq!(badge_rect.max.x, capture_rect.max.x);
	assert_eq!(badge_rect.min.y, capture_rect.min.y);
	assert!(badge_rect.intersects(reserved_rect));
}

#[test]
fn selection_size_badge_text_uses_monitor_pixel_dimensions() {
	let monitor = tests::test_monitor_with_scale(1_000, 800, 2_000);

	assert_eq!(
		WindowRenderer::selection_size_badge_text(monitor, RectPoints::new(10, 20, 120, 80)),
		"240x160"
	);
}

#[test]
fn selection_size_badge_layout_keeps_visual_bounds_inside_badge_rect() {
	let ctx = tests::test_egui_context();
	let layout = WindowRenderer::selection_size_badge_layout(&ctx, "240x160", HudTheme::Light, 1.0);
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));

	for (label, capture_rect) in [
		("right edge rect", Rect::from_min_size(Pos2::new(760.0, 160.0), Vec2::new(40.0, 120.0))),
		(
			"bottom fallback rect",
			Rect::from_min_size(Pos2::new(120.0, 588.0), Vec2::new(140.0, 12.0)),
		),
	] {
		let badge_rect =
			WindowRenderer::selection_size_badge_rect(screen_rect, capture_rect, layout.badge_size);
		let text_anchor = WindowRenderer::selection_size_badge_text_anchor(badge_rect, layout, 1.0);
		let visual_bounds =
			WindowRenderer::selection_size_badge_visual_bounds(text_anchor, layout.text_size, 1.0);

		if label == "right edge rect" {
			assert_eq!(badge_rect.max.x, capture_rect.max.x, "{label}");
		}
		if label == "bottom fallback rect" {
			assert_eq!(badge_rect.max.y, screen_rect.max.y, "{label}");
		}

		assert!(visual_bounds.min.x >= badge_rect.min.x, "{label}");
		assert!(visual_bounds.max.x <= badge_rect.max.x, "{label}");
		assert!(visual_bounds.min.y >= badge_rect.min.y, "{label}");
		assert!(visual_bounds.max.y <= badge_rect.max.y, "{label}");
	}
}

#[test]
fn live_capture_size_badge_target_prefers_drag_then_hover_then_fullscreen() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Live;
	state.cursor = Some(GlobalPoint::new(320, 260));
	state.hovered_window_rect = Some(MonitorRectPoints {
		monitor_id: monitor.id,
		rect: RectPoints::new(120, 140, 300, 220),
	});

	assert_eq!(
		WindowRenderer::live_capture_size_badge_target(&state, monitor, screen_rect, true),
		Some(SelectionSizeBadgeTarget {
			rect: Rect::from_min_size(Pos2::new(120.0, 140.0), Vec2::new(300.0, 220.0)),
			size_points: RectPoints::new(120, 140, 300, 220),
		})
	);

	state.drag_rect = Some(MonitorRectPoints {
		monitor_id: monitor.id,
		rect: RectPoints::new(180, 200, 260, 180),
	});

	assert_eq!(
		WindowRenderer::live_capture_size_badge_target(&state, monitor, screen_rect, true),
		Some(SelectionSizeBadgeTarget {
			rect: Rect::from_min_size(Pos2::new(180.0, 200.0), Vec2::new(260.0, 180.0)),
			size_points: RectPoints::new(180, 200, 260, 180),
		})
	);

	state.drag_rect = None;
	state.hovered_window_rect = None;

	assert_eq!(
		WindowRenderer::live_capture_size_badge_target(&state, monitor, screen_rect, true),
		Some(SelectionSizeBadgeTarget {
			rect: screen_rect,
			size_points: RectPoints::new(0, 0, monitor.width, monitor.height),
		})
	);
}

#[test]
fn live_capture_size_badge_target_skips_fullscreen_fallback_while_primary_down() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Live;
	state.cursor = Some(GlobalPoint::new(320, 260));

	assert_eq!(
		WindowRenderer::live_capture_size_badge_target(&state, monitor, screen_rect, false),
		None
	);
}

#[test]
fn frozen_capture_size_badge_target_uses_frozen_rect_even_when_tiny() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1_000.0, 800.0));

	for frozen_rect in [RectPoints::new(140, 180, 320, 240), RectPoints::new(140, 180, 2, 1)] {
		let mut state = OverlayState::new();

		state.mode = OverlayMode::Frozen;
		state.frozen_capture_rect = Some(frozen_rect);

		assert_eq!(
			WindowRenderer::frozen_capture_size_badge_target(&state, screen_rect),
			Some(SelectionSizeBadgeTarget {
				rect: Rect::from_min_size(
					Pos2::new(frozen_rect.x as f32, frozen_rect.y as f32),
					Vec2::new(frozen_rect.width as f32, frozen_rect.height as f32)
				),
				size_points: frozen_rect,
			})
		);
	}
}

#[test]
fn render_frozen_capture_affordance_keeps_tiny_frozen_badge_path() {
	let ctx = tests::test_egui_context();
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(RectPoints::new(140, 180, 2, 1));

	assert!(WindowRenderer::render_frozen_capture_affordance(
		&ctx,
		&state,
		monitor,
		screen_rect,
		HudTheme::Dark,
		false,
		FrozenCaptureSource::None,
		None,
		&[],
		None,
		&[],
		None,
		&[],
		None,
		&[],
		None,
		FrozenToolbarState::default().text_style,
		false,
		true,
		1.0,
		&mut selection_flow_geometry_cache,
		&mut selection_dashed_border_cache,
	));
	assert!(selection_flow_geometry_cache.is_empty());
}

#[test]
fn render_live_capture_affordances_updates_flow_or_dash_for_target() {
	#[derive(Clone, Copy)]
	enum TargetKind {
		Hover,
		Drag,
		Fullscreen,
	}

	let ctx = tests::test_egui_context();
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));

	for (label, target_kind, flow_enabled, theme, expected_flow, expected_dash) in [
		("hover scrim with flow disabled", TargetKind::Hover, false, HudTheme::Light, false, false),
		("hover flow when enabled", TargetKind::Hover, true, HudTheme::Light, true, false),
		("drag border when flow disabled", TargetKind::Drag, false, HudTheme::Light, false, true),
		("idle fullscreen skips flow", TargetKind::Fullscreen, true, HudTheme::Dark, false, false),
	] {
		let layer = LayerId::new(Order::Foreground, Id::new(label));
		let painter = ctx.layer_painter(layer);
		let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();
		let mut state = OverlayState::new();
		let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();

		state.mode = OverlayMode::Live;

		match target_kind {
			TargetKind::Hover => {
				state.hovered_window_rect = Some(MonitorRectPoints {
					monitor_id: monitor.id,
					rect: RectPoints::new(100, 120, 240, 320),
				});
			},
			TargetKind::Drag => {
				state.drag_rect = Some(MonitorRectPoints {
					monitor_id: monitor.id,
					rect: RectPoints::new(100, 120, 240, 320),
				});
			},
			TargetKind::Fullscreen => {
				state.cursor = Some(GlobalPoint::new(240, 260));
			},
		}

		assert!(WindowRenderer::render_live_capture_affordances(
			&ctx,
			&painter,
			&state,
			monitor,
			screen_rect,
			theme,
			flow_enabled,
			1.0,
			&mut selection_flow_geometry_cache,
			&mut selection_dashed_border_cache,
		));
		assert_eq!(!selection_flow_geometry_cache.is_empty(), expected_flow, "{label}");
		assert_eq!(selection_dashed_border_cache.key.is_some(), expected_dash, "{label}");
	}
}

#[test]
fn selection_flow_light_palette_uses_lower_luminance_colors() {
	let dark_palette = WindowRenderer::selection_flow_palette(HudTheme::Dark);
	let light_palette = WindowRenderer::selection_flow_palette(HudTheme::Light);

	assert_ne!(dark_palette, light_palette);

	for &(red, green, blue) in light_palette {
		let channel_sum = u16::from(red) + u16::from(green) + u16::from(blue);

		assert!(
			channel_sum < 430,
			"light theme flow colors should stay visible on light backgrounds"
		);
	}
}

#[test]
fn live_capture_size_badge_target_keeps_tiny_drag_rect() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Live;
	state.drag_rect =
		Some(MonitorRectPoints { monitor_id: monitor.id, rect: RectPoints::new(180, 200, 2, 1) });

	assert_eq!(
		WindowRenderer::live_capture_size_badge_target(&state, monitor, screen_rect, false),
		Some(SelectionSizeBadgeTarget {
			rect: Rect::from_min_size(Pos2::new(180.0, 200.0), Vec2::new(2.0, 1.0)),
			size_points: RectPoints::new(180, 200, 2, 1),
		})
	);
}
