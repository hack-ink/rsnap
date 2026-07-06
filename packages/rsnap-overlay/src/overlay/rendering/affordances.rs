pub(super) mod size_badge;

mod frozen_annotations;
mod selection_flow;
mod selection_scrim;
mod toolbar;

use egui::Context;

use crate::overlay::frozen_selection_geometry::LIVE_DRAG_START_THRESHOLD_PX;
use crate::overlay::frozen_selection_handles::{
	self, FrozenSelectionResizeHandleGeometry, RESIZE_HANDLE_CENTER_DOT_RADIUS_POINTS,
	RESIZE_HANDLE_OUTER_RADIUS_POINTS, RESIZE_HANDLE_STROKE_WIDTH_POINTS,
};
use crate::overlay::rendering::{
	SelectionDashedBorderCache, SelectionFlowGeometryCache, WindowRenderer,
};
use crate::overlay::{
	Color32, FrozenArrowAnnotation, FrozenBrushState, FrozenCaptureSource, FrozenEditKind,
	FrozenSelectionCorner, FrozenSpotlightAnnotation, FrozenTextAnnotation, FrozenTextEditState,
	FrozenTextStyle, HudTheme, Id, LayerId, MonitorRect, Order, OverlayMode, OverlayState, Painter,
	Pos2, Rect, RectPoints, SelectionFlowStyle, Stroke, Vec2,
};

impl WindowRenderer {
	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn render_live_capture_affordances(
		ctx: &Context,
		painter: &Painter,
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
		theme: HudTheme,
		selection_flow_enabled: bool,
		selection_flow_stroke_width_px: f32,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		let mut has_rect = false;

		if !matches!(state.mode, OverlayMode::Live | OverlayMode::Frozen) {
			return false;
		}

		let primary_not_down = !ctx.input(|i| i.pointer.primary_down());

		if let Some(hovered_window) = state.hovered_window_rect
			&& hovered_window.monitor_id == monitor.id
		{
			let rect = Rect::from_min_size(
				Pos2::new(hovered_window.rect.x as f32, hovered_window.rect.y as f32),
				Vec2::new(hovered_window.rect.width as f32, hovered_window.rect.height as f32),
			);
			let rect = rect.intersect(screen_rect);

			if rect.width() >= LIVE_DRAG_START_THRESHOLD_PX
				&& rect.height() >= LIVE_DRAG_START_THRESHOLD_PX
			{
				Self::render_live_drag_selection_scrim(painter, rect, screen_rect, theme);

				if selection_flow_enabled {
					Self::render_selection_flow_ring(
						painter,
						rect,
						ctx,
						theme,
						SelectionFlowStyle::Band,
						selection_flow_stroke_width_px,
						selection_flow_geometry_cache,
					);
				}

				has_rect = true;
			}
		}
		if let Some(rect) = Self::live_drag_focus_rect(state, monitor, screen_rect) {
			Self::render_live_drag_selection_affordance(
				painter,
				rect,
				screen_rect,
				theme,
				selection_dashed_border_cache,
			);

			has_rect = true;
		}
		if let Some(target) =
			Self::live_capture_size_badge_target(state, monitor, screen_rect, primary_not_down)
		{
			Self::render_selection_size_badge(
				ctx,
				painter,
				monitor,
				screen_rect,
				target,
				None,
				false,
				theme,
			);

			has_rect = true;
		}

		has_rect
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn render_pending_frozen_display_handoff_affordance(
		ctx: &Context,
		painter: &Painter,
		state: &OverlayState,
		monitor: MonitorRect,
		pending_handoff_monitor: Option<MonitorRect>,
		screen_rect: Rect,
		theme: HudTheme,
		selection_flow_enabled: bool,
		selection_flow_stroke_width_px: f32,
		frozen_capture_source: FrozenCaptureSource,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		if !matches!(state.mode, OverlayMode::Live | OverlayMode::Frozen) {
			return false;
		}
		if pending_handoff_monitor != Some(monitor) {
			return false;
		}

		let Some(capture_rect) = state.frozen_capture_rect else {
			return false;
		};
		let focus_rect = Self::selection_focus_rect(capture_rect, screen_rect);

		if focus_rect.width() <= 0.0 || focus_rect.height() <= 0.0 {
			return false;
		}

		match frozen_capture_source {
			FrozenCaptureSource::None => false,
			FrozenCaptureSource::DragRegion => Self::render_live_drag_selection_affordance(
				painter,
				focus_rect,
				screen_rect,
				theme,
				selection_dashed_border_cache,
			),
			FrozenCaptureSource::Window | FrozenCaptureSource::FullscreenFallback => {
				let mut rendered =
					Self::render_live_drag_selection_scrim(painter, focus_rect, screen_rect, theme);

				if selection_flow_enabled
					&& focus_rect.width() >= LIVE_DRAG_START_THRESHOLD_PX
					&& focus_rect.height() >= LIVE_DRAG_START_THRESHOLD_PX
				{
					Self::render_selection_flow_ring(
						painter,
						focus_rect,
						ctx,
						theme,
						SelectionFlowStyle::Band,
						selection_flow_stroke_width_px,
						selection_flow_geometry_cache,
					);

					rendered = true;
				}

				rendered
			},
		}
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn render_frozen_capture_affordance(
		ctx: &Context,
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
		theme: HudTheme,
		frozen_selection_resize_handles_enabled: bool,
		frozen_capture_source: FrozenCaptureSource,
		frozen_toolbar_reserved_rect: Option<Rect>,
		frozen_edit_history: &[FrozenEditKind],
		frozen_brush_state: Option<&FrozenBrushState>,
		frozen_arrow_annotations: &[FrozenArrowAnnotation],
		frozen_arrow_preview: Option<&FrozenArrowAnnotation>,
		frozen_spotlight_annotations: &[FrozenSpotlightAnnotation],
		frozen_spotlight_preview_rect: Option<RectPoints>,
		frozen_text_annotations: &[FrozenTextAnnotation],
		frozen_text_edit: Option<&FrozenTextEditState>,
		frozen_text_style: FrozenTextStyle,
		_frozen_capture_is_fullscreen_fallback: bool,
		_selection_flow_enabled: bool,
		_selection_flow_stroke_width_px: f32,
		_selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		let Some(capture_rect_points) = state.frozen_capture_rect else {
			return false;
		};
		let Some(rect) = Self::frozen_capture_focus_rect(state, screen_rect) else {
			return false;
		};
		let layer =
			LayerId::new(Order::Foreground, Id::new(format!("frozen-pending-{}", monitor.id)));
		let painter = ctx.layer_painter(layer);
		let show_resize_handles = frozen_selection_resize_handles_enabled
			&& frozen_capture_source == FrozenCaptureSource::DragRegion;
		let mut has_affordance = Self::render_frozen_selection_scrim(
			&painter,
			rect,
			screen_rect,
			theme,
			show_resize_handles,
			selection_dashed_border_cache,
		);
		let brush_painter = painter.with_clip_rect(rect);

		has_affordance |= Self::render_frozen_spotlight_annotations(
			&brush_painter,
			capture_rect_points,
			screen_rect,
			frozen_spotlight_annotations,
			frozen_spotlight_preview_rect,
			theme,
			selection_dashed_border_cache,
		);
		has_affordance |= Self::render_frozen_committed_overlay_annotations(
			&brush_painter,
			frozen_edit_history,
			frozen_brush_state,
			frozen_arrow_annotations,
			frozen_text_annotations,
		);

		if let Some(target) = Self::frozen_capture_size_badge_target(state, screen_rect) {
			Self::render_selection_size_badge(
				ctx,
				&painter,
				monitor,
				screen_rect,
				target,
				frozen_toolbar_reserved_rect,
				frozen_capture_source == FrozenCaptureSource::DragRegion,
				theme,
			);

			has_affordance = true;
		}

		if show_resize_handles && let Some(capture_rect) = state.frozen_capture_rect {
			has_affordance |=
				Self::render_frozen_selection_resize_handles(&painter, capture_rect, theme);
		}

		if let Some(mosaic_preview_rect) = state.frozen_mosaic_preview_rect {
			let preview_rect = Self::selection_focus_rect(mosaic_preview_rect, screen_rect);
			let preview_fill = match theme {
				HudTheme::Dark => Color32::from_rgba_unmultiplied(110, 196, 255, 38),
				HudTheme::Light => Color32::from_rgba_unmultiplied(34, 132, 214, 30),
			};

			painter.rect_filled(preview_rect, 10.0, preview_fill);

			has_affordance |= Self::render_selection_dashed_border(
				&painter,
				preview_rect,
				screen_rect,
				theme,
				Some(2.1),
				false,
				selection_dashed_border_cache,
			);
		}

		has_affordance |= Self::render_frozen_text_annotations(
			&brush_painter,
			theme,
			&[],
			frozen_text_edit,
			frozen_text_style,
		);

		if let Some(frozen_brush_state) = frozen_brush_state {
			has_affordance |=
				Self::render_active_frozen_brush_stroke(&brush_painter, frozen_brush_state);
		}
		if let Some(arrow_preview) = frozen_arrow_preview {
			has_affordance |= Self::paint_frozen_arrow(&brush_painter, arrow_preview);
		}

		has_affordance
	}

	pub(in crate::overlay) fn frozen_capture_focus_rect(
		state: &OverlayState,
		screen_rect: Rect,
	) -> Option<Rect> {
		let capture_rect = state.frozen_capture_rect?;

		Some(Self::selection_focus_rect(capture_rect, screen_rect))
	}

	pub(in crate::overlay) fn live_drag_focus_rect(
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
	) -> Option<Rect> {
		let drag_rect = state.drag_rect?;

		if drag_rect.monitor_id != monitor.id {
			return None;
		}

		let rect = Self::selection_focus_rect(drag_rect.rect, screen_rect);

		if rect.width() < LIVE_DRAG_START_THRESHOLD_PX
			|| rect.height() < LIVE_DRAG_START_THRESHOLD_PX
		{
			return None;
		}

		Some(rect)
	}

	pub(in crate::overlay) fn selection_focus_rect(rect: RectPoints, screen_rect: Rect) -> Rect {
		Rect::from_min_size(
			Pos2::new(rect.x as f32, rect.y as f32),
			Vec2::new(rect.width as f32, rect.height as f32),
		)
		.intersect(screen_rect)
	}

	pub(in crate::overlay) fn frozen_selection_resize_handles(
		capture_rect: RectPoints,
	) -> [FrozenSelectionResizeHandleGeometry; 4] {
		frozen_selection_handles::resize_handles(capture_rect)
	}

	pub(in crate::overlay) fn frozen_selection_resize_hit_test(
		capture_rect: RectPoints,
		cursor_local: Pos2,
	) -> Option<FrozenSelectionCorner> {
		let rect = Rect::from_min_size(
			Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
			Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
		);
		let mut best_corner = None;
		let mut best_distance_sq = f32::MAX;

		for handle in Self::frozen_selection_resize_handles(capture_rect) {
			if !handle.hit_rect.contains(cursor_local) {
				continue;
			}
			if rect.contains(cursor_local)
				&& !frozen_selection_handles::resize_handle_interior_hit(
					handle.corner,
					rect,
					cursor_local,
				) {
				continue;
			}

			let distance_sq = handle.anchor.distance_sq(cursor_local);

			if distance_sq < best_distance_sq {
				best_corner = Some(handle.corner);
				best_distance_sq = distance_sq;
			}
		}

		best_corner
	}

	fn frozen_selection_resize_handle_outline_stroke(theme: HudTheme) -> Stroke {
		let _ = theme;
		let color = Color32::from_rgba_unmultiplied(229, 247, 255, 124);

		Stroke::new(RESIZE_HANDLE_STROKE_WIDTH_POINTS + 0.6, color)
	}

	fn frozen_selection_resize_handle_stroke(theme: HudTheme) -> Stroke {
		let _ = theme;

		Stroke::new(
			RESIZE_HANDLE_STROKE_WIDTH_POINTS,
			Color32::from_rgba_unmultiplied(167, 223, 255, 246),
		)
	}

	fn frozen_selection_resize_handle_center(handle: FrozenSelectionResizeHandleGeometry) -> Pos2 {
		handle.hit_rect.center()
	}

	fn frozen_selection_resize_handle_center_dot_color(theme: HudTheme) -> Color32 {
		let _ = theme;

		Color32::from_rgba_unmultiplied(167, 223, 255, 252)
	}

	pub(in crate::overlay) fn render_frozen_selection_resize_handles(
		painter: &Painter,
		capture_rect: RectPoints,
		theme: HudTheme,
	) -> bool {
		let outline_stroke = Self::frozen_selection_resize_handle_outline_stroke(theme);
		let stroke = Self::frozen_selection_resize_handle_stroke(theme);
		let center_dot_color = Self::frozen_selection_resize_handle_center_dot_color(theme);

		for handle in Self::frozen_selection_resize_handles(capture_rect) {
			let center = Self::frozen_selection_resize_handle_center(handle);

			painter.circle_stroke(center, RESIZE_HANDLE_OUTER_RADIUS_POINTS, outline_stroke);
			painter.circle_stroke(center, RESIZE_HANDLE_OUTER_RADIUS_POINTS, stroke);
			painter.circle_filled(center, RESIZE_HANDLE_CENTER_DOT_RADIUS_POINTS, center_dot_color);
		}

		true
	}
}
