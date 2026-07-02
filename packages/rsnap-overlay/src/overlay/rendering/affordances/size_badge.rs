use egui::Context;

use crate::overlay::rendering::{
	SelectionSizeBadgeLayout, SelectionSizeBadgePadding, SelectionSizeBadgeTarget, WindowRenderer,
};
use crate::overlay::{
	Align2, Color32, FontFamily, FontId, HudTheme, MonitorRect, OverlayState, Painter, Pos2, Rect,
	RectPoints, Vec2,
};

pub(in crate::overlay) const SELECTION_SIZE_BADGE_GAP_PX: f32 = 8.0;
pub(in crate::overlay) const SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX: f32 = 8.0;
pub(in crate::overlay) const SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX: f32 = 8.0;

const SELECTION_SIZE_BADGE_FONT_SIZE_POINTS: f32 = 13.0;
const SELECTION_SIZE_BADGE_TEXT_OUTSET_POINTS: f32 = 2.0;
const SELECTION_SIZE_BADGE_OUTLINE_OFFSET_PX: f32 = 1.0;
const SELECTION_SIZE_BADGE_NEAR_SHADOW_OFFSET_PX: f32 = 1.0;
const SELECTION_SIZE_BADGE_FAR_SHADOW_OFFSET_PX: f32 = 2.0;

impl WindowRenderer {
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
}
