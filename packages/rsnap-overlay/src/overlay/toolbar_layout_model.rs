use egui::{Pos2, Rect, Vec2};

use crate::overlay::hud_pill_style::HUD_PILL_CORNER_RADIUS_POINTS;
use crate::overlay::rendering::WindowRenderer;
use crate::overlay::{
	FrozenToolbarState, FrozenToolbarTool, TOOLBAR_DEFAULT_SLOT_POSITION_EPSILON_POINTS,
	TOOLBAR_EXPANDED_HEIGHT_PX,
};

pub(super) fn frozen_toolbar_corner_radius_u8(toolbar_height_points: f32) -> u8 {
	if toolbar_height_points <= TOOLBAR_EXPANDED_HEIGHT_PX + 0.5 {
		(toolbar_height_points * 0.5).round().clamp(1.0, f32::from(u8::MAX)) as u8
	} else {
		HUD_PILL_CORNER_RADIUS_POINTS
	}
}

pub(super) fn frozen_toolbar_corner_radius_points(toolbar_height_points: f32) -> f64 {
	f64::from(frozen_toolbar_corner_radius_u8(toolbar_height_points))
}

pub(super) fn frozen_toolbar_window_startup_size_points() -> Vec2 {
	[
		FrozenToolbarState::default(),
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Pen,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Arrow,
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
			selected_tool: FrozenToolbarTool::Arrow,
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
	]
	.into_iter()
	.map(|toolbar_state| WindowRenderer::frozen_toolbar_size(&toolbar_state))
	.fold(Vec2::new(0.0, TOOLBAR_EXPANDED_HEIGHT_PX), |max_size, size| {
		Vec2::new(max_size.x.max(size.x), max_size.y.max(size.y))
	}) + Vec2::new(0.0, WindowRenderer::frozen_toolbar_window_top_padding_points())
}

pub(super) fn frozen_toolbar_needs_new_sample(
	last_screen_size_points: Option<Vec2>,
	screen_size_points: Vec2,
) -> bool {
	match last_screen_size_points {
		None => true,
		Some(last) => {
			let dx = (last.x - screen_size_points.x).abs();
			let dy = (last.y - screen_size_points.y).abs();

			dx > 0.5 || dy > 0.5
		},
	}
}

pub(super) fn advance_frozen_toolbar_readiness_sample_state(
	toolbar_state: &mut FrozenToolbarState,
	screen_rect: Rect,
) -> bool {
	let screen_size_points = screen_rect.size();

	if frozen_toolbar_needs_new_sample(
		toolbar_state.layout_last_screen_size_points,
		screen_size_points,
	) {
		toolbar_state.layout_last_screen_size_points = Some(screen_size_points);
		toolbar_state.layout_stable_frames = 0;

		return false;
	}
	if toolbar_state.layout_stable_frames < 1 {
		toolbar_state.layout_stable_frames = toolbar_state.layout_stable_frames.saturating_add(1);

		return false;
	}

	true
}

pub(super) fn frozen_toolbar_matches_default_slot(toolbar_pos: Pos2, default_pos: Pos2) -> bool {
	let dx = (toolbar_pos.x - default_pos.x).abs();
	let dy = (toolbar_pos.y - default_pos.y).abs();

	dx <= TOOLBAR_DEFAULT_SLOT_POSITION_EPSILON_POINTS
		&& dy <= TOOLBAR_DEFAULT_SLOT_POSITION_EPSILON_POINTS
}

#[cfg(target_os = "macos")]
pub(super) fn frozen_toolbar_window_primary_origin() -> Pos2 {
	Pos2::new(0.0, WindowRenderer::frozen_toolbar_window_top_padding_points())
}
