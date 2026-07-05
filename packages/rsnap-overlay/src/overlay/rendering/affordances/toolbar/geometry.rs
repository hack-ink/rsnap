use egui::Context;

use crate::overlay::hud_pill_style::{
	HUD_PILL_INNER_MARGIN_X_POINTS, HUD_PILL_STROKE_WIDTH_POINTS,
};
use crate::overlay::rendering::WindowRenderer;
use crate::overlay::rendering::affordances::toolbar::annotation_style::{
	FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS, FROZEN_ANNOTATION_TOOLBAR_SWATCH_GAP_POINTS,
	FROZEN_ANNOTATION_TOOLBAR_SWATCH_SIZE_POINTS, FrozenAnnotationStyleToolbarKind,
};
use crate::overlay::session_state::FrozenAnnotationStyleCapsulePlacement;
use crate::overlay::toolbar_geometry::{
	FROZEN_TOOLBAR_BUTTON_SIZE_POINTS, FROZEN_TOOLBAR_ITEM_SPACING_POINTS, TOOLBAR_CAPTURE_GAP_PX,
	TOOLBAR_EXPANDED_HEIGHT_PX, TOOLBAR_SCREEN_MARGIN_PX,
};
use crate::overlay::{
	FrozenAnnotationColor, FrozenToolbarState, FrozenToolbarTool, MonitorRect, OverlayMode,
	OverlayState, Pos2, Rect, ToolbarPlacement, Vec2, toolbar_layout_model,
};

impl WindowRenderer {
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
		let toolbar_primary_size = Self::frozen_toolbar_primary_size(toolbar_state);
		let toolbar_positioning_size = Self::frozen_toolbar_positioning_size(toolbar_state);
		let default_pos = Self::frozen_toolbar_default_window_pos(
			screen_rect,
			capture_rect,
			toolbar_primary_size,
			toolbar_positioning_size,
			toolbar_placement,
		);
		let toolbar_pos = toolbar_state.floating_position.unwrap_or(default_pos);

		if !toolbar_layout_model::frozen_toolbar_matches_default_slot(toolbar_pos, default_pos) {
			return None;
		}

		let mut reserved_toolbar_state = toolbar_state.clone();

		Self::sync_frozen_annotation_style_capsule_placement(
			&mut reserved_toolbar_state,
			screen_rect,
			toolbar_pos,
		);

		Some(Self::frozen_toolbar_window_rect(&reserved_toolbar_state, toolbar_pos))
	}

	pub(in crate::overlay) fn frozen_toolbar_primary_size(
		toolbar_state: &FrozenToolbarState,
	) -> Vec2 {
		let tool_count = Self::frozen_toolbar_tools(toolbar_state).len() as f32;
		let spacing_count = (tool_count - 1.0).max(0.0);
		let width = tool_count * FROZEN_TOOLBAR_BUTTON_SIZE_POINTS
			+ spacing_count * FROZEN_TOOLBAR_ITEM_SPACING_POINTS
			+ 2.0 * f32::from(HUD_PILL_INNER_MARGIN_X_POINTS)
			+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
		let height = toolbar_state.pill_height_points.unwrap_or(TOOLBAR_EXPANDED_HEIGHT_PX);

		Vec2::new(width, height)
	}

	pub(in crate::overlay) fn frozen_toolbar_primary_rect(
		toolbar_state: &FrozenToolbarState,
		toolbar_pos: Pos2,
	) -> Rect {
		Rect::from_min_size(toolbar_pos, Self::frozen_toolbar_primary_size(toolbar_state))
	}

	pub(in crate::overlay) fn frozen_annotation_style_capsule_size(
		toolbar_state: &FrozenToolbarState,
	) -> Option<Vec2> {
		let style_kind = FrozenAnnotationStyleToolbarKind::from_toolbar_state(toolbar_state)?;
		let swatch_count = FrozenAnnotationColor::ALL.len() as f32;
		let swatches_width = swatch_count * FROZEN_ANNOTATION_TOOLBAR_SWATCH_SIZE_POINTS
			+ (swatch_count - 1.0).max(0.0) * FROZEN_ANNOTATION_TOOLBAR_SWATCH_GAP_POINTS;
		let content_width = style_kind.size_control_width() + 4.0 + swatches_width;
		let width = content_width
			+ 2.0 * f32::from(HUD_PILL_INNER_MARGIN_X_POINTS)
			+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
		let height = toolbar_state.pill_height_points.unwrap_or(TOOLBAR_EXPANDED_HEIGHT_PX);

		Some(Vec2::new(width, height))
	}

	pub(in crate::overlay) fn frozen_toolbar_positioning_size(
		toolbar_state: &FrozenToolbarState,
	) -> Vec2 {
		Self::frozen_toolbar_primary_size(toolbar_state)
	}

	pub(in crate::overlay) fn frozen_toolbar_window_top_padding_points() -> f32 {
		[
			FrozenToolbarState {
				selected_tool: FrozenToolbarTool::Pen,
				..FrozenToolbarState::default()
			},
			FrozenToolbarState {
				selected_tool: FrozenToolbarTool::Text,
				..FrozenToolbarState::default()
			},
		]
		.into_iter()
		.map(|toolbar_state| {
			Self::frozen_annotation_style_capsule_size(&toolbar_state).map_or(0.0, |style_size| {
				style_size.y + FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS
			})
		})
		.fold(0.0, f32::max)
	}

	fn frozen_annotation_style_capsule_placement_for_toolbar_pos(
		toolbar_state: &FrozenToolbarState,
		screen_rect: Rect,
		toolbar_pos: Pos2,
	) -> FrozenAnnotationStyleCapsulePlacement {
		let Some(style_size) = Self::frozen_annotation_style_capsule_size(toolbar_state) else {
			return FrozenAnnotationStyleCapsulePlacement::Below;
		};
		let toolbar_rect = Self::frozen_toolbar_primary_rect(toolbar_state, toolbar_pos);
		let below_y = toolbar_rect.max.y + FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS;
		let above_y =
			toolbar_rect.min.y - FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS - style_size.y;
		let fits_below = below_y + style_size.y + TOOLBAR_SCREEN_MARGIN_PX <= screen_rect.max.y;
		let fits_above = above_y >= screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;

		if fits_below {
			FrozenAnnotationStyleCapsulePlacement::Below
		} else if fits_above {
			FrozenAnnotationStyleCapsulePlacement::Above
		} else {
			let below_space = screen_rect.max.y - below_y;
			let above_space = toolbar_rect.min.y
				- FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS
				- screen_rect.min.y;

			if above_space > below_space {
				FrozenAnnotationStyleCapsulePlacement::Above
			} else {
				FrozenAnnotationStyleCapsulePlacement::Below
			}
		}
	}

	pub(in crate::overlay) fn sync_frozen_annotation_style_capsule_placement(
		toolbar_state: &mut FrozenToolbarState,
		screen_rect: Rect,
		toolbar_pos: Pos2,
	) {
		toolbar_state.annotation_style_capsule_placement =
			Self::frozen_annotation_style_capsule_placement_for_toolbar_pos(
				toolbar_state,
				screen_rect,
				toolbar_pos,
			);
	}

	pub(super) fn frozen_annotation_style_capsule_rect(
		toolbar_state: &FrozenToolbarState,
		toolbar_rect: Rect,
	) -> Option<Rect> {
		let style_size = Self::frozen_annotation_style_capsule_size(toolbar_state)?;
		let min_x = toolbar_rect.left();
		let max_x = (toolbar_rect.right() - style_size.x).max(min_x);
		let x = (toolbar_rect.center().x - style_size.x * 0.5).clamp(min_x, max_x);
		let y = match toolbar_state.annotation_style_capsule_placement {
			FrozenAnnotationStyleCapsulePlacement::Above => {
				toolbar_rect.min.y - FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS - style_size.y
			},
			FrozenAnnotationStyleCapsulePlacement::Below => {
				toolbar_rect.max.y + FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS
			},
		};

		Some(Rect::from_min_size(Pos2::new(x, y), style_size))
	}

	pub(in crate::overlay) fn frozen_toolbar_window_rect(
		toolbar_state: &FrozenToolbarState,
		toolbar_pos: Pos2,
	) -> Rect {
		let toolbar_rect = Self::frozen_toolbar_primary_rect(toolbar_state, toolbar_pos);

		Self::frozen_annotation_style_capsule_rect(toolbar_state, toolbar_rect)
			.map_or(toolbar_rect, |style_rect| toolbar_rect.union(style_rect))
	}

	#[cfg(any(target_os = "macos", test))]
	pub(in crate::overlay) fn frozen_toolbar_visible_capsules_contain(
		toolbar_state: &FrozenToolbarState,
		toolbar_pos: Pos2,
		cursor_local: Pos2,
	) -> bool {
		let toolbar_rect = Self::frozen_toolbar_primary_rect(toolbar_state, toolbar_pos);

		if toolbar_rect.contains(cursor_local) {
			return true;
		}

		Self::frozen_annotation_style_capsule_rect(toolbar_state, toolbar_rect)
			.is_some_and(|style_rect| style_rect.contains(cursor_local))
	}

	pub(in crate::overlay) fn frozen_toolbar_size(toolbar_state: &FrozenToolbarState) -> Vec2 {
		Self::frozen_toolbar_window_rect(toolbar_state, Pos2::ZERO).size()
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn resolve_frozen_toolbar_birth(
		ctx: &Context,
		state: &OverlayState,
		monitor: MonitorRect,
		toolbar_state: &mut FrozenToolbarState,
		screen_rect: Rect,
		capture_rect: Rect,
		toolbar_primary_size: Vec2,
		toolbar_size: Vec2,
		toolbar_placement: ToolbarPlacement,
	) -> Option<Pos2> {
		if let Some(pos) = toolbar_state.floating_position {
			#[cfg(any(not(target_os = "macos"), test))]
			Self::sync_frozen_annotation_style_capsule_placement(toolbar_state, screen_rect, pos);

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

		let needs_new_sample = toolbar_layout_model::frozen_toolbar_needs_new_sample(
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

		let default_pos = Self::frozen_toolbar_default_window_pos(
			screen_rect,
			capture_rect,
			toolbar_primary_size,
			toolbar_size,
			toolbar_placement,
		);

		tracing::debug!(
			monitor_id = monitor.id,
			frozen_generation = state.frozen_generation,
			toolbar_primary_size_points = ?toolbar_primary_size,
			toolbar_size_points = ?toolbar_size,
			default_pos = ?default_pos,
			"Frozen toolbar birth resolved."
		);

		toolbar_state.default_slot_position = Some(default_pos);
		toolbar_state.floating_position = Some(default_pos);

		#[cfg(any(not(target_os = "macos"), test))]
		{
			Self::sync_frozen_annotation_style_capsule_placement(
				toolbar_state,
				screen_rect,
				default_pos,
			);
		}

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

	pub(in crate::overlay) fn frozen_toolbar_default_window_pos(
		screen_rect: Rect,
		capture_rect: Rect,
		toolbar_primary_size: Vec2,
		toolbar_positioning_size: Vec2,
		toolbar_placement: ToolbarPlacement,
	) -> Pos2 {
		let y = match toolbar_placement {
			ToolbarPlacement::Bottom => {
				let below_y = capture_rect.max.y + TOOLBAR_CAPTURE_GAP_PX;
				let within_screen = below_y + toolbar_primary_size.y + TOOLBAR_SCREEN_MARGIN_PX
					<= screen_rect.max.y;

				if within_screen {
					below_y
				} else {
					capture_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - toolbar_primary_size.y
				}
			},
			ToolbarPlacement::Top => {
				let above_y = capture_rect.min.y - TOOLBAR_CAPTURE_GAP_PX - toolbar_primary_size.y;
				let within_screen = above_y >= screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;

				if within_screen { above_y } else { capture_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX }
			},
		};
		let min_y = screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;
		let max_y =
			(screen_rect.max.y - toolbar_positioning_size.y - TOOLBAR_SCREEN_MARGIN_PX).max(min_y);
		let ideal_x = capture_rect.center().x - toolbar_primary_size.x / 2.0;
		let min_x = screen_rect.min.x + TOOLBAR_SCREEN_MARGIN_PX;
		let max_x =
			(screen_rect.max.x - toolbar_positioning_size.x - TOOLBAR_SCREEN_MARGIN_PX).max(min_x);
		let x = ideal_x.clamp(min_x, max_x);
		let y = y.max(min_y).min(max_y);

		Pos2::new(x, y)
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
