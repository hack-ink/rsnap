use egui::Context;

#[allow(unused_imports)]
use crate::overlay::rendering::{
	FrozenToolbarButtonStyle, SelectionDashedBorderCache, SelectionDashedBorderCacheKey,
	SelectionDashedBorderMetrics, SelectionFlowGeometryCache, SelectionFlowGeometryCacheKey,
	SelectionSizeBadgeLayout, SelectionSizeBadgePadding, SelectionSizeBadgeTarget, WindowRenderer,
};
#[allow(unused_imports)]
use crate::overlay::{
	self, Align, Align2, Area, Color32, CornerRadius, FROZEN_SELECTION_SCRIM_ALPHA_DARK,
	FROZEN_SELECTION_SCRIM_ALPHA_LIGHT, FROZEN_TOOLBAR_BUTTON_SIZE_POINTS,
	FROZEN_TOOLBAR_ITEM_SPACING_POINTS, FontFamily, FontId, FrozenCaptureSource,
	FrozenToolbarPointerState, FrozenToolbarState, FrozenToolbarTool,
	HUD_PILL_CORNER_RADIUS_POINTS, HUD_PILL_INNER_MARGIN_X_POINTS, HUD_PILL_INNER_MARGIN_Y_POINTS,
	HUD_PILL_STROKE_WIDTH_POINTS, HudPillGeometry, HudTheme, Id,
	LIVE_DRAG_SELECTION_SCRIM_ALPHA_DARK, LIVE_DRAG_SELECTION_SCRIM_ALPHA_LIGHT,
	LIVE_DRAG_START_THRESHOLD_PX, LayerId, Layout, Mesh, MonitorRect, Order, OverlayMode,
	OverlayState, Painter, Pos2, Rect, RectPoints, SELECTION_DASHED_BORDER_ALPHA,
	SELECTION_DASHED_BORDER_DASH_LENGTH_PX, SELECTION_DASHED_BORDER_GAP_LENGTH_PX,
	SELECTION_DASHED_BORDER_WIDTH_PX, SELECTION_FLOW_CORE_FLOW_WIDTH,
	SELECTION_FLOW_CORNER_RADIUS_PX, SELECTION_FLOW_FLOW_BOOST, SELECTION_FLOW_FROZEN_ALPHA_SCALE,
	SELECTION_FLOW_FROZEN_INTENSITY, SELECTION_FLOW_LIGHT_PALETTE, SELECTION_FLOW_MAX_SEGMENTS,
	SELECTION_FLOW_MIN_SEGMENTS, SELECTION_FLOW_PALETTE, SELECTION_FLOW_SAMPLE_STEP_PX,
	SELECTION_FLOW_SPEED, SELECTION_SIZE_BADGE_FAR_SHADOW_OFFSET_PX,
	SELECTION_SIZE_BADGE_FONT_SIZE_POINTS, SELECTION_SIZE_BADGE_GAP_PX,
	SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX, SELECTION_SIZE_BADGE_NEAR_SHADOW_OFFSET_PX,
	SELECTION_SIZE_BADGE_OUTLINE_OFFSET_PX, SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX,
	SELECTION_SIZE_BADGE_TEXT_OUTSET_POINTS, SelectionFlowStyle, Sense, Shape, Stroke, StrokeKind,
	TOOLBAR_CAPTURE_GAP_PX, TOOLBAR_EXPANDED_HEIGHT_PX, TOOLBAR_SCREEN_MARGIN_PX, ToolbarPlacement,
	Ui, UiBuilder, Vec2, regular,
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
	) -> bool {
		let mut has_rect = false;

		if !matches!(state.mode, OverlayMode::Live) {
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
			Self::render_live_drag_selection_scrim(painter, rect, screen_rect, theme);

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

		let has_hovered_window_for_this_monitor =
			state.hovered_window_rect.is_some_and(|hovered| hovered.monitor_id == monitor.id);
		let has_drag_rect_for_this_monitor =
			state.drag_rect.is_some_and(|drag_rect| drag_rect.monitor_id == monitor.id);
		let cursor_on_monitor = state.cursor.is_some_and(|cursor| monitor.contains(cursor));

		if selection_flow_enabled
			&& !has_hovered_window_for_this_monitor
			&& !has_drag_rect_for_this_monitor
			&& cursor_on_monitor
			&& primary_not_down
		{
			Self::render_selection_flow_ring(
				painter,
				screen_rect,
				ctx,
				theme,
				SelectionFlowStyle::Band,
				selection_flow_stroke_width_px,
				selection_flow_geometry_cache,
			);

			has_rect = true;
		}

		has_rect
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn render_frozen_capture_affordance(
		ctx: &Context,
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
		theme: HudTheme,
		frozen_capture_source: FrozenCaptureSource,
		frozen_toolbar_reserved_rect: Option<Rect>,
		frozen_capture_is_fullscreen_fallback: bool,
		selection_flow_enabled: bool,
		selection_flow_stroke_width_px: f32,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		let Some(rect) = Self::frozen_capture_focus_rect(state, screen_rect) else {
			return false;
		};
		let layer =
			LayerId::new(Order::Foreground, Id::new(format!("frozen-pending-{}", monitor.id)));
		let painter = ctx.layer_painter(layer);

		if state.frozen_image.is_some() {
			let mut has_affordance = Self::render_frozen_selection_scrim(
				&painter,
				rect,
				screen_rect,
				theme,
				selection_dashed_border_cache,
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

			return has_affordance;
		}
		if !selection_flow_enabled {
			let mut has_affordance = Self::render_frozen_selection_scrim(
				&painter,
				rect,
				screen_rect,
				theme,
				selection_dashed_border_cache,
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

			return has_affordance;
		}

		Self::render_selection_flow_ring(
			&painter,
			rect,
			ctx,
			theme,
			if frozen_capture_is_fullscreen_fallback {
				SelectionFlowStyle::Band
			} else {
				SelectionFlowStyle::FullBorder
			},
			selection_flow_stroke_width_px,
			selection_flow_geometry_cache,
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
		}

		true
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

	pub(in crate::overlay) fn selection_size_badge_target_from_rect(
		rect_points: RectPoints,
		screen_rect: Rect,
	) -> Option<SelectionSizeBadgeTarget> {
		let rect = Self::selection_focus_rect(rect_points, screen_rect);

		if rect.width() <= 0.0 || rect.height() <= 0.0 {
			return None;
		}

		Some(SelectionSizeBadgeTarget { rect, size_points: rect_points })
	}

	pub(in crate::overlay) fn live_capture_size_badge_target(
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
		primary_not_down: bool,
	) -> Option<SelectionSizeBadgeTarget> {
		if let Some(drag_rect) = state.drag_rect
			&& drag_rect.monitor_id == monitor.id
			&& let Some(target) =
				Self::selection_size_badge_target_from_rect(drag_rect.rect, screen_rect)
		{
			return Some(target);
		}
		if let Some(hovered_window) = state.hovered_window_rect
			&& hovered_window.monitor_id == monitor.id
			&& let Some(target) =
				Self::selection_size_badge_target_from_rect(hovered_window.rect, screen_rect)
		{
			return Some(target);
		}

		if primary_not_down && state.cursor.is_some_and(|cursor| monitor.contains(cursor)) {
			return Some(SelectionSizeBadgeTarget {
				rect: screen_rect,
				size_points: RectPoints::new(0, 0, monitor.width, monitor.height),
			});
		}

		None
	}

	pub(in crate::overlay) fn frozen_capture_size_badge_target(
		state: &OverlayState,
		screen_rect: Rect,
	) -> Option<SelectionSizeBadgeTarget> {
		let capture_rect = state.frozen_capture_rect?;

		Self::selection_size_badge_target_from_rect(capture_rect, screen_rect)
	}

	pub(in crate::overlay) fn frozen_toolbar_reserved_rect(
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
		toolbar_placement: ToolbarPlacement,
		toolbar_state: &FrozenToolbarState,
	) -> Option<Rect> {
		if !toolbar_state.visible
			|| !matches!(state.mode, OverlayMode::Frozen)
			|| state.monitor != Some(monitor)
		{
			return None;
		}

		let capture_rect = Self::frozen_toolbar_capture_rect(state, monitor, screen_rect);
		let toolbar_size = Self::frozen_toolbar_size(toolbar_state);
		let default_pos = Self::frozen_toolbar_default_pos(
			screen_rect,
			capture_rect,
			toolbar_size,
			toolbar_placement,
		);
		let toolbar_pos = toolbar_state.floating_position.unwrap_or(default_pos);

		if !overlay::frozen_toolbar_matches_default_slot(toolbar_pos, default_pos) {
			return None;
		}

		Some(Rect::from_min_size(toolbar_pos, toolbar_size))
	}

	pub(in crate::overlay) fn selection_size_badge_text(
		monitor: MonitorRect,
		size_points: RectPoints,
	) -> String {
		let size_pixels = monitor.local_rect_to_pixels(size_points);

		format!("{}x{}", size_pixels.width, size_pixels.height)
	}

	fn selection_size_badge_visual_overflow(pixels_per_point: f32) -> SelectionSizeBadgePadding {
		let points_per_pixel = 1.0 / pixels_per_point.max(f32::MIN_POSITIVE);
		let outline_offset = SELECTION_SIZE_BADGE_OUTLINE_OFFSET_PX * points_per_pixel;
		let near_shadow_offset = SELECTION_SIZE_BADGE_NEAR_SHADOW_OFFSET_PX * points_per_pixel;
		let far_shadow_offset = SELECTION_SIZE_BADGE_FAR_SHADOW_OFFSET_PX * points_per_pixel;

		SelectionSizeBadgePadding {
			left: outline_offset,
			right: outline_offset.max(near_shadow_offset),
			top: outline_offset,
			bottom: outline_offset.max(near_shadow_offset).max(far_shadow_offset),
		}
	}

	pub(in crate::overlay) fn selection_size_badge_layout(
		ctx: &Context,
		text: &str,
		theme: HudTheme,
		pixels_per_point: f32,
	) -> SelectionSizeBadgeLayout {
		let text_color = Self::hud_text_colors(theme).0;
		let font_id = FontId::new(SELECTION_SIZE_BADGE_FONT_SIZE_POINTS, FontFamily::Monospace);
		let galley = ctx
			.fonts_mut(|fonts| fonts.layout_no_wrap(text.to_owned(), font_id.clone(), text_color));
		let text_size = galley.size();
		let visual_overflow = Self::selection_size_badge_visual_overflow(pixels_per_point);
		let base_padding = SELECTION_SIZE_BADGE_TEXT_OUTSET_POINTS * 0.5;
		let padding = SelectionSizeBadgePadding {
			left: base_padding + visual_overflow.left,
			right: base_padding + visual_overflow.right,
			top: base_padding + visual_overflow.top,
			bottom: base_padding + visual_overflow.bottom,
		};

		SelectionSizeBadgeLayout {
			text_size,
			badge_size: Vec2::new(
				(text_size.x + padding.left + padding.right).ceil(),
				(text_size.y + padding.top + padding.bottom).ceil(),
			),
			padding,
		}
	}

	#[cfg(test)]
	pub(in crate::overlay) fn selection_size_badge_rect(
		screen_rect: Rect,
		capture_rect: Rect,
		badge_size: Vec2,
	) -> Rect {
		Self::selection_size_badge_rect_with_reserved_rect(
			screen_rect,
			capture_rect,
			badge_size,
			None,
		)
	}

	pub(in crate::overlay) fn selection_size_badge_rect_with_reserved_rect(
		screen_rect: Rect,
		capture_rect: Rect,
		badge_size: Vec2,
		reserved_rect: Option<Rect>,
	) -> Rect {
		// Geometry priority contract:
		// 1. Keep the badge fully visible inside the viewport whenever the viewport can fit it.
		// 2. Keep the badge right-aligned to the capture rect whenever that still satisfies (1).
		// 3. Prefer the below-capture slot when it fits and does not hit a reserved rect.
		// 4. Otherwise stay inside the capture while avoiding the reserved rect when a
		//    non-overlapping inside band exists.
		// 5. If the reserved rect exhausts the in-capture space, try a right-aligned
		//    above-capture slot before accepting overlap.
		let min_x = screen_rect.min.x;
		let max_x = (screen_rect.max.x - badge_size.x).max(min_x);
		let aligned_x = capture_rect.max.x - badge_size.x;
		let x = aligned_x.clamp(min_x, max_x);
		let below_y = capture_rect.max.y + SELECTION_SIZE_BADGE_GAP_PX;
		let below_rect = Rect::from_min_size(Pos2::new(x, below_y), badge_size);
		let fits_below = below_rect.max.y
			<= screen_rect.max.y - SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX
			&& reserved_rect.is_none_or(|rect| !below_rect.intersects(rect));

		if fits_below {
			return below_rect;
		}

		let screen_max_y = (screen_rect.max.y - badge_size.y).max(screen_rect.min.y);
		let max_inside_y =
			(capture_rect.max.y - badge_size.y).min(screen_max_y).max(screen_rect.min.y);
		let min_inside_y = capture_rect.min.y.min(max_inside_y).max(screen_rect.min.y);
		let preferred_inside_y =
			(capture_rect.max.y - SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX - badge_size.y)
				.clamp(min_inside_y, max_inside_y);
		let preferred_inside_rect =
			Rect::from_min_size(Pos2::new(x, preferred_inside_y), badge_size);

		if reserved_rect.is_none_or(|rect| !preferred_inside_rect.intersects(rect)) {
			return preferred_inside_rect;
		}

		if let Some(reserved_rect) = reserved_rect {
			let upper_y =
				reserved_rect.min.y - SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX - badge_size.y;
			let lower_y = reserved_rect.max.y + SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX;
			let candidate_ys = if reserved_rect.center().y <= capture_rect.center().y {
				[Some(lower_y), Some(upper_y)]
			} else {
				[Some(upper_y), Some(lower_y)]
			};

			for candidate_y in candidate_ys.into_iter().flatten() {
				if candidate_y < min_inside_y || candidate_y > max_inside_y {
					continue;
				}

				let candidate_rect = Rect::from_min_size(Pos2::new(x, candidate_y), badge_size);

				if !candidate_rect.intersects(reserved_rect) {
					return candidate_rect;
				}
			}

			let above_y = capture_rect.min.y - SELECTION_SIZE_BADGE_GAP_PX - badge_size.y;

			if above_y >= screen_rect.min.y {
				let above_rect = Rect::from_min_size(Pos2::new(x, above_y), badge_size);

				if !above_rect.intersects(reserved_rect) {
					return above_rect;
				}
			}
		}

		preferred_inside_rect
	}

	pub(in crate::overlay) fn selection_size_badge_rect_preferring_outside_with_reserved_rect(
		screen_rect: Rect,
		capture_rect: Rect,
		badge_size: Vec2,
		reserved_rect: Option<Rect>,
	) -> Rect {
		let min_x = screen_rect.min.x;
		let max_x = (screen_rect.max.x - badge_size.x).max(min_x);
		let aligned_x = capture_rect.max.x - badge_size.x;
		let x = aligned_x.clamp(min_x, max_x);
		let below_y = capture_rect.max.y + SELECTION_SIZE_BADGE_GAP_PX;
		let below_rect = Rect::from_min_size(Pos2::new(x, below_y), badge_size);
		let fits_below = below_rect.max.y
			<= screen_rect.max.y - SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX
			&& reserved_rect.is_none_or(|rect| !below_rect.intersects(rect));

		if fits_below {
			return below_rect;
		}

		let above_y = capture_rect.min.y - SELECTION_SIZE_BADGE_GAP_PX - badge_size.y;
		let above_rect = Rect::from_min_size(Pos2::new(x, above_y), badge_size);
		let fits_above = above_rect.min.y >= screen_rect.min.y
			&& reserved_rect.is_none_or(|rect| !above_rect.intersects(rect));

		if fits_above {
			return above_rect;
		}

		Self::selection_size_badge_rect_with_reserved_rect(
			screen_rect,
			capture_rect,
			badge_size,
			reserved_rect,
		)
	}

	pub(in crate::overlay) fn snap_points_to_pixel_grid(value: f32, pixels_per_point: f32) -> f32 {
		let pixels_per_point = pixels_per_point.max(f32::MIN_POSITIVE);

		(value * pixels_per_point).round() / pixels_per_point
	}

	pub(in crate::overlay) fn snap_pos_to_pixel_grid(pos: Pos2, pixels_per_point: f32) -> Pos2 {
		Pos2::new(
			Self::snap_points_to_pixel_grid(pos.x, pixels_per_point),
			Self::snap_points_to_pixel_grid(pos.y, pixels_per_point),
		)
	}

	pub(in crate::overlay) fn selection_size_badge_text_anchor(
		badge_rect: Rect,
		layout: SelectionSizeBadgeLayout,
		pixels_per_point: f32,
	) -> Pos2 {
		Self::snap_pos_to_pixel_grid(
			Pos2::new(
				badge_rect.max.x - layout.padding.right,
				badge_rect.min.y + layout.padding.top + layout.text_size.y * 0.5,
			),
			pixels_per_point,
		)
	}

	#[cfg(test)]
	pub(in crate::overlay) fn selection_size_badge_visual_bounds(
		text_anchor: Pos2,
		text_size: Vec2,
		pixels_per_point: f32,
	) -> Rect {
		let visual_overflow = Self::selection_size_badge_visual_overflow(pixels_per_point);

		Rect::from_min_max(
			Pos2::new(
				text_anchor.x - text_size.x - visual_overflow.left,
				text_anchor.y - text_size.y * 0.5 - visual_overflow.top,
			),
			Pos2::new(
				text_anchor.x + visual_overflow.right,
				text_anchor.y + text_size.y * 0.5 + visual_overflow.bottom,
			),
		)
	}

	pub(in crate::overlay) fn selection_size_badge_text_colors(
		theme: HudTheme,
	) -> (Color32, Color32, Color32, Color32) {
		match theme {
			HudTheme::Dark => (
				Color32::from_rgba_unmultiplied(255, 255, 255, 248),
				Color32::from_rgba_unmultiplied(0, 0, 0, 108),
				Color32::from_rgba_unmultiplied(0, 0, 0, 154),
				Color32::from_rgba_unmultiplied(0, 0, 0, 72),
			),
			HudTheme::Light => (
				Color32::from_rgba_unmultiplied(255, 255, 255, 252),
				Color32::from_rgba_unmultiplied(0, 0, 0, 156),
				Color32::from_rgba_unmultiplied(0, 0, 0, 196),
				Color32::from_rgba_unmultiplied(0, 0, 0, 96),
			),
		}
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn render_selection_size_badge(
		ctx: &Context,
		painter: &Painter,
		monitor: MonitorRect,
		screen_rect: Rect,
		target: SelectionSizeBadgeTarget,
		reserved_rect: Option<Rect>,
		prefer_outside_fallback: bool,
		theme: HudTheme,
	) {
		let text = Self::selection_size_badge_text(monitor, target.size_points);
		let pixels_per_point = painter.pixels_per_point();
		let layout = Self::selection_size_badge_layout(ctx, &text, theme, pixels_per_point);
		let badge_rect = if prefer_outside_fallback {
			Self::selection_size_badge_rect_preferring_outside_with_reserved_rect(
				screen_rect,
				target.rect,
				layout.badge_size,
				reserved_rect,
			)
		} else {
			Self::selection_size_badge_rect_with_reserved_rect(
				screen_rect,
				target.rect,
				layout.badge_size,
				reserved_rect,
			)
		};
		let font_id = FontId::new(SELECTION_SIZE_BADGE_FONT_SIZE_POINTS, FontFamily::Monospace);
		let points_per_pixel = 1.0 / pixels_per_point.max(f32::MIN_POSITIVE);
		let outline_offset = SELECTION_SIZE_BADGE_OUTLINE_OFFSET_PX * points_per_pixel;
		let near_shadow_offset = SELECTION_SIZE_BADGE_NEAR_SHADOW_OFFSET_PX * points_per_pixel;
		let far_shadow_offset = SELECTION_SIZE_BADGE_FAR_SHADOW_OFFSET_PX * points_per_pixel;
		let text_anchor =
			Self::selection_size_badge_text_anchor(badge_rect, layout, pixels_per_point);
		let (text_color, outline_color, near_shadow_color, far_shadow_color) =
			Self::selection_size_badge_text_colors(theme);

		painter.text(
			Self::snap_pos_to_pixel_grid(
				text_anchor + Vec2::new(0.0, far_shadow_offset),
				pixels_per_point,
			),
			Align2::RIGHT_CENTER,
			text.clone(),
			font_id.clone(),
			far_shadow_color,
		);

		for offset in [
			Vec2::new(-outline_offset, 0.0),
			Vec2::new(outline_offset, 0.0),
			Vec2::new(0.0, -outline_offset),
			Vec2::new(0.0, outline_offset),
		] {
			painter.text(
				Self::snap_pos_to_pixel_grid(text_anchor + offset, pixels_per_point),
				Align2::RIGHT_CENTER,
				text.clone(),
				font_id.clone(),
				outline_color,
			);
		}

		painter.text(
			Self::snap_pos_to_pixel_grid(
				text_anchor + Vec2::new(near_shadow_offset, near_shadow_offset),
				pixels_per_point,
			),
			Align2::RIGHT_CENTER,
			text.clone(),
			font_id.clone(),
			near_shadow_color,
		);
		painter.text(text_anchor, Align2::RIGHT_CENTER, text, font_id, text_color);
	}

	pub(in crate::overlay) fn frozen_selection_scrim_rects(
		screen_rect: Rect,
		focus_rect: Rect,
	) -> [Rect; 4] {
		[
			Rect::from_min_max(screen_rect.min, Pos2::new(screen_rect.max.x, focus_rect.min.y)),
			Rect::from_min_max(Pos2::new(screen_rect.min.x, focus_rect.max.y), screen_rect.max),
			Rect::from_min_max(
				Pos2::new(screen_rect.min.x, focus_rect.min.y),
				Pos2::new(focus_rect.min.x, focus_rect.max.y),
			),
			Rect::from_min_max(
				Pos2::new(focus_rect.max.x, focus_rect.min.y),
				Pos2::new(screen_rect.max.x, focus_rect.max.y),
			),
		]
	}

	pub(in crate::overlay) fn frozen_selection_scrim_color(theme: HudTheme) -> Color32 {
		let alpha = match theme {
			HudTheme::Light => FROZEN_SELECTION_SCRIM_ALPHA_LIGHT,
			HudTheme::Dark => FROZEN_SELECTION_SCRIM_ALPHA_DARK,
		};

		Color32::from_rgba_unmultiplied(0, 0, 0, alpha)
	}

	pub(in crate::overlay) fn live_drag_selection_scrim_color(theme: HudTheme) -> Color32 {
		let alpha = match theme {
			HudTheme::Light => LIVE_DRAG_SELECTION_SCRIM_ALPHA_LIGHT,
			HudTheme::Dark => LIVE_DRAG_SELECTION_SCRIM_ALPHA_DARK,
		};

		Color32::from_rgba_unmultiplied(0, 0, 0, alpha)
	}

	pub(in crate::overlay) fn render_frozen_selection_scrim(
		painter: &Painter,
		focus_rect: Rect,
		screen_rect: Rect,
		theme: HudTheme,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		Self::render_selection_scrim(
			painter,
			focus_rect,
			screen_rect,
			Self::frozen_selection_scrim_color(theme),
			selection_dashed_border_cache,
		)
	}

	pub(in crate::overlay) fn render_live_drag_selection_scrim(
		painter: &Painter,
		focus_rect: Rect,
		screen_rect: Rect,
		theme: HudTheme,
	) -> bool {
		Self::render_selection_scrim_fill(
			painter,
			focus_rect,
			screen_rect,
			Self::live_drag_selection_scrim_color(theme),
		)
	}

	pub(in crate::overlay) fn render_selection_scrim(
		painter: &Painter,
		focus_rect: Rect,
		screen_rect: Rect,
		scrim_fill: Color32,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		let drew_scrim =
			Self::render_selection_scrim_fill(painter, focus_rect, screen_rect, scrim_fill);
		let drew_border = Self::render_selection_dashed_border(
			painter,
			focus_rect,
			screen_rect,
			selection_dashed_border_cache,
		);

		drew_scrim || drew_border
	}

	pub(in crate::overlay) fn render_selection_scrim_fill(
		painter: &Painter,
		focus_rect: Rect,
		screen_rect: Rect,
		scrim_fill: Color32,
	) -> bool {
		let scrim_rects = Self::frozen_selection_scrim_rects(screen_rect, focus_rect);
		let mut drew_scrim = false;

		for rect in scrim_rects {
			if rect.width() <= 0.0 || rect.height() <= 0.0 {
				continue;
			}

			painter.rect_filled(rect, 0.0, scrim_fill);

			drew_scrim = true;
		}

		drew_scrim
	}

	pub(in crate::overlay) fn render_selection_dashed_border(
		painter: &Painter,
		focus_rect: Rect,
		screen_rect: Rect,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		let metrics = Self::selection_dashed_border_metrics(painter.pixels_per_point());
		let border_outset =
			Self::selection_dashed_border_outset(metrics.stroke_width, painter.pixels_per_point());
		let Some(border_rect) =
			Self::selection_dashed_border_rect(screen_rect, focus_rect, border_outset)
		else {
			return false;
		};
		let segments = Self::selection_dashed_border_cached_segments(
			selection_dashed_border_cache,
			border_rect,
			metrics.dash_length,
			metrics.gap_length,
		);

		if segments.is_empty() {
			return false;
		}

		let stroke = Stroke::new(
			metrics.stroke_width,
			Color32::from_rgba_unmultiplied(255, 255, 255, SELECTION_DASHED_BORDER_ALPHA),
		);

		for segment in segments {
			painter.add(Shape::line_segment(*segment, stroke));
		}

		true
	}

	pub(in crate::overlay) fn selection_dashed_border_metrics(
		pixels_per_point: f32,
	) -> SelectionDashedBorderMetrics {
		let points_per_pixel = 1.0 / pixels_per_point.max(f32::MIN_POSITIVE);

		SelectionDashedBorderMetrics {
			stroke_width: SELECTION_DASHED_BORDER_WIDTH_PX * points_per_pixel,
			dash_length: SELECTION_DASHED_BORDER_DASH_LENGTH_PX * points_per_pixel,
			gap_length: SELECTION_DASHED_BORDER_GAP_LENGTH_PX * points_per_pixel,
		}
	}

	pub(in crate::overlay) fn selection_dashed_border_rect(
		screen_rect: Rect,
		focus_rect: Rect,
		border_outset: f32,
	) -> Option<Rect> {
		Self::selection_has_outside_region(screen_rect, focus_rect)
			.then_some(focus_rect.expand(border_outset))
	}

	pub(in crate::overlay) fn selection_dashed_border_outset(
		stroke_width: f32,
		pixels_per_point: f32,
	) -> f32 {
		let feathering = 1.0 / pixels_per_point.max(f32::MIN_POSITIVE);

		// Match epaint's outer stroke radius so the anti-aliased dashed keyline
		// stays fully in the scrim instead of bleeding into the capture rect.
		(stroke_width + feathering) * 0.5
	}

	pub(in crate::overlay) fn selection_has_outside_region(
		screen_rect: Rect,
		focus_rect: Rect,
	) -> bool {
		Self::frozen_selection_scrim_rects(screen_rect, focus_rect)
			.into_iter()
			.any(|rect| rect.width() > 0.0 && rect.height() > 0.0)
	}

	pub(in crate::overlay) fn selection_dashed_border_segments(
		rect: Rect,
		target_dash_length: f32,
		target_gap_length: f32,
	) -> Vec<[Pos2; 2]> {
		let perimeter = Self::selection_dashed_border_perimeter(rect);

		if perimeter <= 0.0 {
			return Vec::new();
		}

		let mut segments = Vec::new();

		for (dash_start, dash_end) in Self::selection_dashed_border_dash_ranges(
			perimeter,
			target_dash_length,
			target_gap_length,
		) {
			Self::append_selection_dashed_border_dash_segments(
				rect,
				dash_start,
				dash_end,
				&mut segments,
			);
		}

		segments
	}

	pub(in crate::overlay) fn selection_dashed_border_cached_segments(
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
		rect: Rect,
		target_dash_length: f32,
		target_gap_length: f32,
	) -> &[[Pos2; 2]] {
		let key = SelectionDashedBorderCacheKey::new(rect, target_dash_length, target_gap_length);

		if selection_dashed_border_cache.key != Some(key) {
			selection_dashed_border_cache.segments.clear();
			selection_dashed_border_cache.segments.extend(Self::selection_dashed_border_segments(
				rect,
				target_dash_length,
				target_gap_length,
			));

			selection_dashed_border_cache.key = Some(key);
		}

		selection_dashed_border_cache.segments.as_slice()
	}

	pub(in crate::overlay) fn selection_dashed_border_dash_ranges(
		perimeter: f32,
		target_dash_length: f32,
		target_gap_length: f32,
	) -> Vec<(f32, f32)> {
		if perimeter <= 0.0 {
			return Vec::new();
		}

		let target_cycle = (target_dash_length + target_gap_length).max(f32::MIN_POSITIVE);
		let cycle_count = (perimeter / target_cycle).round().max(1.0) as usize;
		let cycle_span = perimeter / cycle_count as f32;
		let dash_length = target_dash_length.min(cycle_span);

		(0..cycle_count)
			.map(|index| {
				let dash_start = index as f32 * cycle_span;

				(dash_start, dash_start + dash_length)
			})
			.collect()
	}

	pub(in crate::overlay) fn append_selection_dashed_border_dash_segments(
		rect: Rect,
		dash_start: f32,
		dash_end: f32,
		segments: &mut Vec<[Pos2; 2]>,
	) {
		let mut segment_start = dash_start;

		for corner_distance in Self::selection_dashed_border_corner_distances(rect) {
			if segment_start >= dash_end {
				break;
			}
			if corner_distance <= segment_start || corner_distance >= dash_end {
				continue;
			}

			Self::push_selection_dashed_border_segment(
				rect,
				segment_start,
				corner_distance,
				segments,
			);

			segment_start = corner_distance;
		}

		if segment_start < dash_end {
			Self::push_selection_dashed_border_segment(rect, segment_start, dash_end, segments);
		}
	}

	pub(in crate::overlay) fn push_selection_dashed_border_segment(
		rect: Rect,
		start_distance: f32,
		end_distance: f32,
		segments: &mut Vec<[Pos2; 2]>,
	) {
		let start = Self::selection_dashed_border_point_at(rect, start_distance);
		let end = Self::selection_dashed_border_point_at(rect, end_distance);

		if start != end {
			segments.push([start, end]);
		}
	}

	pub(in crate::overlay) fn selection_dashed_border_point_at(rect: Rect, distance: f32) -> Pos2 {
		let width = rect.width();
		let height = rect.height();
		let perimeter = Self::selection_dashed_border_perimeter(rect);
		let distance = distance.rem_euclid(perimeter);

		if distance < width {
			return Pos2::new(rect.min.x + distance, rect.min.y);
		}
		if distance < width + height {
			return Pos2::new(rect.max.x, rect.min.y + (distance - width));
		}
		if distance < width * 2.0 + height {
			return Pos2::new(rect.max.x - (distance - width - height), rect.max.y);
		}

		Pos2::new(rect.min.x, rect.max.y - (distance - width * 2.0 - height))
	}

	pub(in crate::overlay) fn selection_dashed_border_corner_distances(rect: Rect) -> [f32; 4] {
		let width = rect.width();
		let height = rect.height();

		[width, width + height, width * 2.0 + height, Self::selection_dashed_border_perimeter(rect)]
	}

	pub(in crate::overlay) fn selection_dashed_border_perimeter(rect: Rect) -> f32 {
		if rect.width() <= 0.0 || rect.height() <= 0.0 {
			return 0.0;
		}

		(rect.width() + rect.height()) * 2.0
	}

	pub(in crate::overlay) fn render_selection_flow_ring(
		painter: &Painter,
		rect: Rect,
		ctx: &Context,
		theme: HudTheme,
		style: SelectionFlowStyle,
		selection_flow_stroke_width_px: f32,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
	) {
		if rect.width() < LIVE_DRAG_START_THRESHOLD_PX
			|| rect.height() < LIVE_DRAG_START_THRESHOLD_PX
		{
			return;
		}

		let corner_radius = Self::selection_flow_corner_radius(rect);
		let perimeter = Self::selection_flow_perimeter(rect, corner_radius);
		let time = ctx.input(|i| i.time) as f32;
		let sample_count = Self::selection_flow_sample_count(perimeter);
		let seam_offset = if rect.width() > corner_radius * 2.0 {
			(rect.width() - corner_radius * 2.0) * 0.5
		} else {
			0.0
		};
		let (samples, normals) = Self::selection_flow_cached_geometry(
			selection_flow_geometry_cache,
			rect,
			corner_radius,
			sample_count,
			seam_offset,
		);
		let base_alpha_scale = 1.0;
		let stroke_width = selection_flow_stroke_width_px.clamp(1.0, 8.0);

		if samples.is_empty() {
			return;
		}

		let flow_time = time * SELECTION_FLOW_SPEED;
		let phase = flow_time * 1.28 + 0.72;

		match style {
			SelectionFlowStyle::Band => Self::selection_flow_draw_layer(
				painter,
				samples,
				normals,
				stroke_width,
				base_alpha_scale * 0.52,
				phase,
				SELECTION_FLOW_CORE_FLOW_WIDTH,
				theme,
			),
			SelectionFlowStyle::FullBorder => Self::selection_flow_draw_layer_full_border(
				painter,
				samples,
				normals,
				stroke_width,
				base_alpha_scale * SELECTION_FLOW_FROZEN_ALPHA_SCALE,
				phase,
				SELECTION_FLOW_FROZEN_INTENSITY,
				theme,
			),
		}
	}

	pub(in crate::overlay) fn selection_flow_corner_radius(rect: Rect) -> f32 {
		SELECTION_FLOW_CORNER_RADIUS_PX
			.min(rect.width() / 2.0 - 0.25)
			.min(rect.height() / 2.0 - 0.25)
			.max(0.0)
	}

	pub(in crate::overlay) fn selection_flow_palette(
		theme: HudTheme,
	) -> &'static [(u8, u8, u8); 3] {
		match theme {
			HudTheme::Dark => &SELECTION_FLOW_PALETTE,
			HudTheme::Light => &SELECTION_FLOW_LIGHT_PALETTE,
		}
	}

	pub(in crate::overlay) fn selection_flow_cached_geometry(
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		rect: Rect,
		corner_radius: f32,
		sample_count: usize,
		seam_offset: f32,
	) -> (&[(Pos2, f32)], &[Vec2]) {
		let key =
			SelectionFlowGeometryCacheKey::new(rect, corner_radius, seam_offset, sample_count);

		if selection_flow_geometry_cache.key == Some(key)
			&& !selection_flow_geometry_cache.samples.is_empty()
		{
			return (
				&selection_flow_geometry_cache.samples,
				&selection_flow_geometry_cache.normals,
			);
		}

		let samples =
			Self::selection_flow_path_samples(rect, corner_radius, sample_count, seam_offset);
		let normals = Self::selection_flow_compute_normals(&samples);

		selection_flow_geometry_cache.key = Some(key);
		selection_flow_geometry_cache.samples = samples;
		selection_flow_geometry_cache.normals = normals;

		(&selection_flow_geometry_cache.samples, &selection_flow_geometry_cache.normals)
	}

	pub(in crate::overlay) fn selection_flow_compute_normals(samples: &[(Pos2, f32)]) -> Vec<Vec2> {
		let n = samples.len();

		if n == 0 {
			return Vec::new();
		}

		let mut normals = Vec::with_capacity(n);
		let mut first_non_zero = None;

		for i in 0..n {
			let (current_point, _) = samples[i];
			let (prev_point, _) = samples[(i + n - 1) % n];
			let (next_point, _) = samples[(i + 1) % n];
			let prev_tangent = current_point - prev_point;
			let next_tangent = next_point - current_point;
			let mut normal = Vec2::ZERO;

			if prev_tangent.length_sq() > f32::EPSILON {
				let prev_len = prev_tangent.length();

				normal += Vec2::new(-prev_tangent.y / prev_len, prev_tangent.x / prev_len);
			}
			if next_tangent.length_sq() > f32::EPSILON {
				let next_len = next_tangent.length();

				normal += Vec2::new(-next_tangent.y / next_len, next_tangent.x / next_len);
			}
			if normal.length_sq() <= f32::EPSILON {
				if next_tangent.length_sq() > f32::EPSILON {
					let next_len = next_tangent.length();

					normal = Vec2::new(-next_tangent.y / next_len, next_tangent.x / next_len);
				} else if prev_tangent.length_sq() > f32::EPSILON {
					let prev_len = prev_tangent.length();

					normal = Vec2::new(-prev_tangent.y / prev_len, prev_tangent.x / prev_len);
				}
			}

			let normal = if normal.length_sq() > f32::EPSILON {
				let normalized = normal / normal.length();

				if first_non_zero.is_none() && normalized.length_sq() > f32::EPSILON {
					first_non_zero = Some(i);
				}

				normalized
			} else {
				Vec2::ZERO
			};

			normals.push(normal);
		}

		if let Some(first_idx) = first_non_zero {
			let mut previous = normals[first_idx];

			for normal in normals.iter_mut().skip(first_idx + 1) {
				if normal.length_sq() > f32::EPSILON && normal.dot(previous) < 0.0 {
					*normal = -*normal;
				}
				if normal.length_sq() > f32::EPSILON {
					previous = *normal;
				}
			}
			for normal in normals.iter_mut().take(first_idx).rev() {
				if normal.length_sq() > f32::EPSILON && normal.dot(previous) < 0.0 {
					*normal = -*normal;
				}
				if normal.length_sq() > f32::EPSILON {
					previous = *normal;
				}
			}

			if normals[first_idx].length_sq() > f32::EPSILON
				&& normals[(first_idx + n - 1) % n].length_sq() > f32::EPSILON
				&& normals[first_idx].dot(normals[(first_idx + n - 1) % n]) < 0.0
			{
				for normal in &mut normals {
					*normal = -*normal;
				}
			}
		}

		normals
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn selection_flow_draw_layer(
		painter: &Painter,
		samples: &[(Pos2, f32)],
		normals: &[Vec2],
		line_width: f32,
		alpha_scale: f32,
		phase: f32,
		flow_band_width: f32,
		theme: HudTheme,
	) {
		if samples.is_empty() || normals.is_empty() || samples.len() != normals.len() {
			return;
		}

		let half = (line_width * 0.5).max(0.1);
		let n = samples.len();
		let mut mesh = Mesh::default();

		for i in 0..n {
			let (current_point, t) = samples[i];
			let movement = Self::selection_flow_flow_band(t, phase, flow_band_width);
			let intensity = SELECTION_FLOW_FLOW_BOOST * movement;
			let color = Self::selection_flow_color(t + phase, theme, alpha_scale, intensity);
			let normal = normals[i] * half;

			mesh.colored_vertex(current_point + normal, color);
			mesh.colored_vertex(current_point - normal, color);
		}
		for i in 0..n {
			let i0 = (i * 2) as u32;
			let i1 = ((i * 2) + 1) as u32;
			let n0 = (((i + 1) % n) * 2) as u32;
			let n1 = (((i + 1) % n) * 2 + 1) as u32;

			mesh.add_triangle(i0, i1, n0);
			mesh.add_triangle(i1, n1, n0);
		}

		painter.add(Shape::Mesh(mesh.into()));
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn selection_flow_draw_layer_full_border(
		painter: &Painter,
		samples: &[(Pos2, f32)],
		normals: &[Vec2],
		line_width: f32,
		alpha_scale: f32,
		phase: f32,
		intensity: f32,
		theme: HudTheme,
	) {
		if samples.is_empty() || normals.is_empty() || samples.len() != normals.len() {
			return;
		}

		let half = (line_width * 0.5).max(0.1);
		let n = samples.len();
		let mut mesh = Mesh::default();

		for i in 0..n {
			let (current_point, t) = samples[i];
			let color = Self::selection_flow_color(t + phase, theme, alpha_scale, intensity);
			let normal = normals[i] * half;

			mesh.colored_vertex(current_point + normal, color);
			mesh.colored_vertex(current_point - normal, color);
		}
		for i in 0..n {
			let i0 = (i * 2) as u32;
			let i1 = ((i * 2) + 1) as u32;
			let n0 = (((i + 1) % n) * 2) as u32;
			let n1 = (((i + 1) % n) * 2 + 1) as u32;

			mesh.add_triangle(i0, i1, n0);
			mesh.add_triangle(i1, n1, n0);
		}

		painter.add(Shape::Mesh(mesh.into()));
	}

	pub(in crate::overlay) fn selection_flow_flow_band(
		progress: f32,
		phase: f32,
		band_width: f32,
	) -> f32 {
		let width = band_width.clamp(0.001, 0.5);
		let distance = (progress - phase).rem_euclid(1.0);
		let distance = distance.min(1.0 - distance);
		let normalized = (distance / width).min(1.0);

		(1.0 - normalized).powf(2.0)
	}

	pub(in crate::overlay) fn selection_flow_sample_count(perimeter: f32) -> usize {
		if perimeter <= 0.0 || !perimeter.is_finite() {
			return SELECTION_FLOW_MIN_SEGMENTS;
		}

		let by_step = (perimeter / SELECTION_FLOW_SAMPLE_STEP_PX).ceil() as usize;

		by_step.clamp(SELECTION_FLOW_MIN_SEGMENTS, SELECTION_FLOW_MAX_SEGMENTS)
	}

	pub(in crate::overlay) fn selection_flow_path_samples(
		rect: Rect,
		corner_radius: f32,
		sample_count: usize,
		start_offset: f32,
	) -> Vec<(Pos2, f32)> {
		let perimeter = Self::selection_flow_perimeter(rect, corner_radius);

		if perimeter <= 0.0 {
			return Vec::new();
		}

		let start = (start_offset / perimeter).rem_euclid(1.0);

		(0..sample_count)
			.map(|index| {
				let t = (index as f32 + 0.5) / sample_count as f32;
				let progress = (t + start).rem_euclid(1.0);

				(
					Self::selection_flow_sample_at_distance(
						rect,
						corner_radius,
						perimeter * progress,
					),
					t,
				)
			})
			.collect()
	}

	pub(in crate::overlay) fn selection_flow_sample_at_distance(
		rect: Rect,
		corner_radius: f32,
		distance: f32,
	) -> Pos2 {
		if corner_radius <= f32::EPSILON {
			let perimeter = Self::selection_flow_perimeter(rect, 0.0);
			let keep = distance.rem_euclid(perimeter);
			let edge_top = rect.width();
			let edge_right = rect.height();

			if keep < edge_top {
				return Pos2::new(rect.min.x + keep, rect.min.y);
			}
			if keep < edge_top + edge_right {
				return Pos2::new(rect.max.x, rect.min.y + (keep - edge_top));
			}
			if keep < edge_top * 2.0 + edge_right {
				return Pos2::new(rect.max.x - (keep - edge_top - edge_right), rect.max.y);
			}

			return Pos2::new(rect.min.x, rect.max.y - (keep - edge_top * 2.0 - edge_right));
		}

		let x0 = rect.min.x;
		let x1 = rect.max.x;
		let y0 = rect.min.y;
		let y1 = rect.max.y;
		let perimeter = Self::selection_flow_perimeter(rect, corner_radius);
		let remain = distance.rem_euclid(perimeter);
		let edge_top_len = (rect.width() - corner_radius * 2.0).max(0.0);
		let edge_right_len = (rect.height() - corner_radius * 2.0).max(0.0);
		let corner_len = std::f32::consts::FRAC_PI_2 * corner_radius;

		if remain < edge_top_len {
			return Pos2::new(x0 + corner_radius + remain, y0);
		}

		let mut offset = remain - edge_top_len;

		if offset < corner_len {
			let angle = -std::f32::consts::FRAC_PI_2 + offset / corner_radius;

			return Pos2::new(
				x1 - corner_radius + corner_radius * angle.cos(),
				y0 + corner_radius + corner_radius * angle.sin(),
			);
		}

		offset -= corner_len;

		if offset < edge_right_len {
			return Pos2::new(x1, y0 + corner_radius + offset);
		}

		offset -= edge_right_len;

		if offset < corner_len {
			let angle = offset / corner_radius;

			return Pos2::new(
				x1 - corner_radius + corner_radius * angle.cos(),
				y1 - corner_radius + corner_radius * angle.sin(),
			);
		}

		offset -= corner_len;

		if offset < edge_top_len {
			return Pos2::new(x1 - corner_radius - offset, y1);
		}

		offset -= edge_top_len;

		if offset < corner_len {
			let angle = std::f32::consts::FRAC_PI_2 + offset / corner_radius;

			return Pos2::new(
				x0 + corner_radius + corner_radius * angle.cos(),
				y1 - corner_radius + corner_radius * angle.sin(),
			);
		}

		offset -= corner_len;

		if offset < edge_right_len {
			return Pos2::new(x0, y1 - corner_radius - offset);
		}

		offset -= edge_right_len;

		if offset < corner_len {
			let angle = std::f32::consts::PI + offset / corner_radius;

			return Pos2::new(
				x0 + corner_radius + corner_radius * angle.cos(),
				y0 + corner_radius + corner_radius * angle.sin(),
			);
		}

		Pos2::new(x0 + corner_radius, y0)
	}

	pub(in crate::overlay) fn selection_flow_perimeter(rect: Rect, corner_radius: f32) -> f32 {
		let edge_top_len = (rect.width() - corner_radius * 2.0).max(0.0);
		let edge_right_len = (rect.height() - corner_radius * 2.0).max(0.0);
		let corner_len = std::f32::consts::FRAC_PI_2 * corner_radius;

		2.0 * (edge_top_len + edge_right_len) + 4.0 * corner_len
	}

	pub(in crate::overlay) fn selection_flow_color(
		progress: f32,
		theme: HudTheme,
		alpha_scale: f32,
		intensity: f32,
	) -> Color32 {
		let palette = Self::selection_flow_palette(theme);
		let normalized = progress.rem_euclid(1.0);
		let band_position = normalized * palette.len() as f32;
		let band = band_position.floor() as usize % palette.len();
		let local = band_position - band as f32;
		let (r0, g0, b0) = palette[band];
		let (r1, g1, b1) = palette[(band + 1) % palette.len()];
		let blend = |a: u8, b: u8, ratio: f32| -> u8 {
			(a as f32 + (b as f32 - a as f32) * ratio).clamp(0.0, 255.0).round() as u8
		};
		let theme_alpha = 1.0;
		let alpha = (255.0 * alpha_scale * intensity * theme_alpha).clamp(0.0, 255.0);

		Color32::from_rgba_unmultiplied(
			blend(r0, r1, local),
			blend(g0, g1, local),
			blend(b0, b1, local),
			alpha as u8,
		)
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn render_frozen_toolbar_ui(
		ctx: &Context,
		state: &OverlayState,
		monitor: MonitorRect,
		theme: HudTheme,
		toolbar_placement: ToolbarPlacement,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		toolbar_state: Option<&mut FrozenToolbarState>,
		pointer_state: Option<FrozenToolbarPointerState>,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		let Some(toolbar_state) = toolbar_state else {
			return;
		};

		if !matches!(state.mode, OverlayMode::Frozen) || !toolbar_state.visible {
			return;
		}
		if state.monitor != Some(monitor) {
			return;
		}

		let (cursor, left_button_down) = if let Some(pointer_state) = pointer_state {
			(pointer_state.cursor_local, pointer_state.left_button_down)
		} else {
			toolbar_state.dragging = false;

			(Pos2::new(-1.0, -1.0), false)
		};
		let toolbar_size = Self::frozen_toolbar_size(toolbar_state);
		let screen_rect = ctx.input(|i| i.viewport_rect());
		let capture_rect = Self::frozen_toolbar_capture_rect(state, monitor, screen_rect);
		let Some(toolbar_pos) = Self::resolve_frozen_toolbar_birth(
			ctx,
			state,
			monitor,
			toolbar_state,
			screen_rect,
			capture_rect,
			toolbar_size,
			toolbar_placement,
		) else {
			return;
		};

		#[cfg(any(not(target_os = "macos"), test))]
		{
			if !overlay::advance_frozen_toolbar_readiness_sample_state(toolbar_state, screen_rect) {
				ctx.request_repaint();

				return;
			}
		}

		Self::draw_frozen_toolbar(
			ctx,
			toolbar_state,
			monitor,
			screen_rect,
			toolbar_pos,
			toolbar_size,
			theme,
			hud_blur_active,
			hud_opaque,
			hud_opacity,
			hud_milk_amount,
			hud_tint_hue,
			cursor,
			left_button_down,
			hud_pill_out,
		);
	}

	pub(in crate::overlay) fn frozen_toolbar_tools(
		toolbar_state: &FrozenToolbarState,
	) -> &'static [FrozenToolbarTool] {
		#[cfg(target_os = "macos")]
		const TOOLS_SCROLL_MODE: [FrozenToolbarTool; 3] =
			[FrozenToolbarTool::Ocr, FrozenToolbarTool::Copy, FrozenToolbarTool::Save];
		#[cfg(not(target_os = "macos"))]
		const TOOLS_SCROLL_MODE: [FrozenToolbarTool; 2] =
			[FrozenToolbarTool::Copy, FrozenToolbarTool::Save];
		#[cfg(target_os = "macos")]
		const TOOLS_WITH_SCROLL_AND_AUTO_CENTER: [FrozenToolbarTool; 11] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::AutoCenter,
			FrozenToolbarTool::Scroll,
			FrozenToolbarTool::Ocr,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(not(target_os = "macos"))]
		const TOOLS_WITH_SCROLL_AND_AUTO_CENTER: [FrozenToolbarTool; 10] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::AutoCenter,
			FrozenToolbarTool::Scroll,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(target_os = "macos")]
		const TOOLS_WITH_AUTO_CENTER: [FrozenToolbarTool; 10] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::AutoCenter,
			FrozenToolbarTool::Ocr,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(not(target_os = "macos"))]
		const TOOLS_WITH_AUTO_CENTER: [FrozenToolbarTool; 9] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::AutoCenter,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(target_os = "macos")]
		const TOOLS_WITH_SCROLL: [FrozenToolbarTool; 10] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::Scroll,
			FrozenToolbarTool::Ocr,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(not(target_os = "macos"))]
		const TOOLS_WITH_SCROLL: [FrozenToolbarTool; 9] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::Scroll,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(target_os = "macos")]
		const TOOLS_WITHOUT_SCROLL: [FrozenToolbarTool; 9] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::Ocr,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(not(target_os = "macos"))]
		const TOOLS_WITHOUT_SCROLL: [FrozenToolbarTool; 8] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];

		if toolbar_state.scroll_capture_active {
			&TOOLS_SCROLL_MODE
		} else if toolbar_state.auto_center_available && toolbar_state.scroll_capture_available {
			&TOOLS_WITH_SCROLL_AND_AUTO_CENTER
		} else if toolbar_state.auto_center_available {
			&TOOLS_WITH_AUTO_CENTER
		} else if toolbar_state.scroll_capture_available {
			&TOOLS_WITH_SCROLL
		} else {
			&TOOLS_WITHOUT_SCROLL
		}
	}

	pub(in crate::overlay) fn frozen_toolbar_size(toolbar_state: &FrozenToolbarState) -> Vec2 {
		let tool_count = Self::frozen_toolbar_tools(toolbar_state).len() as f32;
		let spacing_count = (tool_count - 1.0).max(0.0);
		let width = tool_count * FROZEN_TOOLBAR_BUTTON_SIZE_POINTS
			+ spacing_count * FROZEN_TOOLBAR_ITEM_SPACING_POINTS
			+ 2.0 * HUD_PILL_INNER_MARGIN_X_POINTS
			+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
		let height = toolbar_state.pill_height_points.unwrap_or(TOOLBAR_EXPANDED_HEIGHT_PX);

		Vec2::new(width, height)
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn resolve_frozen_toolbar_birth(
		ctx: &Context,
		state: &OverlayState,
		monitor: MonitorRect,
		toolbar_state: &mut FrozenToolbarState,
		screen_rect: Rect,
		capture_rect: Rect,
		toolbar_size: Vec2,
		toolbar_placement: ToolbarPlacement,
	) -> Option<Pos2> {
		if let Some(pos) = toolbar_state.floating_position {
			return Some(pos);
		}

		let screen_size_points = screen_rect.size();

		tracing::trace!(
			monitor_id = monitor.id,
			frozen_generation = state.frozen_generation,
			screen_rect = ?screen_rect,
			screen_size_points = ?screen_size_points,
			pixels_per_point = ctx.pixels_per_point(),
			last_screen_size_points = ?toolbar_state.layout_last_screen_size_points,
			stable_frames = toolbar_state.layout_stable_frames,
			"Frozen toolbar birth attempt."
		);

		let needs_new_sample = overlay::frozen_toolbar_needs_new_sample(
			toolbar_state.layout_last_screen_size_points,
			screen_size_points,
		);

		if needs_new_sample {
			toolbar_state.layout_last_screen_size_points = Some(screen_size_points);
			toolbar_state.layout_stable_frames = 0;
			toolbar_state.needs_redraw = true;

			tracing::debug!(
				monitor_id = monitor.id,
				frozen_generation = state.frozen_generation,
				new_screen_size_points = ?screen_size_points,
				"Frozen toolbar waiting for stable screen rect (new sample)."
			);

			ctx.request_repaint();

			return None;
		}
		if toolbar_state.layout_stable_frames < 1 {
			toolbar_state.layout_stable_frames =
				toolbar_state.layout_stable_frames.saturating_add(1);
			toolbar_state.needs_redraw = true;

			tracing::debug!(
				monitor_id = monitor.id,
				frozen_generation = state.frozen_generation,
				screen_size_points = ?screen_size_points,
				stable_frames = toolbar_state.layout_stable_frames,
				"Frozen toolbar waiting for stable screen rect (confirm)."
			);

			ctx.request_repaint();

			return None;
		}

		let default_pos = Self::frozen_toolbar_default_pos(
			screen_rect,
			capture_rect,
			toolbar_size,
			toolbar_placement,
		);

		tracing::debug!(
			monitor_id = monitor.id,
			frozen_generation = state.frozen_generation,
			toolbar_size_points = ?toolbar_size,
			default_pos = ?default_pos,
			"Frozen toolbar birth resolved."
		);

		toolbar_state.default_slot_position = Some(default_pos);
		toolbar_state.floating_position = Some(default_pos);

		Some(default_pos)
	}

	pub(in crate::overlay) fn frozen_toolbar_capture_rect(
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
	) -> Rect {
		let Some(capture_rect) = state.frozen_capture_rect else {
			return screen_rect;
		};
		let Some(frozen_monitor) = state.monitor else {
			return screen_rect;
		};

		if frozen_monitor != monitor {
			return screen_rect;
		}

		let capture_rect = Rect::from_min_size(
			Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
			Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
		);

		capture_rect.intersect(screen_rect)
	}

	pub(in crate::overlay) fn frozen_toolbar_default_pos(
		screen_rect: Rect,
		capture_rect: Rect,
		toolbar_size: Vec2,
		toolbar_placement: ToolbarPlacement,
	) -> Pos2 {
		let y = match toolbar_placement {
			ToolbarPlacement::Bottom => {
				let below_y = capture_rect.max.y + TOOLBAR_CAPTURE_GAP_PX;
				let within_screen =
					below_y + toolbar_size.y + TOOLBAR_SCREEN_MARGIN_PX <= screen_rect.max.y;

				if within_screen {
					below_y
				} else {
					capture_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - toolbar_size.y
				}
			},
			ToolbarPlacement::Top => {
				let above_y = capture_rect.min.y - TOOLBAR_CAPTURE_GAP_PX - toolbar_size.y;
				let within_screen = above_y >= screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;

				if within_screen { above_y } else { capture_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX }
			},
		};
		let min_y = screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;
		let max_y = (screen_rect.max.y - toolbar_size.y - TOOLBAR_SCREEN_MARGIN_PX).max(min_y);
		let x = Self::frozen_toolbar_default_x(screen_rect, toolbar_size, capture_rect.center().x);
		let y = y.max(min_y).min(max_y);

		Pos2::new(x, y)
	}

	pub(in crate::overlay) fn frozen_toolbar_default_x(
		screen_rect: Rect,
		toolbar_size: Vec2,
		anchor_center_x: f32,
	) -> f32 {
		let min_x = screen_rect.min.x + TOOLBAR_SCREEN_MARGIN_PX;
		let max_x = (screen_rect.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX).max(min_x);

		(anchor_center_x - toolbar_size.x / 2.0).clamp(min_x, max_x)
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn draw_frozen_toolbar(
		ctx: &Context,
		toolbar_state: &mut FrozenToolbarState,
		monitor: MonitorRect,
		screen_rect: Rect,
		toolbar_pos: Pos2,
		toolbar_size: Vec2,
		theme: HudTheme,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		cursor: Pos2,
		left_button_down: bool,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		Area::new(Id::new(format!("frozen-toolbar-{}", monitor.id)))
			.order(Order::Foreground)
			.fixed_pos(toolbar_pos)
			.show(ctx, |ui| {
				let (rect, response) =
					ui.allocate_exact_size(toolbar_size, Sense::click_and_drag());
				let body_fill = Self::tinted_hud_body_fill(
					theme,
					hud_blur_active,
					hud_opaque,
					hud_opacity,
					hud_milk_amount,
					hud_tint_hue,
				);
				let toolbar_frame =
					Self::hud_pill_frame(theme, hud_opaque, hud_opacity, body_fill, false);

				if response.drag_started() {
					toolbar_state.dragging = true;
					toolbar_state.floating_position = Some(toolbar_pos);
					toolbar_state.drag_offset = cursor - toolbar_pos;
				}
				if toolbar_state.dragging && left_button_down {
					let desired_pos = cursor - toolbar_state.drag_offset;

					toolbar_state.floating_position = Some(Self::clamp_toolbar_position(
						screen_rect,
						toolbar_size,
						desired_pos,
						TOOLBAR_SCREEN_MARGIN_PX,
						TOOLBAR_SCREEN_MARGIN_PX,
					));
				} else if toolbar_state.dragging {
					toolbar_state.dragging = false;
				}

				// Draw the capsule ourselves at the exact allocated rect. This keeps the visible pill
				// and the blur rect perfectly aligned (no shrink-to-content surprises on first frame).
				ui.painter().rect_filled(
					rect,
					f32::from(HUD_PILL_CORNER_RADIUS_POINTS),
					toolbar_frame.fill,
				);
				ui.painter().rect_stroke(
					rect.shrink(0.5),
					CornerRadius::same(HUD_PILL_CORNER_RADIUS_POINTS),
					toolbar_frame.stroke,
					StrokeKind::Inside,
				);

				let inner_stroke_color = match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
					HudTheme::Light => Color32::from_rgba_unmultiplied(255, 255, 255, 140),
				};
				let inner_stroke = Stroke::new(1.0, inner_stroke_color);
				let inner_rect = rect.shrink(1.0);

				ui.painter().rect_stroke(
					inner_rect,
					CornerRadius::same(HUD_PILL_CORNER_RADIUS_POINTS.saturating_sub(1)),
					inner_stroke,
					StrokeKind::Inside,
				);

				let inner_rect = rect.shrink2(egui::vec2(
					HUD_PILL_INNER_MARGIN_X_POINTS,
					HUD_PILL_INNER_MARGIN_Y_POINTS,
				));
				let _ = ui.scope_builder(UiBuilder::new().max_rect(inner_rect), |ui| {
					ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
						ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);

						Self::render_frozen_toolbar_controls(ui, toolbar_state, theme);
					});
				});

				*hud_pill_out = Some(HudPillGeometry {
					rect,
					radius_points: f32::from(HUD_PILL_CORNER_RADIUS_POINTS),
				});
			});
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn render_frozen_toolbar_controls(
		ui: &mut Ui,
		toolbar_state: &mut FrozenToolbarState,
		theme: HudTheme,
	) {
		if toolbar_state.selected_tool == FrozenToolbarTool::Scroll {
			toolbar_state.selected_tool = FrozenToolbarTool::Pointer;
		}

		let tools = Self::frozen_toolbar_tools(toolbar_state);
		let button_size = FROZEN_TOOLBAR_BUTTON_SIZE_POINTS;
		let button_font_size = 18.0;
		let item_spacing = FROZEN_TOOLBAR_ITEM_SPACING_POINTS;
		let hit_area_inset = 5.0;

		ui.horizontal_centered(|ui| {
			ui.spacing_mut().item_spacing.x = item_spacing;

			for tool in tools {
				let is_mode_tool = tool.is_mode_tool();
				let action_ready =
					!tool.requires_final_capture() || toolbar_state.final_capture_ready;
				let response =
					ui.allocate_response(Vec2::new(button_size, button_size), Sense::click());
				let hovered = action_ready && response.hovered();
				let response = if action_ready {
					response.on_hover_text(tool.label())
				} else {
					response.on_hover_text("Preparing capture...")
				};
				let hover_anim: f32 = if hovered { 1.0 } else { 0.0 };

				if action_ready && response.clicked() {
					let tool = *tool;

					if is_mode_tool {
						toolbar_state.selected_tool = tool;
					} else {
						toolbar_state.pending_action = Some(tool);
					}

					toolbar_state.needs_redraw = true;
				}

				let selected = is_mode_tool && *tool == toolbar_state.selected_tool;
				let selected_anim: f32 = if selected { 1.0 } else { 0.0 };
				let glow = hover_anim.max(selected_anim);
				let icon_font = if selected {
					FontFamily::Name("phosphor-fill".into())
				} else {
					FontFamily::Proportional
				};
				let style =
					Self::frozen_toolbar_button_style(theme, action_ready, hovered, selected);

				if glow > 0.0 {
					let bg_rect = response.rect.shrink(hit_area_inset);

					ui.painter().rect_filled(bg_rect, 8.0, style.bg_color);
				}

				if let Some(border_color) = style.border_color {
					ui.painter().rect_stroke(
						response.rect.shrink(hit_area_inset),
						8.0,
						Stroke::new(1.0, border_color),
						StrokeKind::Inside,
					);
				}

				ui.painter().text(
					response.rect.center(),
					Align2::CENTER_CENTER,
					tool.icon(),
					FontId::new(button_font_size, icon_font),
					style.icon_color,
				);
			}
		});
	}

	pub(in crate::overlay) fn frozen_toolbar_button_style(
		theme: HudTheme,
		action_ready: bool,
		hovered: bool,
		selected: bool,
	) -> FrozenToolbarButtonStyle {
		let hover_anim = if hovered { 1.0 } else { 0.0 };
		let selected_anim = if selected { 1.0 } else { 0.0 };
		let (normal_color, hover_color, selected_color, hover_bg, selected_bg) =
			Self::frozen_toolbar_colors(theme);
		let mut icon_color = if action_ready {
			normal_color
		} else {
			Color32::from_rgba_unmultiplied(
				normal_color.r(),
				normal_color.g(),
				normal_color.b(),
				(normal_color.a() as f32 * 0.45).round() as u8,
			)
		};
		let mut bg_color = Color32::from_rgba_unmultiplied(255, 255, 255, 0);

		if selected_anim > 0.0 {
			icon_color = Self::blend_color(icon_color, selected_color, selected_anim);
			bg_color = Self::blend_color(bg_color, selected_bg, selected_anim);
		}
		if hover_anim > 0.0 {
			icon_color = Self::blend_color(icon_color, hover_color, hover_anim);
			bg_color = Self::blend_color(bg_color, hover_bg, hover_anim * (1.0 - selected_anim));
		}

		FrozenToolbarButtonStyle { icon_color, bg_color, border_color: None }
	}

	pub(in crate::overlay) fn frozen_toolbar_colors(
		theme: HudTheme,
	) -> (Color32, Color32, Color32, Color32, Color32) {
		let (normal_color, hover_color, selected_color) = match theme {
			HudTheme::Dark => (
				Color32::from_rgba_unmultiplied(255, 255, 255, 160),
				Color32::from_rgba_unmultiplied(255, 255, 255, 222),
				Color32::from_rgba_unmultiplied(255, 255, 255, 255),
			),
			HudTheme::Light => (
				Color32::from_rgba_unmultiplied(28, 28, 32, 182),
				Color32::from_rgba_unmultiplied(28, 28, 32, 220),
				Color32::from_rgba_unmultiplied(28, 28, 32, 255),
			),
		};
		let hover_bg = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 20),
			HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 20),
		};
		let selected_bg = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 28),
			HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 24),
		};

		(normal_color, hover_color, selected_color, hover_bg, selected_bg)
	}

	pub(in crate::overlay) fn blend_color(a: Color32, b: Color32, t: f32) -> Color32 {
		let t = t.clamp(0.0, 1.0);
		let u = 1.0 - t;

		Color32::from_rgba_unmultiplied(
			((f32::from(a.r()) * u + f32::from(b.r()) * t).round().clamp(0.0, 255.0)) as u8,
			((f32::from(a.g()) * u + f32::from(b.g()) * t).round().clamp(0.0, 255.0)) as u8,
			((f32::from(a.b()) * u + f32::from(b.b()) * t).round().clamp(0.0, 255.0)) as u8,
			((f32::from(a.a()) * u + f32::from(b.a()) * t).round().clamp(0.0, 255.0)) as u8,
		)
	}

	pub(in crate::overlay) fn clamp_toolbar_position(
		screen_rect: Rect,
		toolbar_size: Vec2,
		cursor: Pos2,
		side_margin: f32,
		top_margin: f32,
	) -> Pos2 {
		let min_x = screen_rect.min.x + side_margin;
		let min_y = screen_rect.min.y + top_margin;
		let max_x = (screen_rect.max.x - toolbar_size.x - side_margin).max(min_x);
		let max_y = (screen_rect.max.y - toolbar_size.y - top_margin * 0.5).max(min_y);

		Pos2::new(cursor.x.clamp(min_x, max_x.max(min_x)), cursor.y.clamp(min_y, max_y.max(min_y)))
	}
}
