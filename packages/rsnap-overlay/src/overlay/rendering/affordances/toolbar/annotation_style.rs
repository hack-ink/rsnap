use egui::{Response, Ui};
use egui_phosphor::regular::{MINUS, PLUS};

use crate::overlay::rendering::WindowRenderer;
use crate::overlay::{
	Align2, Color32, CornerRadius, FontFamily, FontId, FrozenAnnotationColor, FrozenToolbarState,
	FrozenToolbarTool, HudTheme, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
};

pub(super) const FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS: f32 = 4.0;
pub(super) const FROZEN_ANNOTATION_TOOLBAR_SECTION_HEIGHT_POINTS: f32 = 24.0;
pub(super) const FROZEN_ANNOTATION_TOOLBAR_SWATCH_SIZE_POINTS: f32 = 16.0;
pub(super) const FROZEN_ANNOTATION_TOOLBAR_SWATCH_GAP_POINTS: f32 = 6.0;

const FROZEN_ANNOTATION_TOOLBAR_SIZE_BUTTON_WIDTH_POINTS: f32 = 20.0;
const FROZEN_ANNOTATION_TOOLBAR_SIZE_DISPLAY_WIDTH_POINTS: f32 = 58.0;
const FROZEN_ANNOTATION_TOOLBAR_PEN_SIZE_DISPLAY_WIDTH_POINTS: f32 = 84.0;
const FROZEN_ANNOTATION_TOOLBAR_SIZE_CAPSULE_CORNER_RADIUS_POINTS: u8 = 8;
const FROZEN_ANNOTATION_TOOLBAR_SIZE_PREVIEW_GAP_POINTS: f32 = 8.0;
const FROZEN_ANNOTATION_TOOLBAR_PEN_PREVIEW_LENGTH_POINTS: f32 = 18.0;
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum FrozenAnnotationStyleToolbarKind {
	Pen,
	Text,
}
impl FrozenAnnotationStyleToolbarKind {
	pub(super) fn from_toolbar_state(toolbar_state: &FrozenToolbarState) -> Option<Self> {
		match toolbar_state.selected_tool {
			FrozenToolbarTool::Pen | FrozenToolbarTool::Arrow => Some(Self::Pen),
			FrozenToolbarTool::Text => Some(Self::Text),
			_ => None,
		}
	}

	const fn size_display_width(self) -> f32 {
		match self {
			Self::Pen => FROZEN_ANNOTATION_TOOLBAR_PEN_SIZE_DISPLAY_WIDTH_POINTS,
			Self::Text => FROZEN_ANNOTATION_TOOLBAR_SIZE_DISPLAY_WIDTH_POINTS,
		}
	}

	pub(super) const fn size_control_width(self) -> f32 {
		self.size_display_width() + FROZEN_ANNOTATION_TOOLBAR_SIZE_BUTTON_WIDTH_POINTS * 2.0
	}

	fn size_value(self, toolbar_state: &FrozenToolbarState) -> f64 {
		match self {
			Self::Pen => toolbar_state.brush_style.stroke_width_points,
			Self::Text => toolbar_state.text_style.font_size_points,
		}
		.into()
	}

	fn formatted_size_text(self, toolbar_state: &FrozenToolbarState) -> String {
		match self {
			Self::Pen => {
				let size_points = self.size_value(toolbar_state);
				let mut text = format!("{size_points:.2}");

				while text.contains('.') && text.ends_with('0') {
					let _ = text.pop();
				}

				if text.ends_with('.') {
					let _ = text.pop();
				}

				text
			},
			Self::Text => {
				let font_size = toolbar_state.text_style.font_size_points;

				if (font_size - font_size.round()).abs() <= f32::EPSILON {
					format!("{}", font_size.round() as i32)
				} else {
					format!("{font_size:.1}")
				}
			},
		}
	}

	fn selected_color(self, toolbar_state: &FrozenToolbarState) -> FrozenAnnotationColor {
		match self {
			Self::Pen => toolbar_state.brush_style.color,
			Self::Text => toolbar_state.text_style.color,
		}
	}

	fn set_color(
		self,
		toolbar_state: &mut FrozenToolbarState,
		color: FrozenAnnotationColor,
	) -> bool {
		let selected_color = match self {
			Self::Pen => &mut toolbar_state.brush_style.color,
			Self::Text => &mut toolbar_state.text_style.color,
		};

		if *selected_color == color {
			return false;
		}

		*selected_color = color;

		true
	}

	fn apply_size_steps(self, toolbar_state: &mut FrozenToolbarState, steps: i32) -> bool {
		toolbar_state.apply_annotation_size_steps(steps)
	}
}

#[derive(Clone, Copy)]
struct FrozenAnnotationSizeControlAppearance {
	capsule_fill: Color32,
	capsule_stroke: Color32,
	divider_color: Color32,
	button_hover_fill: Color32,
	text_color: Color32,
}
impl WindowRenderer {
	pub(super) fn render_frozen_annotation_toolbar_controls(
		ui: &mut Ui,
		toolbar_state: &mut FrozenToolbarState,
		theme: HudTheme,
	) {
		let Some(style_kind) = FrozenAnnotationStyleToolbarKind::from_toolbar_state(toolbar_state)
		else {
			toolbar_state.annotation_size_control_hovered = false;

			return;
		};
		let size_label = match style_kind {
			FrozenAnnotationStyleToolbarKind::Text => {
				format!("{} pt", style_kind.formatted_size_text(toolbar_state))
			},
			FrozenAnnotationStyleToolbarKind::Pen => style_kind.formatted_size_text(toolbar_state),
		};

		ui.horizontal_centered(|ui| {
			ui.spacing_mut().item_spacing.x = FROZEN_ANNOTATION_TOOLBAR_SWATCH_GAP_POINTS;

			Self::render_frozen_annotation_size_control(
				ui,
				toolbar_state,
				theme,
				style_kind,
				&size_label,
			);

			ui.add_space(4.0);

			for color in FrozenAnnotationColor::ALL {
				if Self::render_frozen_annotation_color_swatch(
					ui,
					color,
					style_kind.selected_color(toolbar_state) == color,
					theme,
				) && style_kind.set_color(toolbar_state, color)
				{
					toolbar_state.needs_redraw = true;
				}
			}
		});

		if !toolbar_state.annotation_size_control_hovered {
			toolbar_state.annotation_size_wheel_accumulator = 0.0;
		}
	}

	fn render_frozen_annotation_size_control(
		ui: &mut Ui,
		toolbar_state: &mut FrozenToolbarState,
		theme: HudTheme,
		style_kind: FrozenAnnotationStyleToolbarKind,
		size_label: &str,
	) {
		let (size_rect, size_response) = ui.allocate_exact_size(
			Vec2::new(
				style_kind.size_control_width(),
				FROZEN_ANNOTATION_TOOLBAR_SECTION_HEIGHT_POINTS,
			),
			Sense::hover(),
		);
		let minus_rect = Rect::from_min_max(
			size_rect.min,
			Pos2::new(
				size_rect.min.x + FROZEN_ANNOTATION_TOOLBAR_SIZE_BUTTON_WIDTH_POINTS,
				size_rect.max.y,
			),
		);
		let plus_rect = Rect::from_min_max(
			Pos2::new(
				size_rect.max.x - FROZEN_ANNOTATION_TOOLBAR_SIZE_BUTTON_WIDTH_POINTS,
				size_rect.min.y,
			),
			size_rect.max,
		);
		let display_rect = Rect::from_min_max(
			Pos2::new(minus_rect.max.x, size_rect.min.y),
			Pos2::new(plus_rect.min.x, size_rect.max.y),
		);
		let minus_response = ui.interact(
			minus_rect,
			ui.id().with(("annotation-size-decrease", style_kind)),
			Sense::click(),
		);
		let plus_response = ui.interact(
			plus_rect,
			ui.id().with(("annotation-size-increase", style_kind)),
			Sense::click(),
		);
		let hovered =
			size_response.hovered() || minus_response.hovered() || plus_response.hovered();
		let capsule_rect = size_rect.shrink2(egui::vec2(1.0, 3.0));
		let appearance = Self::frozen_annotation_size_control_appearance(theme, hovered);

		toolbar_state.annotation_size_control_hovered = hovered;

		Self::paint_frozen_annotation_size_control_frame(
			ui,
			capsule_rect,
			display_rect,
			&minus_response,
			&plus_response,
			appearance,
		);
		Self::paint_frozen_annotation_size_step_button(ui, theme, &minus_response, MINUS);
		Self::paint_frozen_annotation_size_step_button(ui, theme, &plus_response, PLUS);
		Self::apply_frozen_annotation_size_control_clicks(
			toolbar_state,
			style_kind,
			&minus_response,
			&plus_response,
		);
		Self::paint_frozen_annotation_size_display(
			ui,
			toolbar_state,
			style_kind,
			display_rect,
			size_label,
			appearance.text_color,
		);
	}

	fn frozen_annotation_size_control_appearance(
		theme: HudTheme,
		hovered: bool,
	) -> FrozenAnnotationSizeControlAppearance {
		match theme {
			HudTheme::Dark => FrozenAnnotationSizeControlAppearance {
				capsule_fill: Color32::from_rgba_unmultiplied(
					255,
					255,
					255,
					if hovered { 22 } else { 12 },
				),
				capsule_stroke: Color32::from_rgba_unmultiplied(
					255,
					255,
					255,
					if hovered { 34 } else { 22 },
				),
				divider_color: Color32::from_white_alpha(if hovered { 34 } else { 22 }),
				button_hover_fill: Color32::from_rgba_unmultiplied(255, 255, 255, 16),
				text_color: Self::frozen_toolbar_button_style(theme, true, hovered, false)
					.icon_color,
			},
			HudTheme::Light => FrozenAnnotationSizeControlAppearance {
				capsule_fill: Color32::from_rgba_unmultiplied(
					0,
					0,
					0,
					if hovered { 18 } else { 10 },
				),
				capsule_stroke: Color32::from_rgba_unmultiplied(
					0,
					0,
					0,
					if hovered { 28 } else { 18 },
				),
				divider_color: Color32::from_black_alpha(if hovered { 30 } else { 18 }),
				button_hover_fill: Color32::from_rgba_unmultiplied(0, 0, 0, 14),
				text_color: Self::frozen_toolbar_button_style(theme, true, hovered, false)
					.icon_color,
			},
		}
	}

	fn paint_frozen_annotation_size_control_frame(
		ui: &Ui,
		capsule_rect: Rect,
		display_rect: Rect,
		minus_response: &Response,
		plus_response: &Response,
		appearance: FrozenAnnotationSizeControlAppearance,
	) {
		ui.painter().rect_filled(
			capsule_rect,
			CornerRadius::same(FROZEN_ANNOTATION_TOOLBAR_SIZE_CAPSULE_CORNER_RADIUS_POINTS),
			appearance.capsule_fill,
		);
		ui.painter().rect_stroke(
			capsule_rect,
			CornerRadius::same(FROZEN_ANNOTATION_TOOLBAR_SIZE_CAPSULE_CORNER_RADIUS_POINTS),
			Stroke::new(1.0, appearance.capsule_stroke),
			StrokeKind::Inside,
		);

		for response in [minus_response, plus_response] {
			if response.hovered() {
				ui.painter().rect_filled(
					response.rect.shrink2(egui::vec2(2.0, 4.0)),
					CornerRadius::same(6),
					appearance.button_hover_fill,
				);
			}
		}
		for divider_x in [display_rect.left(), display_rect.right()] {
			ui.painter().line_segment(
				[
					Pos2::new(divider_x, capsule_rect.top() + 5.0),
					Pos2::new(divider_x, capsule_rect.bottom() - 5.0),
				],
				Stroke::new(1.0, appearance.divider_color),
			);
		}
	}

	fn paint_frozen_annotation_size_step_button(
		ui: &Ui,
		theme: HudTheme,
		response: &Response,
		icon: &str,
	) {
		let button_style =
			Self::frozen_toolbar_button_style(theme, true, response.hovered(), false);

		ui.painter().text(
			response.rect.center(),
			Align2::CENTER_CENTER,
			icon,
			FontId::new(13.0, FontFamily::Proportional),
			button_style.icon_color,
		);
	}

	fn apply_frozen_annotation_size_control_clicks(
		toolbar_state: &mut FrozenToolbarState,
		style_kind: FrozenAnnotationStyleToolbarKind,
		minus_response: &Response,
		plus_response: &Response,
	) {
		let mut size_changed = false;

		if minus_response.clicked() {
			toolbar_state.annotation_size_wheel_accumulator = 0.0;
			size_changed |= style_kind.apply_size_steps(toolbar_state, -1);
		}
		if plus_response.clicked() {
			toolbar_state.annotation_size_wheel_accumulator = 0.0;
			size_changed |= style_kind.apply_size_steps(toolbar_state, 1);
		}
		if size_changed {
			toolbar_state.needs_redraw = true;
		}
	}

	fn paint_frozen_annotation_size_display(
		ui: &Ui,
		toolbar_state: &FrozenToolbarState,
		style_kind: FrozenAnnotationStyleToolbarKind,
		display_rect: Rect,
		size_label: &str,
		text_color: Color32,
	) {
		match style_kind {
			FrozenAnnotationStyleToolbarKind::Text => {
				ui.painter().text(
					display_rect.center(),
					Align2::CENTER_CENTER,
					size_label,
					FontId::new(13.0, FontFamily::Proportional),
					text_color,
				);
			},
			FrozenAnnotationStyleToolbarKind::Pen => {
				let preview_width = toolbar_state.brush_style.stroke_width_points.clamp(1.0, 10.0);
				let preview_center = Pos2::new(
					display_rect.left()
						+ 10.0 + FROZEN_ANNOTATION_TOOLBAR_PEN_PREVIEW_LENGTH_POINTS * 0.5,
					display_rect.center().y,
				);
				let preview_half_length = FROZEN_ANNOTATION_TOOLBAR_PEN_PREVIEW_LENGTH_POINTS * 0.5;
				let preview_start =
					Pos2::new(preview_center.x - preview_half_length, preview_center.y);
				let preview_end =
					Pos2::new(preview_center.x + preview_half_length, preview_center.y);
				let preview_color = toolbar_state.brush_style.color.swatch_fill();

				ui.painter().line_segment(
					[preview_start, preview_end],
					Stroke::new(preview_width, preview_color),
				);
				ui.painter().circle_filled(preview_start, preview_width * 0.5, preview_color);
				ui.painter().circle_filled(preview_end, preview_width * 0.5, preview_color);
				ui.painter().text(
					Pos2::new(
						preview_end.x + FROZEN_ANNOTATION_TOOLBAR_SIZE_PREVIEW_GAP_POINTS,
						display_rect.center().y,
					),
					Align2::LEFT_CENTER,
					size_label,
					FontId::new(13.0, FontFamily::Proportional),
					text_color,
				);
			},
		}
	}

	fn render_frozen_annotation_color_swatch(
		ui: &mut Ui,
		color: FrozenAnnotationColor,
		selected: bool,
		theme: HudTheme,
	) -> bool {
		let response = ui.allocate_response(
			Vec2::splat(FROZEN_ANNOTATION_TOOLBAR_SWATCH_SIZE_POINTS),
			Sense::click(),
		);
		let radius = FROZEN_ANNOTATION_TOOLBAR_SWATCH_SIZE_POINTS * 0.5 - 1.0;
		let stroke_color = match theme {
			HudTheme::Dark => {
				if selected {
					Color32::WHITE
				} else {
					Color32::from_white_alpha(96)
				}
			},
			HudTheme::Light => {
				if selected {
					Color32::BLACK
				} else {
					Color32::from_black_alpha(96)
				}
			},
		};

		ui.painter().circle_filled(response.rect.center(), radius, color.swatch_fill());
		ui.painter().circle_stroke(
			response.rect.center(),
			radius,
			Stroke::new(if selected { 2.0 } else { 1.0 }, stroke_color),
		);

		response.clicked()
	}
}
