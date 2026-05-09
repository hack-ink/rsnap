use image::RgbaImage;
#[cfg(not(target_os = "macos"))]
use winit::event::{DeviceId, TouchPhase, WindowEvent};
#[cfg(not(target_os = "macos"))]
use winit::window::WindowId;

use crate::overlay::tests::{
	self, ActiveFrozenBrushStroke, Duration, ElementState, FROZEN_EDIT_HISTORY_LIMIT,
	FROZEN_TEXT_CARET_REPAINT_INTERVAL, FrozenAnnotationColor, FrozenBrushModelState,
	FrozenBrushStroke, FrozenBrushStyle, FrozenCommittedOverlay, FrozenEditKind,
	FrozenExportTransform, FrozenTextAnnotation, FrozenTextEditState, FrozenTextInputSource,
	FrozenToolbarTool, GlobalPoint, Ime, Instant, Key, MonitorRect, MouseScrollDelta, NamedKey,
	OverlaySession, PhysicalPosition, Pos2, RectPoints, Rgba, Vec2,
	overlay::{
		self, FROZEN_BRUSH_MODEL_SYNTHETIC_SAMPLE_INTERVAL_SECONDS,
		FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS,
	},
};

#[test]
fn frozen_brush_undo_and_redo_update_export_image() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session
		.state
		.commit_frozen_final_image(monitor, RgbaImage::from_pixel(8, 8, Rgba([12, 34, 56, 255])));

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));

	tests::promote_session_export_authority_ready(&mut session);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(3, 3)));
	assert!(session.finish_frozen_brush_stroke());
	assert!(session.perform_frozen_undo());

	let undone = session.current_export_image().expect("undo export image");

	assert_eq!(undone.get_pixel(3, 3), &Rgba([12, 34, 56, 255]));
	assert!(session.perform_frozen_redo());

	let redone = session.current_export_image().expect("redo export image");

	assert_eq!(
		redone.get_pixel(3, 3),
		&Rgba(session.toolbar_state.brush_style.color.export_rgba())
	);
}

#[test]
fn rasterizing_frozen_brush_clears_reused_coverage_mask() {
	let export_transform =
		FrozenExportTransform::new(RectPoints::new(0, 0, 8, 8), 8, 8).expect("export transform");
	let mut export_image = RgbaImage::from_pixel(8, 8, Rgba([12, 34, 56, 255]));
	let mut coverage_mask = vec![255_u8; 8 * 8];

	OverlaySession::rasterize_frozen_brush_points_into_image(
		&mut export_image,
		&mut coverage_mask,
		export_transform,
		&[Pos2::new(2.0, 2.0)],
		FrozenBrushStyle::default(),
	);

	assert_eq!(export_image.get_pixel(7, 7), &Rgba([12, 34, 56, 255]));
	assert_eq!(
		export_image.get_pixel(2, 2),
		&Rgba(FrozenBrushStyle::default().color.export_rgba())
	);
}

#[test]
fn rendered_frozen_brush_points_round_corners_into_a_curve() {
	let points = [Pos2::new(1.0, 1.0), Pos2::new(1.0, 5.0), Pos2::new(5.0, 5.0)];
	let rendered = OverlaySession::rendered_frozen_brush_points(
		&points,
		FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS,
	);

	assert_eq!(rendered.first().copied(), Some(points[0]));
	assert_eq!(rendered.last().copied(), Some(points[2]));
	assert!(rendered.len() > points.len());
	assert!(rendered.iter().any(|point| {
		(point.x - points[0].x).abs() > f32::EPSILON && (point.y - points[2].y).abs() > f32::EPSILON
	}));
}

#[test]
fn corrected_frozen_brush_points_preserve_open_stroke_endpoints() {
	let points = [Pos2::new(1.0, 1.0), Pos2::new(4.0, 2.0), Pos2::new(7.0, 6.0)];
	let corrected = OverlaySession::corrected_frozen_brush_points(&points);

	assert_eq!(corrected.first().copied(), Some(points[0]));
	assert_eq!(corrected.last().copied(), Some(points[2]));
	assert!(corrected.len() >= 2);
}

#[test]
fn corrected_frozen_brush_points_keep_annotation_loops_open() {
	let points = [
		Pos2::new(2.0, 0.0),
		Pos2::new(6.0, 1.0),
		Pos2::new(8.0, 4.0),
		Pos2::new(7.0, 8.0),
		Pos2::new(3.0, 9.0),
		Pos2::new(0.0, 6.0),
		Pos2::new(1.5, 1.0),
	];
	let corrected = OverlaySession::corrected_frozen_brush_points(&points);

	assert!(corrected.len() >= 2);
	assert_eq!(corrected.first().copied(), Some(points[0]));
	assert_eq!(corrected.last().copied(), Some(points[6]));
	assert_ne!(corrected.first().copied(), corrected.last().copied());
}

#[test]
fn corrected_frozen_brush_points_suppress_small_local_dent() {
	let points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(5.0, 0.4),
		Pos2::new(8.0, -1.6),
		Pos2::new(11.0, 0.6),
		Pos2::new(16.0, 0.3),
	];
	let corrected = OverlaySession::corrected_frozen_brush_points(&points);
	let deepest_y = corrected.iter().fold(f32::INFINITY, |deepest, point| deepest.min(point.y));

	assert_eq!(corrected.first().copied(), Some(points[0]));
	assert_eq!(corrected.last().copied(), Some(points[4]));
	assert!(deepest_y > -0.8, "expected final stroke to smooth away the local dent: {corrected:?}");
}

#[test]
fn corrected_frozen_brush_points_preserve_monotonic_arc_trend() {
	let points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(3.0, 1.8),
		Pos2::new(6.0, 3.8),
		Pos2::new(9.0, 5.1),
		Pos2::new(11.0, 4.6),
		Pos2::new(14.0, 6.4),
		Pos2::new(18.0, 8.8),
	];
	let corrected = OverlaySession::corrected_frozen_brush_points(&points);

	assert_eq!(corrected.first().copied(), Some(points[0]));
	assert_eq!(corrected.last().copied(), Some(points[6]));
	assert!(corrected.windows(2).all(|pair| pair[1].y + 0.05 >= pair[0].y));
}

#[test]
fn corrected_frozen_brush_points_preserve_small_wave_backbone() {
	let points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(4.0, 1.8),
		Pos2::new(8.0, -1.7),
		Pos2::new(12.0, 1.9),
		Pos2::new(16.0, -1.8),
		Pos2::new(20.0, 1.7),
		Pos2::new(24.0, 0.0),
	];
	let corrected = OverlaySession::corrected_frozen_brush_points(&points);
	let reversals = tests::significant_y_direction_reversals(&corrected, 0.12);
	let (min_y, max_y) = corrected.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |acc, point| {
		(acc.0.min(point.y), acc.1.max(point.y))
	});

	assert_eq!(corrected.first().copied(), Some(points[0]));
	assert_eq!(corrected.last().copied(), Some(points[6]));
	assert!(
		reversals >= 3,
		"expected the corrected stroke to keep the wave skeleton: {corrected:?}"
	);
	assert!(
		max_y - min_y >= 1.0,
		"expected the corrected stroke to retain visible wave amplitude: {corrected:?}"
	);
}

#[test]
fn preview_frozen_brush_points_keep_live_modeled_path_before_commit() {
	let active_stroke = ActiveFrozenBrushStroke {
		raw_points: vec![
			Pos2::new(0.0, 0.0),
			Pos2::new(4.0, 6.0),
			Pos2::new(8.0, -2.0),
			Pos2::new(12.0, 4.0),
		],
		points: vec![Pos2::new(0.0, 0.0), Pos2::new(6.0, 2.0), Pos2::new(12.0, 4.0)],
		style: FrozenBrushStyle::default(),
		model_state: FrozenBrushModelState {
			filtered_input_point: Pos2::new(12.0, 4.0),
			modeled_point: Pos2::new(12.0, 4.0),
			modeled_velocity: Vec2::ZERO,
			modeled_elapsed_seconds: 0.03,
		},
		started_at: Instant::now(),
		last_sample_at: Instant::now(),
	};
	let preview = OverlaySession::preview_frozen_brush_points(&active_stroke);
	let committed = OverlaySession::corrected_frozen_brush_points(&active_stroke.raw_points);

	assert_eq!(preview.first().copied(), active_stroke.raw_points.first().copied());
	assert_eq!(preview.last().copied(), active_stroke.raw_points.last().copied());
	assert_ne!(preview, active_stroke.points);
	assert_ne!(preview, committed);
}

#[test]
fn preview_frozen_brush_points_follow_modeled_centerline_instead_of_raw_wobble() {
	let active_stroke = ActiveFrozenBrushStroke {
		raw_points: vec![
			Pos2::new(0.0, 0.0),
			Pos2::new(1.0, 0.55),
			Pos2::new(2.0, -0.50),
			Pos2::new(3.0, 0.48),
			Pos2::new(4.0, -0.42),
			Pos2::new(5.0, 0.36),
			Pos2::new(6.0, -0.28),
			Pos2::new(7.0, 0.18),
			Pos2::new(8.0, 0.0),
		],
		points: vec![Pos2::new(0.0, 0.0), Pos2::new(4.0, 0.04), Pos2::new(8.0, 0.0)],
		style: FrozenBrushStyle::default(),
		model_state: FrozenBrushModelState {
			filtered_input_point: Pos2::new(8.0, 0.0),
			modeled_point: Pos2::new(8.0, 0.0),
			modeled_velocity: Vec2::ZERO,
			modeled_elapsed_seconds: 0.05,
		},
		started_at: Instant::now(),
		last_sample_at: Instant::now(),
	};
	let preview = OverlaySession::preview_frozen_brush_points(&active_stroke);
	let (min_y, max_y) = preview.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |acc, point| {
		(acc.0.min(point.y), acc.1.max(point.y))
	});

	assert_eq!(preview.first().copied(), active_stroke.raw_points.first().copied());
	assert_eq!(preview.last().copied(), active_stroke.raw_points.last().copied());
	assert!(
		max_y - min_y <= 0.20,
		"expected preview to stay close to the modeled centerline instead of exposing raw wobble: {preview:?}"
	);
}

#[test]
fn rendered_live_frozen_brush_wave_preview_avoids_hard_inflection_kinks() {
	let raw_points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(4.0, 1.8),
		Pos2::new(8.0, -1.7),
		Pos2::new(12.0, 1.9),
		Pos2::new(16.0, -1.8),
		Pos2::new(20.0, 1.7),
		Pos2::new(24.0, 0.0),
	];
	let started_at = Instant::now();
	let mut stroke = OverlaySession::new_active_frozen_brush_stroke(
		raw_points[0],
		started_at,
		FrozenBrushStyle::default(),
	);

	for (index, point) in raw_points.iter().copied().enumerate().skip(1) {
		let sampled_at = started_at
			+ Duration::from_secs_f32(
				index as f32 * FROZEN_BRUSH_MODEL_SYNTHETIC_SAMPLE_INTERVAL_SECONDS,
			);

		OverlaySession::append_frozen_brush_raw_sample(&mut stroke, point, sampled_at);
	}

	let preview = OverlaySession::preview_frozen_brush_points(&stroke);
	let rendered = OverlaySession::rendered_frozen_brush_points(
		&preview,
		FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS,
	);
	let max_turn_angle = rendered.windows(3).fold(0.0_f32, |max_turn, window| {
		max_turn.max(OverlaySession::frozen_brush_turn_angle(window[0], window[1], window[2]))
	});

	assert!(
		tests::significant_y_direction_reversals(&rendered, 0.12) >= 2,
		"expected live preview to keep visible oscillation: {rendered:?}"
	);
	assert!(
		max_turn_angle <= 0.48,
		"expected live preview to round inflections instead of producing hard kinks: {rendered:?}"
	);
}

#[test]
fn rendered_live_frozen_brush_arc_preview_avoids_corner_snap() {
	let raw_points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(2.4, 0.2),
		Pos2::new(4.7, 1.1),
		Pos2::new(6.6, 2.8),
		Pos2::new(7.7, 5.1),
		Pos2::new(7.8, 7.8),
		Pos2::new(6.8, 10.2),
		Pos2::new(4.9, 12.0),
	];
	let started_at = Instant::now();
	let mut stroke = OverlaySession::new_active_frozen_brush_stroke(
		raw_points[0],
		started_at,
		FrozenBrushStyle::default(),
	);

	for (index, point) in raw_points.iter().copied().enumerate().skip(1) {
		let sampled_at = started_at
			+ Duration::from_secs_f32(
				index as f32 * FROZEN_BRUSH_MODEL_SYNTHETIC_SAMPLE_INTERVAL_SECONDS,
			);

		OverlaySession::append_frozen_brush_raw_sample(&mut stroke, point, sampled_at);
	}

	let preview = OverlaySession::preview_frozen_brush_points(&stroke);
	let rendered = OverlaySession::rendered_frozen_brush_points(
		&preview,
		FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS,
	);
	let max_turn_angle = rendered.windows(3).fold(0.0_f32, |max_turn, window| {
		max_turn.max(OverlaySession::frozen_brush_turn_angle(window[0], window[1], window[2]))
	});

	assert!(
		max_turn_angle <= 0.42,
		"expected sustained arc preview to stay rounded instead of preserving a corner: {rendered:?}"
	);
}

#[test]
fn rendered_live_frozen_brush_suppresses_slow_straight_wobble() {
	let raw_points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(0.45, 0.18),
		Pos2::new(0.9, -0.15),
		Pos2::new(1.35, 0.17),
		Pos2::new(1.8, -0.13),
		Pos2::new(2.25, 0.16),
		Pos2::new(2.7, -0.12),
		Pos2::new(3.15, 0.14),
		Pos2::new(3.6, -0.10),
		Pos2::new(4.05, 0.12),
		Pos2::new(4.5, -0.09),
		Pos2::new(4.95, 0.10),
		Pos2::new(5.4, -0.08),
		Pos2::new(5.85, 0.08),
		Pos2::new(6.3, -0.07),
		Pos2::new(6.75, 0.06),
		Pos2::new(7.2, -0.05),
		Pos2::new(7.6, 0.03),
		Pos2::new(8.0, 0.0),
	];
	let started_at = Instant::now();
	let mut stroke = OverlaySession::new_active_frozen_brush_stroke(
		raw_points[0],
		started_at,
		FrozenBrushStyle::default(),
	);

	for (index, point) in raw_points.iter().copied().enumerate().skip(1) {
		let sampled_at = started_at
			+ Duration::from_secs_f32(
				index as f32 * FROZEN_BRUSH_MODEL_SYNTHETIC_SAMPLE_INTERVAL_SECONDS,
			);

		OverlaySession::append_frozen_brush_raw_sample(&mut stroke, point, sampled_at);
	}

	let preview = OverlaySession::preview_frozen_brush_points(&stroke);
	let rendered = OverlaySession::rendered_frozen_brush_points(
		&preview,
		FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS,
	);
	let (min_y, max_y) = rendered.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |acc, point| {
		(acc.0.min(point.y), acc.1.max(point.y))
	});

	assert!(
		tests::significant_y_direction_reversals(&rendered, 0.03) <= 1,
		"expected slow straight preview to suppress visible wobble reversals: {rendered:?}"
	);
	assert!(
		max_y - min_y <= 0.26,
		"expected slow straight preview to stay close to a single line: {rendered:?}"
	);
}

#[test]
fn frozen_brush_model_response_follows_fast_strokes_more_closely() {
	let points = [Pos2::new(0.0, 0.0), Pos2::new(16.0, 0.0)];
	let slow = OverlaySession::frozen_brush_input_response(&points, 16.0 / 120.0);
	let fast = OverlaySession::frozen_brush_input_response(&points, 16.0 / 1_600.0);

	assert!(slow < fast);
	assert!(slow >= overlay::FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MIN);
	assert!(fast <= overlay::FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MAX);
}

#[test]
fn frozen_brush_model_response_boosts_sustained_curve_motion() {
	let straight_points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(2.0, 0.0),
		Pos2::new(4.0, 0.0),
		Pos2::new(6.0, 0.0),
		Pos2::new(8.0, 0.0),
	];
	let curved_points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(2.0, 0.5),
		Pos2::new(3.7, 1.6),
		Pos2::new(5.0, 3.2),
		Pos2::new(5.8, 5.1),
	];
	let straight = OverlaySession::frozen_brush_input_response(&straight_points, 2.0 / 120.0);
	let curved = OverlaySession::frozen_brush_input_response(&curved_points, 2.0 / 120.0);

	assert!(
		curved > straight,
		"expected sustained curved motion to get a higher live response than straight motion: straight={straight}, curved={curved}"
	);
}

#[test]
fn begin_frozen_text_edit_at_starts_text_input_inside_capture_rect() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert_eq!(
		session.frozen_text_edit.as_ref().map(|edit| edit.anchor),
		Some(Pos2::new(140.0, 160.0))
	);
	assert!(!session.frozen_selection_drag.active);
}

#[test]
fn begin_frozen_text_edit_at_ignores_non_authoritative_monitor() {
	let monitor = tests::test_monitor();
	let other_monitor = MonitorRect {
		id: 2,
		origin: GlobalPoint::new(1_000, 0),
		width: monitor.width,
		height: monitor.height,
		scale_factor_x1000: monitor.scale_factor_x1000,
	};
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(!session.begin_frozen_text_edit_at(other_monitor, GlobalPoint::new(1_140, 160)));
	assert!(session.frozen_text_edit.is_none());
}

#[test]
fn default_frozen_text_style_uses_16_point_font() {
	let session = OverlaySession::new();

	assert_eq!(
		session.toolbar_state.text_style.font_size_points,
		overlay::FROZEN_TEXT_FONT_SIZE_POINTS
	);
	assert_eq!(session.toolbar_state.text_style.font_size_points, 16.0);
	assert_eq!(session.toolbar_state.text_style.color, FrozenAnnotationColor::Blue);
}

#[test]
fn default_frozen_brush_style_uses_existing_width_and_color() {
	let session = OverlaySession::new();

	assert_eq!(
		session.toolbar_state.brush_style.stroke_width_points,
		overlay::FROZEN_BRUSH_STROKE_WIDTH_POINTS
	);
	assert_eq!(session.toolbar_state.brush_style.color, FrozenAnnotationColor::Blue);
}

#[test]
fn frozen_text_style_accepts_arbitrary_sizes_and_clamps_to_bounds() {
	let mut session = OverlaySession::new();

	assert!(session.toolbar_state.text_style.set_font_size(27.5));
	assert_eq!(session.toolbar_state.text_style.font_size_points, 27.5);
	assert!(session.toolbar_state.text_style.set_font_size(2.0));
	assert_eq!(
		session.toolbar_state.text_style.font_size_points,
		overlay::FROZEN_TEXT_FONT_SIZE_MIN_POINTS
	);
	assert!(session.toolbar_state.text_style.set_font_size(200.0));
	assert_eq!(
		session.toolbar_state.text_style.font_size_points,
		overlay::FROZEN_TEXT_FONT_SIZE_MAX_POINTS
	);
}

#[test]
fn frozen_brush_style_accepts_arbitrary_sizes_and_clamps_to_bounds() {
	let mut session = OverlaySession::new();

	assert!(session.toolbar_state.brush_style.set_stroke_width(4.25));
	assert_eq!(session.toolbar_state.brush_style.stroke_width_points, 4.25);
	assert!(session.toolbar_state.brush_style.set_stroke_width(0.1));
	assert_eq!(
		session.toolbar_state.brush_style.stroke_width_points,
		overlay::FROZEN_BRUSH_STROKE_WIDTH_MIN_POINTS
	);
	assert!(session.toolbar_state.brush_style.set_stroke_width(100.0));
	assert_eq!(
		session.toolbar_state.brush_style.stroke_width_points,
		overlay::FROZEN_BRUSH_STROKE_WIDTH_MAX_POINTS
	);
}

#[test]
fn toolbar_annotation_size_wheel_requires_hovered_size_control() {
	let mut session = OverlaySession::new();

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(
		!session
			.toolbar_state
			.apply_annotation_size_wheel_delta(&MouseScrollDelta::LineDelta(0.0, 1.0))
	);
	assert_eq!(session.toolbar_state.text_style.font_size_points, 16.0);
}

#[test]
fn toolbar_annotation_size_wheel_adjusts_pen_and_text_sizes() {
	let mut session = OverlaySession::new();

	session.toolbar_state.annotation_size_control_hovered = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(
		session
			.toolbar_state
			.apply_annotation_size_wheel_delta(&MouseScrollDelta::LineDelta(0.0, 2.0))
	);
	assert_eq!(session.toolbar_state.brush_style.stroke_width_points, 4.0);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(
		session
			.toolbar_state
			.apply_annotation_size_wheel_delta(&MouseScrollDelta::LineDelta(0.0, -2.0))
	);
	assert_eq!(session.toolbar_state.text_style.font_size_points, 14.0);
}

#[test]
fn toolbar_annotation_size_line_wheel_treats_nonzero_delta_as_immediate_step() {
	let mut session = OverlaySession::new();

	session.toolbar_state.annotation_size_control_hovered = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(
		session
			.toolbar_state
			.apply_annotation_size_wheel_delta(&MouseScrollDelta::LineDelta(0.0, 0.25))
	);
	assert_eq!(session.toolbar_state.text_style.font_size_points, 17.0);
	assert_eq!(session.toolbar_state.annotation_size_wheel_accumulator, 0.0);
}

#[test]
fn toolbar_annotation_size_wheel_accumulates_trackpad_pixel_deltas() {
	let mut session = OverlaySession::new();

	session.toolbar_state.annotation_size_control_hovered = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(!session.toolbar_state.apply_annotation_size_wheel_delta(
		&MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 15.0)),
	));
	assert_eq!(session.toolbar_state.text_style.font_size_points, 16.0);
	assert!(session.toolbar_state.apply_annotation_size_wheel_delta(
		&MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 17.0)),
	));
	assert_eq!(session.toolbar_state.text_style.font_size_points, 17.0);
}

#[test]
fn toolbar_annotation_size_wheel_snaps_fractional_text_sizes_toward_integers() {
	let mut session = OverlaySession::new();

	session.toolbar_state.annotation_size_control_hovered = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.toolbar_state.text_style.set_font_size(27.5));
	assert!(
		session
			.toolbar_state
			.apply_annotation_size_wheel_delta(&MouseScrollDelta::LineDelta(0.0, 1.0))
	);
	assert_eq!(session.toolbar_state.text_style.font_size_points, 28.0);
	assert!(session.toolbar_state.text_style.set_font_size(27.5));

	session.toolbar_state.annotation_size_wheel_accumulator = 0.0;

	assert!(
		session
			.toolbar_state
			.apply_annotation_size_wheel_delta(&MouseScrollDelta::LineDelta(0.0, -1.0))
	);
	assert_eq!(session.toolbar_state.text_style.font_size_points, 27.0);
}

#[test]
fn toolbar_annotation_size_wheel_uses_adaptive_pen_step_sizes() {
	let mut session = OverlaySession::new();

	session.toolbar_state.annotation_size_control_hovered = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert_eq!(session.toolbar_state.brush_style.stroke_width_points, 3.5);
	assert!(
		session
			.toolbar_state
			.apply_annotation_size_wheel_delta(&MouseScrollDelta::LineDelta(0.0, 1.0))
	);
	assert_eq!(session.toolbar_state.brush_style.stroke_width_points, 3.75);
	assert!(session.toolbar_state.brush_style.set_stroke_width(6.0));
	assert!(
		session
			.toolbar_state
			.apply_annotation_size_wheel_delta(&MouseScrollDelta::LineDelta(0.0, 1.0))
	);
	assert_eq!(session.toolbar_state.brush_style.stroke_width_points, 6.5);
	assert!(session.toolbar_state.brush_style.set_stroke_width(12.0));
	assert!(
		session
			.toolbar_state
			.apply_annotation_size_wheel_delta(&MouseScrollDelta::LineDelta(0.0, 1.0))
	);
	assert_eq!(session.toolbar_state.brush_style.stroke_width_points, 13.0);
}

#[test]
fn toolbar_annotation_size_steps_share_the_same_pen_and_text_logic() {
	let mut session = OverlaySession::new();

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.toolbar_state.apply_annotation_size_steps(1));
	assert_eq!(session.toolbar_state.brush_style.stroke_width_points, 3.75);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.toolbar_state.text_style.set_font_size(27.5));
	assert!(session.toolbar_state.apply_annotation_size_steps(-1));
	assert_eq!(session.toolbar_state.text_style.font_size_points, 27.0);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn overlay_window_mouse_wheel_routes_inline_toolbar_size_adjustments() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;
	session.toolbar_state.visible = true;
	session.toolbar_state.annotation_size_control_hovered = true;

	let event = WindowEvent::MouseWheel {
		device_id: DeviceId::dummy(),
		delta: MouseScrollDelta::LineDelta(0.0, 1.0),
		phase: TouchPhase::Moved,
	};
	let _ = session.handle_window_event(WindowId::dummy(), &event);

	assert_eq!(session.toolbar_state.text_style.font_size_points, 17.0);
}

#[test]
fn frozen_text_edit_drag_repositions_anchor_within_capture_rect() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.begin_frozen_text_edit_drag_at(monitor, GlobalPoint::new(141, 161)));
	assert!(session.update_frozen_text_edit_drag_anchor(GlobalPoint::new(200, 210)));
	assert_eq!(
		session.frozen_text_edit.as_ref().map(|edit| edit.anchor),
		Some(Pos2::new(199.0, 209.0))
	);
	assert!(session.stop_frozen_text_edit_drag());
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.dragging), Some(false));
}

#[test]
fn toolbar_mouse_release_stops_active_frozen_text_edit_drag() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.begin_frozen_text_edit_drag_at(monitor, GlobalPoint::new(141, 161)));

	session.toolbar_left_button_down = true;

	let _ = session.handle_toolbar_mouse_input(ElementState::Released);

	assert!(!session.toolbar_left_button_down);
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.dragging), Some(false));
}

#[test]
fn toolbar_mouse_release_commits_active_frozen_arrow_drag() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Arrow;

	assert!(session.begin_frozen_arrow_drag(GlobalPoint::new(140, 160)));
	assert!(session.update_frozen_arrow_drag(GlobalPoint::new(220, 200)));

	session.toolbar_left_button_down = true;

	let _ = session.handle_toolbar_mouse_input(ElementState::Released);

	assert!(!session.toolbar_left_button_down);
	assert!(!session.frozen_arrow_drag.active);
	assert_eq!(session.frozen_arrow_annotations.len(), 1);
	assert_eq!(session.frozen_edit_undo_stack.last(), Some(&FrozenEditKind::ArrowAnnotation));
}

#[test]
fn toolbar_mouse_release_commits_active_frozen_spotlight_drag() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Spotlight;

	assert!(session.begin_frozen_spotlight_drag(GlobalPoint::new(140, 160)));
	assert!(session.update_frozen_spotlight_drag_rect(GlobalPoint::new(220, 200)));

	session.toolbar_left_button_down = true;

	let _ = session.handle_toolbar_mouse_input(ElementState::Released);

	assert!(!session.toolbar_left_button_down);
	assert!(!session.frozen_spotlight_drag.active);
	assert_eq!(session.frozen_spotlight_preview_rect, None);
	assert_eq!(session.frozen_spotlight_annotations.len(), 1);
	assert_eq!(session.frozen_edit_undo_stack.last(), Some(&FrozenEditKind::SpotlightAnnotation));
}

#[test]
fn toolbar_mouse_release_finishes_active_frozen_brush_stroke() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(140, 160)));
	assert!(session.update_frozen_brush_stroke(GlobalPoint::new(220, 200)));

	session.toolbar_left_button_down = true;

	let _ = session.handle_toolbar_mouse_input(ElementState::Released);

	assert!(!session.toolbar_left_button_down);
	assert!(session.frozen_brush.active_stroke.is_none());
	assert_eq!(session.frozen_brush.committed_strokes.len(), 1);
	assert_eq!(session.frozen_edit_undo_stack.last(), Some(&FrozenEditKind::BrushStroke));
}

#[test]
fn adjacent_text_events_from_key_and_ime_are_deduplicated() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));

	let key_generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Key,
		key_generation,
		"A",
	));

	let ime_generation = session.note_frozen_text_input_event();

	assert!(!session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Ime,
		ime_generation,
		"A",
	));
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some("A"));

	let _ = session.finish_frozen_text_editing(false);

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(150, 165)));

	let ime_generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Ime,
		ime_generation,
		"B",
	));

	let key_generation = session.note_frozen_text_input_event();

	assert!(!session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Key,
		key_generation,
		"B",
	));
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some("B"));
}

#[test]
fn non_adjacent_identical_text_events_from_different_sources_are_not_deduplicated() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));

	let key_generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Key,
		key_generation,
		"A",
	));

	let _ = session.note_frozen_text_input_event();
	let ime_generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Ime,
		ime_generation,
		"A",
	));
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some("AA"));
}

#[test]
fn backspace_clears_recent_input_dedupe_marker_before_cross_source_retype() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));

	let key_generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Key,
		key_generation,
		"A",
	));
	assert!(session.backspace_frozen_text_edit());
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some(""));

	let ime_generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Ime,
		ime_generation,
		"A",
	));
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some("A"));
}

#[test]
fn text_input_resets_frozen_text_caret_blink_phase() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));

	let stale_started_at = Instant::now() - FROZEN_TEXT_CARET_REPAINT_INTERVAL * 3;

	session.frozen_text_edit.as_mut().expect("text edit").reset_caret_blink_at(stale_started_at);

	let generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Key,
		generation,
		"A",
	));

	let edit_state = session.frozen_text_edit.as_ref().expect("text edit");

	assert!(edit_state.caret_blink_started_at > stale_started_at);
	assert!(edit_state.caret_blink_elapsed_secs_at(edit_state.caret_blink_started_at) == 0.0);
}

#[test]
fn ime_preedit_updates_reset_frozen_text_caret_blink_phase() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));

	let stale_started_at = Instant::now() - FROZEN_TEXT_CARET_REPAINT_INTERVAL * 3;

	session.frozen_text_edit.as_mut().expect("text edit").reset_caret_blink_at(stale_started_at);

	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((0, 0))));

	let edit_state = session.frozen_text_edit.as_ref().expect("text edit");

	assert!(edit_state.caret_blink_started_at > stale_started_at);
}

#[test]
fn ime_disabled_clears_frozen_text_preedit_state() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((0, 0))));
	assert!(session.frozen_text_edit.as_ref().is_some_and(FrozenTextEditState::has_ime_preedit));
	assert!(session.apply_frozen_text_ime_event(&Ime::Disabled));
	assert_eq!(
		session.frozen_text_edit.as_ref().and_then(|edit| edit.ime_preedit.as_deref()),
		None
	);
	assert!(!session.frozen_text_edit.as_ref().is_some_and(FrozenTextEditState::has_ime_preedit));
}

#[test]
fn frozen_text_preedit_cursor_range_updates_caret_position() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("A"));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉字")), Some((3, 3))));

	let edit_state = session.frozen_text_edit.as_ref().expect("text edit");

	assert_eq!(edit_state.visible_text(), "A汉字");
	assert_eq!(edit_state.visible_text_and_caret_char_index().1, Some(2));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉字")), Some((0, 0))));
	assert_eq!(
		session
			.frozen_text_edit
			.as_ref()
			.and_then(|edit| edit.visible_text_and_caret_char_index().1),
		Some(1)
	);
}

#[test]
fn frozen_text_style_change_refresh_check_requires_active_ime_preedit() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(!session.should_refresh_frozen_text_ime_cursor_area_for_text_style_change(monitor));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((0, 0))));
	assert!(session.should_refresh_frozen_text_ime_cursor_area_for_text_style_change(monitor));
}

#[test]
fn frozen_text_style_change_refresh_check_ignores_other_monitor() {
	let monitor = tests::test_monitor();
	let other_monitor = MonitorRect {
		id: 2,
		origin: GlobalPoint::new(1_000, 0),
		width: monitor.width,
		height: monitor.height,
		scale_factor_x1000: monitor.scale_factor_x1000,
	};
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((0, 0))));
	assert!(
		!session.should_refresh_frozen_text_ime_cursor_area_for_text_style_change(other_monitor)
	);
}

#[test]
fn frozen_text_enter_does_not_finish_while_ime_preedit_is_active() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("A"));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((3, 3))));
	assert!(!session.handle_frozen_text_pressed_key(&Key::Named(NamedKey::Enter), None));
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some("A"));
	assert_eq!(
		session.frozen_text_edit.as_ref().and_then(|edit| edit.ime_preedit.as_deref()),
		Some("汉")
	);
	assert!(session.frozen_text_annotations.is_empty());
}

#[test]
fn frozen_text_caret_repaint_schedules_delayed_repaint_while_editing() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.frozen_text_edit = Some(FrozenTextEditState::new(Pos2::new(120.0, 140.0)));
	*session.egui_repaint_deadline.lock().unwrap_or_else(|err| err.into_inner()) = None;

	let started_at = Instant::now();

	session.maybe_keep_frozen_text_caret_repaint();

	let deadline = session
		.egui_repaint_deadline
		.lock()
		.unwrap_or_else(|err| err.into_inner())
		.expect("caret repaint should be scheduled");

	assert!(deadline >= started_at);
	assert!(
		deadline <= started_at + FROZEN_TEXT_CARET_REPAINT_INTERVAL + Duration::from_millis(20)
	);
}

#[test]
fn finish_frozen_text_editing_commits_current_toolbar_text_style() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;
	session.toolbar_state.text_style.font_size_points = 27.5;
	session.toolbar_state.text_style.color = FrozenAnnotationColor::Yellow;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("Styled"));
	assert!(session.finish_frozen_text_editing(true));

	let annotation = session.frozen_text_annotations.first().expect("annotation");

	assert_eq!(annotation.style.font_size_points, 27.5);
	assert_eq!(annotation.style.color, FrozenAnnotationColor::Yellow);
}

#[test]
fn finish_frozen_brush_stroke_commits_current_toolbar_brush_style() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));

	tests::promote_session_export_authority_ready(&mut session);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;
	session.toolbar_state.brush_style.stroke_width_points = 4.25;
	session.toolbar_state.brush_style.color = FrozenAnnotationColor::Green;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(140, 160)));
	assert!(session.finish_frozen_brush_stroke());

	let stroke = session.frozen_brush.committed_strokes.first().expect("brush stroke");

	assert_eq!(stroke.style.stroke_width_points, 4.25);
	assert_eq!(stroke.style.color, FrozenAnnotationColor::Green);
}

#[test]
fn finish_frozen_text_editing_commits_active_ime_preedit_text() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("A"));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((3, 3))));
	assert!(session.finish_frozen_text_editing(true));
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert_eq!(session.frozen_text_annotations[0].text, "A汉");
}

#[test]
fn inline_toolbar_mode_switch_finishes_active_frozen_text_edit() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));

	tests::promote_session_export_authority_ready(&mut session);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("Switched"));

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pointer;

	let _ = session.handle_capture_and_toolbar_redraw_post(monitor, true);

	assert!(session.frozen_text_edit.is_none());
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert_eq!(session.frozen_text_annotations[0].text, "Switched");
	assert_eq!(session.toolbar_state.selected_tool, FrozenToolbarTool::Pointer);
}

#[test]
fn inline_toolbar_mode_switch_commits_active_ime_preedit_text() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));

	tests::promote_session_export_authority_ready(&mut session);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("A"));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((3, 3))));

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pointer;

	let _ = session.handle_capture_and_toolbar_redraw_post(monitor, true);

	assert!(session.frozen_text_edit.is_none());
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert_eq!(session.frozen_text_annotations[0].text, "A汉");
}

#[test]
fn frozen_text_undo_and_redo_round_trip_annotations() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("Undoable"));
	assert!(session.finish_frozen_text_editing(true));
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
	assert!(session.perform_frozen_undo());
	assert!(session.frozen_text_annotations.is_empty());
	assert!(!session.toolbar_state.undo_available);
	assert!(session.toolbar_state.redo_available);
	assert!(session.perform_frozen_redo());
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert_eq!(session.frozen_text_annotations[0].text, "Undoable");
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
}

#[test]
fn frozen_committed_overlay_iteration_preserves_cross_tool_order() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session
		.state
		.commit_frozen_final_image(monitor, RgbaImage::from_pixel(16, 16, Rgba([0, 0, 0, 255])));

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 16, 16));

	tests::promote_session_export_authority_ready(&mut session);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(2, 2)));
	assert!(session.finish_frozen_brush_stroke());

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(6, 6)));
	assert!(session.append_text_to_frozen_edit("middle"));
	assert!(session.finish_frozen_text_editing(true));

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(10, 10)));
	assert!(session.finish_frozen_brush_stroke());

	let mut observed = Vec::new();

	OverlaySession::for_each_frozen_committed_overlay(
		&session.frozen_edit_undo_stack,
		&session.frozen_brush.committed_strokes,
		&session.frozen_arrow_annotations,
		&session.frozen_text_annotations,
		|overlay| match overlay {
			FrozenCommittedOverlay::Brush(stroke) => {
				observed.push(format!("brush:{:.0}", stroke.points[0].x));
			},
			FrozenCommittedOverlay::Arrow(annotation) => {
				observed.push(format!("arrow:{:.0}", annotation.start.x));
			},
			FrozenCommittedOverlay::Text(annotation) => {
				observed.push(format!("text:{}", annotation.text));
			},
		},
	);

	assert_eq!(observed, ["brush:2", "text:middle", "brush:10"]);
}

#[test]
fn frozen_annotation_history_undoes_across_tools_in_reverse_commit_order() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let original = RgbaImage::from_fn(8, 8, |x, y| {
		Rgba([(x * 17) as u8, (y * 23) as u8, ((x + y) * 11) as u8, 255])
	});
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, original.clone());

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));

	tests::promote_session_export_authority_ready(&mut session);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(2, 2)));
	assert!(session.finish_frozen_brush_stroke());

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(3, 4)));
	assert!(session.append_text_to_frozen_edit("Layered"));
	assert!(session.finish_frozen_text_editing(true));

	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;

	assert!(session.begin_frozen_mosaic_drag(GlobalPoint::new(1, 1)));
	assert!(session.update_frozen_mosaic_drag_rect(GlobalPoint::new(4, 4)));
	assert!(session.commit_frozen_mosaic_drag());

	let mosaiced = session
		.state
		.frozen_display_image
		.clone()
		.expect("mosaic commit should retain the frozen display image");

	assert!(session.perform_frozen_undo());
	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&original));
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert_eq!(session.frozen_brush.committed_strokes.len(), 1);
	assert!(session.perform_frozen_undo());
	assert!(session.frozen_text_annotations.is_empty());
	assert_eq!(session.frozen_brush.committed_strokes.len(), 1);
	assert!(session.perform_frozen_undo());
	assert!(session.frozen_brush.committed_strokes.is_empty());
	assert!(!session.toolbar_state.undo_available);
	assert!(session.toolbar_state.redo_available);
	assert!(session.perform_frozen_redo());
	assert_eq!(session.frozen_brush.committed_strokes.len(), 1);
	assert!(session.frozen_text_annotations.is_empty());
	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&original));
	assert!(session.perform_frozen_redo());
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&original));
	assert!(session.perform_frozen_redo());
	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&mosaiced));
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
}

#[test]
fn committing_new_frozen_edit_clears_redo_across_tools() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let base = RgbaImage::from_fn(8, 8, |x, y| {
		Rgba([(x * 11) as u8, (y * 13) as u8, ((x + y) * 7) as u8, 255])
	});
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, base);

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));

	tests::promote_session_export_authority_ready(&mut session);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(2, 2)));
	assert!(session.append_text_to_frozen_edit("redo"));
	assert!(session.finish_frozen_text_editing(true));
	assert!(session.perform_frozen_undo());
	assert!(session.toolbar_state.redo_available);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;

	assert!(session.begin_frozen_mosaic_drag(GlobalPoint::new(1, 1)));
	assert!(session.update_frozen_mosaic_drag_rect(GlobalPoint::new(4, 4)));
	assert!(session.commit_frozen_mosaic_drag());
	assert!(!session.toolbar_state.redo_available);
	assert!(!session.perform_frozen_redo());
	assert!(session.frozen_text_annotations.is_empty());
}

#[test]
fn evicting_old_mosaic_history_also_discards_patch_payloads() {
	let mut session = OverlaySession::new();

	for _ in 0..FROZEN_EDIT_HISTORY_LIMIT {
		session.push_frozen_mosaic_edit(tests::test_frozen_mosaic_edit());
		session.push_frozen_edit_to_undo_history(FrozenEditKind::MosaicEdit);
	}

	assert_eq!(session.frozen_mosaic_undo_stack.len(), FROZEN_EDIT_HISTORY_LIMIT);

	for _ in 0..FROZEN_EDIT_HISTORY_LIMIT {
		session.push_frozen_edit_to_undo_history(FrozenEditKind::BrushStroke);
	}

	assert_eq!(session.frozen_edit_undo_stack.len(), FROZEN_EDIT_HISTORY_LIMIT);
	assert!(session.frozen_mosaic_undo_stack.is_empty());
}

#[test]
fn evicting_old_brush_and_text_history_discards_matching_payloads() {
	let mut session = OverlaySession::new();

	for index in 0..(FROZEN_EDIT_HISTORY_LIMIT + 2) {
		let x = index as f32;

		if index % 2 == 0 {
			session.frozen_brush.committed_strokes.push(FrozenBrushStroke {
				points: vec![Pos2::new(x, 0.0)],
				style: FrozenBrushStyle::default(),
			});
			session.push_frozen_edit_to_undo_history(FrozenEditKind::BrushStroke);
		} else {
			session.frozen_text_annotations.push(FrozenTextAnnotation {
				anchor: Pos2::new(x, 0.0),
				text: format!("text-{index}"),
				style: session.toolbar_state.text_style,
			});
			session.push_frozen_edit_to_undo_history(FrozenEditKind::TextAnnotation);
		}
	}

	assert_eq!(session.frozen_edit_undo_stack.len(), FROZEN_EDIT_HISTORY_LIMIT);
	assert_eq!(session.frozen_brush.committed_strokes.len(), FROZEN_EDIT_HISTORY_LIMIT / 2);
	assert_eq!(session.frozen_text_annotations.len(), FROZEN_EDIT_HISTORY_LIMIT / 2);
	assert_eq!(session.frozen_brush.committed_strokes[0].points[0], Pos2::new(2.0, 0.0));
	assert_eq!(session.frozen_text_annotations[0].text, "text-3");

	let mut observed = Vec::new();

	OverlaySession::for_each_frozen_committed_overlay(
		&session.frozen_edit_undo_stack,
		&session.frozen_brush.committed_strokes,
		&session.frozen_arrow_annotations,
		&session.frozen_text_annotations,
		|overlay| match overlay {
			FrozenCommittedOverlay::Brush(stroke) => {
				observed.push(format!("brush:{:.0}", stroke.points[0].x));
			},
			FrozenCommittedOverlay::Arrow(annotation) => {
				observed.push(format!("arrow:{:.0}", annotation.start.x));
			},
			FrozenCommittedOverlay::Text(annotation) => observed.push(annotation.text.clone()),
		},
	);

	let expected = (2..(FROZEN_EDIT_HISTORY_LIMIT + 2))
		.map(
			|index| {
				if index % 2 == 0 { format!("brush:{index}") } else { format!("text-{index}") }
			},
		)
		.collect::<Vec<_>>();

	assert_eq!(observed, expected);
}
