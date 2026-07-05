mod annotation_style;
mod geometry;
mod tool_palette;

use egui::Context;

use crate::overlay::hud_pill_style::{
	HUD_PILL_INNER_MARGIN_X_POINTS, HUD_PILL_STROKE_WIDTH_POINTS,
};
use crate::overlay::rendering::{FrozenToolbarButtonStyle, WindowRenderer};
#[cfg(not(target_os = "macos"))]
use crate::overlay::toolbar_geometry::TOOLBAR_SCREEN_MARGIN_PX;
use crate::overlay::toolbar_geometry::{
	FROZEN_TOOLBAR_BUTTON_SIZE_POINTS, FROZEN_TOOLBAR_ITEM_SPACING_POINTS,
	TOOLBAR_PILL_INNER_MARGIN_Y_POINTS,
};
use crate::overlay::{
	Align, Align2, Area, Color32, CornerRadius, FontFamily, FontId, FrozenToolbarPointerState,
	FrozenToolbarState, FrozenToolbarTool, HudPillGeometry, HudTheme, Id, Layout, MonitorRect,
	Order, OverlayMode, OverlayState, Pos2, Rect, Sense, Stroke, StrokeKind, ToolbarPlacement, Ui,
	UiBuilder, Vec2, toolbar_layout_model,
};
use annotation_style::FROZEN_ANNOTATION_TOOLBAR_SECTION_HEIGHT_POINTS;

impl WindowRenderer {
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
		#[cfg(target_os = "macos")]
		let _ = pointer_state;

		if !matches!(state.mode, OverlayMode::Frozen) || !toolbar_state.visible {
			return;
		}
		if state.monitor != Some(monitor) {
			return;
		}

		#[cfg(not(target_os = "macos"))]
		let (cursor, left_button_down) = if let Some(pointer_state) = pointer_state {
			(pointer_state.cursor_local, pointer_state.left_button_down)
		} else {
			(Pos2::new(-1.0, -1.0), false)
		};
		let toolbar_primary_size = Self::frozen_toolbar_primary_size(toolbar_state);
		let toolbar_positioning_size = Self::frozen_toolbar_positioning_size(toolbar_state);
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
			toolbar_primary_size,
			toolbar_positioning_size,
			toolbar_placement,
		) else {
			return;
		};

		#[cfg(any(not(target_os = "macos"), test))]
		{
			if !toolbar_layout_model::advance_frozen_toolbar_readiness_sample_state(
				toolbar_state,
				screen_rect,
			) {
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
			#[cfg(not(target_os = "macos"))]
			cursor,
			#[cfg(not(target_os = "macos"))]
			left_button_down,
			hud_pill_out,
		);
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
		#[cfg(not(target_os = "macos"))] cursor: Pos2,
		#[cfg(not(target_os = "macos"))] left_button_down: bool,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		#[cfg(target_os = "macos")]
		let _ = screen_rect;
		let area_id = Id::new(format!("frozen-toolbar-{}", monitor.id));
		let window_rect = Self::frozen_toolbar_window_rect(toolbar_state, toolbar_pos);

		Area::new(area_id).order(Order::Foreground).fixed_pos(window_rect.min).show(ctx, |ui| {
			let (_area_rect, _window_response) =
				ui.allocate_exact_size(toolbar_size, Sense::hover());
			let toolbar_rect = Self::frozen_toolbar_primary_rect(toolbar_state, toolbar_pos);
			let style_rect =
				Self::frozen_annotation_style_capsule_rect(toolbar_state, toolbar_rect);
			let toolbar_response = ui.interact(
				toolbar_rect,
				area_id.with("primary-capsule"),
				if cfg!(target_os = "macos") { Sense::hover() } else { Sense::click_and_drag() },
			);
			#[cfg(target_os = "macos")]
			let _ = &toolbar_response;

			toolbar_state.annotation_size_control_hovered = false;

			#[cfg(not(target_os = "macos"))]
			Self::update_frozen_toolbar_drag_state(
				toolbar_state,
				toolbar_response.drag_started(),
				toolbar_pos,
				screen_rect,
				Self::frozen_toolbar_positioning_size(toolbar_state),
				cursor,
				left_button_down,
			);
			Self::paint_frozen_toolbar_capsule(
				ui,
				toolbar_rect,
				theme,
				hud_blur_active,
				hud_opaque,
				hud_opacity,
				hud_milk_amount,
				hud_tint_hue,
			);

			let toolbar_inner_rect = toolbar_rect.shrink2(egui::vec2(
				f32::from(HUD_PILL_INNER_MARGIN_X_POINTS),
				TOOLBAR_PILL_INNER_MARGIN_Y_POINTS,
			));
			let _ = ui.scope_builder(UiBuilder::new().max_rect(toolbar_inner_rect), |ui| {
				Self::render_frozen_toolbar_primary_row(
					ui,
					toolbar_inner_rect.width(),
					toolbar_state,
					theme,
				);
			});

			if let Some(style_rect) = style_rect {
				Self::paint_frozen_toolbar_capsule(
					ui,
					style_rect,
					theme,
					hud_blur_active,
					hud_opaque,
					hud_opacity,
					hud_milk_amount,
					hud_tint_hue,
				);

				let style_inner_rect = style_rect.shrink2(egui::vec2(
					f32::from(HUD_PILL_INNER_MARGIN_X_POINTS),
					TOOLBAR_PILL_INNER_MARGIN_Y_POINTS,
				));
				let _ = ui.scope_builder(UiBuilder::new().max_rect(style_inner_rect), |ui| {
					let _ = ui.allocate_ui_with_layout(
						Vec2::new(
							style_inner_rect.width(),
							FROZEN_ANNOTATION_TOOLBAR_SECTION_HEIGHT_POINTS,
						),
						Layout::left_to_right(Align::Center),
						|ui| {
							Self::render_frozen_annotation_toolbar_controls(
								ui,
								toolbar_state,
								theme,
							)
						},
					);
				});
			}

			*hud_pill_out = Some(HudPillGeometry {
				rect: window_rect,
				radius_points: f32::from(toolbar_layout_model::frozen_toolbar_corner_radius_u8(
					window_rect.height(),
				)),
			});
		});
	}

	#[allow(clippy::too_many_arguments)]
	fn paint_frozen_toolbar_capsule(
		ui: &Ui,
		rect: Rect,
		theme: HudTheme,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
	) {
		let corner_radius = toolbar_layout_model::frozen_toolbar_corner_radius_u8(rect.height());
		let body_fill = Self::tinted_hud_body_fill(
			theme,
			hud_blur_active,
			hud_opaque,
			hud_opacity,
			hud_milk_amount,
			hud_tint_hue,
		);
		let toolbar_frame = Self::hud_pill_frame(theme, hud_opaque, hud_opacity, body_fill, false);

		ui.painter().rect_filled(rect, f32::from(corner_radius), toolbar_frame.fill);
		ui.painter().rect_stroke(
			rect.shrink(0.5),
			CornerRadius::same(corner_radius),
			toolbar_frame.stroke,
			StrokeKind::Inside,
		);

		let inner_stroke_color = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
			HudTheme::Light => Color32::from_rgba_unmultiplied(255, 255, 255, 140),
		};
		let inner_rect = rect.shrink(HUD_PILL_STROKE_WIDTH_POINTS);

		ui.painter().rect_stroke(
			inner_rect,
			CornerRadius::same(corner_radius.saturating_sub(1)),
			Stroke::new(HUD_PILL_STROKE_WIDTH_POINTS, inner_stroke_color),
			StrokeKind::Inside,
		);
	}

	#[cfg(not(target_os = "macos"))]
	fn update_frozen_toolbar_drag_state(
		toolbar_state: &mut FrozenToolbarState,
		drag_started: bool,
		toolbar_pos: Pos2,
		screen_rect: Rect,
		toolbar_size: Vec2,
		cursor: Pos2,
		left_button_down: bool,
	) {
		if drag_started {
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
		}
	}

	fn render_frozen_toolbar_primary_row(
		ui: &mut Ui,
		width: f32,
		toolbar_state: &mut FrozenToolbarState,
		theme: HudTheme,
	) {
		let _ = ui.allocate_ui_with_layout(
			Vec2::new(width, FROZEN_TOOLBAR_BUTTON_SIZE_POINTS),
			Layout::left_to_right(Align::Center),
			|ui| {
				ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);

				Self::render_frozen_toolbar_controls(ui, toolbar_state, theme);
			},
		);
	}

	#[allow(clippy::too_many_arguments)]
	fn render_frozen_toolbar_controls(
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
				let action_ready = tool.is_available(toolbar_state)
					&& (!tool.requires_final_capture() || toolbar_state.final_capture_ready);
				let response =
					ui.allocate_response(Vec2::new(button_size, button_size), Sense::click());
				let hovered = action_ready && response.hovered();
				let response = if action_ready {
					response.on_hover_text(tool.label())
				} else {
					response.on_hover_text(tool.unavailable_label(toolbar_state))
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
}
