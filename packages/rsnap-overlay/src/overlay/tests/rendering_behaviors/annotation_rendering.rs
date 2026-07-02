use std::slice;

use crate::overlay::tests::rendering_behaviors::{
	self, FROZEN_TEXT_CARET_BLINK_PERIOD_SECS, FROZEN_TEXT_FONT_SIZE_POINTS, FontId,
	FrozenAnnotationColor, FrozenCaptureSource, FrozenEditKind, FrozenTextAnnotation,
	FrozenTextEditState, FrozenToolbarTool, GlobalPoint, HudTheme, Id, LayerId, MonitorRect, Order,
	OverlayMode, OverlaySession, OverlayState, Pos2, RawInput, Rect, RectPoints,
	SelectionDashedBorderCache, SelectionFlowGeometryCache, Ui, Vec2, WindowRenderer, tests,
};
#[cfg(target_os = "macos")]
use crate::overlay::tests::rendering_behaviors::{CursorIcon, overlay::macos_cursor_runtime};

#[test]
fn frozen_mosaic_drag_waits_for_final_capture_ready() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let original = rendering_behaviors::test_mosaic_source_image();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, original.clone());

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;

	assert!(!session.frozen_final_capture_ready());
	assert!(!session.begin_frozen_mosaic_drag(GlobalPoint::new(1, 1)));
	assert!(!session.commit_frozen_mosaic_drag());
	assert!(!session.perform_frozen_undo());
	assert!(!session.perform_frozen_redo());
	assert_eq!(session.state.frozen_mosaic_preview_rect, None);
	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&original));

	tests::promote_session_export_authority_ready(&mut session);

	assert!(session.begin_frozen_mosaic_drag(GlobalPoint::new(1, 1)));
}

#[test]
fn frozen_mosaic_drag_updates_preview_rect_inside_capture_bounds() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(
		&mut session,
		monitor,
		rendering_behaviors::test_mosaic_source_image(),
	);

	session.state.frozen_capture_rect = Some(RectPoints::new(2, 2, 4, 4));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;

	tests::promote_session_export_authority_ready(&mut session);

	assert!(session.begin_frozen_mosaic_drag(GlobalPoint::new(3, 3)));
	assert!(session.update_frozen_mosaic_drag_rect(GlobalPoint::new(30, 30)));
	assert_eq!(session.state.frozen_mosaic_preview_rect, Some(RectPoints::new(3, 3, 3, 3)));
}

#[test]
fn frozen_mosaic_commit_round_trips_through_undo_and_redo() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let original = rendering_behaviors::test_mosaic_source_image();
	let expected_fill = rendering_behaviors::average_patch_color(&original, 1, 1, 4, 4);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, original.clone());

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;

	tests::promote_session_export_authority_ready(&mut session);

	assert!(session.begin_frozen_mosaic_drag(GlobalPoint::new(1, 1)));
	assert!(session.update_frozen_mosaic_drag_rect(GlobalPoint::new(4, 4)));
	assert!(session.commit_frozen_mosaic_drag());

	let edited = session
		.state
		.frozen_display_image
		.clone()
		.expect("mosaic commit should retain the frozen display image");

	assert_eq!(edited.get_pixel(2, 2), &expected_fill);
	assert_eq!(edited.get_pixel(4, 4), &expected_fill);
	assert_ne!(edited, original);
	assert_eq!(session.state.frozen_mosaic_preview_rect, None);
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
	assert!(session.perform_frozen_undo());
	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&original));
	assert!(!session.toolbar_state.undo_available);
	assert!(session.toolbar_state.redo_available);
	assert!(session.perform_frozen_redo());
	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&edited));
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
}

#[test]
fn frozen_arrow_drag_commits_without_final_capture_and_round_trips_undo_redo() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(
		&mut session,
		monitor,
		rendering_behaviors::test_mosaic_source_image(),
	);

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Arrow;

	assert!(!session.frozen_final_capture_ready());
	assert!(session.begin_frozen_arrow_drag(GlobalPoint::new(1, 1)));
	assert_eq!(
		session.active_frozen_arrow_preview().map(|annotation| (annotation.start, annotation.end)),
		Some((Pos2::new(1.0, 1.0), Pos2::new(1.0, 1.0)))
	);
	assert!(session.update_frozen_arrow_drag(GlobalPoint::new(7, 1)));
	assert_eq!(
		session.active_frozen_arrow_preview().map(|annotation| (annotation.start, annotation.end)),
		Some((Pos2::new(1.0, 1.0), Pos2::new(7.0, 1.0)))
	);
	assert!(session.commit_frozen_arrow_drag());
	assert_eq!(session.frozen_arrow_annotations.len(), 1);
	assert_eq!(session.frozen_arrow_annotations[0].start, Pos2::new(1.0, 1.0));
	assert_eq!(session.frozen_arrow_annotations[0].end, Pos2::new(7.0, 1.0));
	assert_eq!(session.frozen_edit_undo_stack.last(), Some(&FrozenEditKind::ArrowAnnotation));
	assert!(session.active_frozen_arrow_preview().is_none());
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
	assert!(session.perform_frozen_undo());
	assert!(session.frozen_arrow_annotations.is_empty());
	assert!(!session.toolbar_state.undo_available);
	assert!(session.toolbar_state.redo_available);
	assert!(session.perform_frozen_redo());
	assert_eq!(session.frozen_arrow_annotations.len(), 1);
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
}

#[test]
fn frozen_spotlight_drag_clamps_preview_and_round_trips_undo_redo() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(
		&mut session,
		monitor,
		rendering_behaviors::test_mosaic_source_image(),
	);

	session.state.frozen_capture_rect = Some(RectPoints::new(2, 2, 4, 4));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Spotlight;

	assert!(!session.frozen_final_capture_ready());
	assert!(session.begin_frozen_spotlight_drag(GlobalPoint::new(3, 3)));
	assert_eq!(session.frozen_spotlight_preview_rect, Some(RectPoints::new(3, 3, 1, 1)));
	assert!(session.update_frozen_spotlight_drag_rect(GlobalPoint::new(30, 30)));
	assert_eq!(session.frozen_spotlight_preview_rect, Some(RectPoints::new(3, 3, 3, 3)));
	assert!(session.commit_frozen_spotlight_drag());
	assert_eq!(session.frozen_spotlight_annotations.len(), 1);
	assert_eq!(session.frozen_spotlight_annotations[0].rect, RectPoints::new(3, 3, 3, 3));
	assert_eq!(session.frozen_edit_undo_stack.last(), Some(&FrozenEditKind::SpotlightAnnotation));
	assert_eq!(session.frozen_spotlight_preview_rect, None);
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
	assert!(session.perform_frozen_undo());
	assert!(session.frozen_spotlight_annotations.is_empty());
	assert!(!session.toolbar_state.undo_available);
	assert!(session.toolbar_state.redo_available);
	assert!(session.perform_frozen_redo());
	assert_eq!(session.frozen_spotlight_annotations.len(), 1);
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
}

#[test]
fn frozen_text_edit_caret_rect_starts_at_anchor_when_text_is_empty() {
	let ctx = tests::test_egui_context();
	let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("text-caret-empty")));
	let anchor = Pos2::new(140.0, 160.0);
	let font_id = FontId::proportional(FROZEN_TEXT_FONT_SIZE_POINTS);
	let caret_rect = WindowRenderer::frozen_text_edit_caret_rect(&painter, anchor, "", &font_id);

	assert!((caret_rect.min.x - anchor.x).abs() <= f32::EPSILON);
	assert!((caret_rect.min.y - anchor.y).abs() <= f32::EPSILON);
	assert!(caret_rect.height() >= FROZEN_TEXT_FONT_SIZE_POINTS);
}

#[test]
fn frozen_text_edit_caret_rect_tracks_multiline_text_end() {
	let ctx = tests::test_egui_context();
	let painter =
		ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("text-caret-multiline")));
	let anchor = Pos2::new(140.0, 160.0);
	let font_id = FontId::proportional(FROZEN_TEXT_FONT_SIZE_POINTS);
	let caret_rect =
		WindowRenderer::frozen_text_edit_caret_rect(&painter, anchor, "A\nB", &font_id);

	assert!(caret_rect.min.y > anchor.y);
	assert!(caret_rect.min.x > anchor.x);
}

#[test]
fn frozen_text_edit_caret_rect_tracks_explicit_preedit_cursor_position() {
	let ctx = tests::test_egui_context();
	let painter =
		ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("text-caret-preedit-cursor")));
	let anchor = Pos2::new(140.0, 160.0);
	let font_id = FontId::proportional(FROZEN_TEXT_FONT_SIZE_POINTS);
	let caret_rect = WindowRenderer::frozen_text_edit_caret_rect_at_char_index(
		&painter, anchor, "ABCD", &font_id, 2,
	);
	let end_rect = WindowRenderer::frozen_text_edit_caret_rect(&painter, anchor, "ABCD", &font_id);

	assert!(caret_rect.min.x > anchor.x);
	assert!(caret_rect.min.x < end_rect.min.x);
	assert!((caret_rect.min.y - anchor.y).abs() <= f32::EPSILON);
}

#[test]
fn frozen_text_placeholder_fill_tracks_selected_text_color() {
	let blue =
		WindowRenderer::frozen_text_placeholder_fill(FrozenAnnotationColor::Blue, HudTheme::Dark);
	let red =
		WindowRenderer::frozen_text_placeholder_fill(FrozenAnnotationColor::Red, HudTheme::Dark);

	assert!(blue.b() > blue.r());
	assert!(red.r() > red.b());
	assert!(blue.a() < 255);
	assert!(red.a() < 255);
}

#[test]
fn frozen_text_edit_interaction_rect_uses_placeholder_bounds_when_empty() {
	let anchor = Pos2::new(140.0, 160.0);
	let font_id = FontId::proportional(FROZEN_TEXT_FONT_SIZE_POINTS);
	let rect = WindowRenderer::frozen_text_edit_interaction_rect(anchor, "", &font_id);

	assert!(rect.contains(anchor));
	assert!(rect.width() > FROZEN_TEXT_FONT_SIZE_POINTS);
	assert!(rect.height() >= FROZEN_TEXT_FONT_SIZE_POINTS);
}

#[test]
fn frozen_text_edit_interaction_rect_covers_full_width_text_layout() {
	let ctx = tests::test_egui_context();
	let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("text-hitbox-cjk")));
	let anchor = Pos2::new(140.0, 160.0);
	let font_id = FontId::proportional(FROZEN_TEXT_FONT_SIZE_POINTS);
	let rect = WindowRenderer::frozen_text_edit_interaction_rect(anchor, "你好世界", &font_id);
	let caret_rect =
		WindowRenderer::frozen_text_edit_caret_rect(&painter, anchor, "你好世界", &font_id);

	assert!(rect.contains(caret_rect.min));
	assert!(rect.contains(Pos2::new(caret_rect.max.x, caret_rect.min.y)));
}

#[test]
fn frozen_committed_text_annotations_are_clipped_to_capture_rect() {
	let ctx = tests::test_egui_context();
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 120.0));
	let capture_rect_points = RectPoints::new(40, 20, 80, 40);
	let capture_rect = Rect::from_min_size(
		Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
		Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
	);
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: screen_rect.width() as u32,
		height: screen_rect.height() as u32,
		scale_factor_x1000: 1_000,
	};
	let style = OverlaySession::new().toolbar_state.text_style;
	let annotation = FrozenTextAnnotation {
		anchor: Pos2::new(capture_rect.max.x - 2.0, capture_rect.min.y + 4.0),
		text: String::from("edge"),
		style,
	};
	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(capture_rect_points);

	let empty_output = ctx.run_ui(
		RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|_ui: &mut Ui| {
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
				style,
				false,
				true,
				1.0,
				&mut selection_flow_geometry_cache,
				&mut selection_dashed_border_cache,
			));
		},
	);
	let clipped_shape_count_without_text =
		empty_output.shapes.iter().filter(|shape| shape.clip_rect == capture_rect).count();
	let full_output = ctx.run_ui(
		RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|_ui: &mut Ui| {
			assert!(WindowRenderer::render_frozen_capture_affordance(
				&ctx,
				&state,
				monitor,
				screen_rect,
				HudTheme::Dark,
				false,
				FrozenCaptureSource::None,
				None,
				&[FrozenEditKind::TextAnnotation],
				None,
				&[],
				None,
				&[],
				None,
				slice::from_ref(&annotation),
				None,
				style,
				false,
				true,
				1.0,
				&mut selection_flow_geometry_cache,
				&mut selection_dashed_border_cache,
			));
		},
	);
	let clipped_shape_count_with_text =
		full_output.shapes.iter().filter(|shape| shape.clip_rect == capture_rect).count();

	assert!(
		clipped_shape_count_with_text > clipped_shape_count_without_text,
		"committed text should add shapes clipped to the frozen capture rect",
	);
}

#[test]
fn frozen_active_text_preview_is_clipped_to_capture_rect() {
	let ctx = tests::test_egui_context();
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 120.0));
	let capture_rect_points = RectPoints::new(40, 20, 80, 40);
	let capture_rect = Rect::from_min_size(
		Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
		Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
	);
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: screen_rect.width() as u32,
		height: screen_rect.height() as u32,
		scale_factor_x1000: 1_000,
	};
	let style = OverlaySession::new().toolbar_state.text_style;
	let mut text_edit =
		FrozenTextEditState::new(Pos2::new(capture_rect.max.x - 2.0, capture_rect.min.y + 4.0));

	text_edit.text = String::from("editing");

	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(capture_rect_points);

	let empty_output = ctx.run_ui(
		RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|_ui: &mut Ui| {
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
				style,
				false,
				true,
				1.0,
				&mut selection_flow_geometry_cache,
				&mut selection_dashed_border_cache,
			));
		},
	);
	let clipped_shape_count_without_preview =
		empty_output.shapes.iter().filter(|shape| shape.clip_rect == capture_rect).count();
	let preview_output = ctx.run_ui(
		RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|_ui: &mut Ui| {
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
				Some(&text_edit),
				style,
				false,
				true,
				1.0,
				&mut selection_flow_geometry_cache,
				&mut selection_dashed_border_cache,
			));
		},
	);
	let clipped_shape_count_with_preview =
		preview_output.shapes.iter().filter(|shape| shape.clip_rect == capture_rect).count();

	assert!(
		clipped_shape_count_with_preview > clipped_shape_count_without_preview,
		"active text preview should add shapes clipped to the frozen capture rect",
	);
}

#[test]
fn frozen_text_caret_visible_blinks_on_half_periods() {
	assert!(WindowRenderer::frozen_text_caret_visible(0.0));
	assert!(WindowRenderer::frozen_text_caret_visible(FROZEN_TEXT_CARET_BLINK_PERIOD_SECS * 0.49,));
	assert!(
		!WindowRenderer::frozen_text_caret_visible(FROZEN_TEXT_CARET_BLINK_PERIOD_SECS * 0.51,)
	);
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_mosaic_cursor_rects_preserve_crosshair_hover_and_drag() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());
	tests::promote_session_export_authority_ready(&mut session);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;

	let rects = session.frozen_selection_cursor_rects_for_monitor(monitor);

	assert_eq!(
		macos_cursor_runtime::overlay_cursor_rect_icon_at_point(&rects, Pos2::new(150.0, 180.0)),
		Some(CursorIcon::Crosshair)
	);
	assert_eq!(
		macos_cursor_runtime::overlay_cursor_rect_icon_at_point(&rects, Pos2::new(80.0, 100.0)),
		None
	);

	session.frozen_mosaic_drag.active = true;

	let rects = session.frozen_selection_cursor_rects_for_monitor(monitor);

	assert_eq!(rects.len(), 1);
	assert_eq!(rects[0].icon, CursorIcon::Crosshair);
	assert_eq!(rects[0].rect.min, Pos2::ZERO);
	assert_eq!(rects[0].rect.max, Pos2::new(monitor.width as f32, monitor.height as f32));
}
