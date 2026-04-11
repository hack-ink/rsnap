use egui::Context;

use crate::overlay::rendering::WindowRenderer;
use crate::overlay::{
	Align, Area, Color32, ColorImage, CornerRadius, Frame, GlobalPoint, HUD_LOUPE_STRIP_GAP_POINTS,
	HUD_PILL_CORNER_RADIUS_POINTS, HudAnchor, HudPillGeometry, HudTheme, Id, Layout, Margin,
	MonitorRect, Order, OverlayMode, OverlayState, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
	TextureHandle, TextureId, TextureOptions, Ui, Vec2, hud_helpers,
};
use crate::state::LoupeSample;

pub(super) struct LiveLoupeTexture {
	texture: TextureHandle,
	patch_size_px: [usize; 2],
	rgba: Vec<u8>,
}

impl WindowRenderer {
	pub(in crate::overlay::rendering) fn should_draw_hud(
		state: &OverlayState,
		monitor: MonitorRect,
	) -> bool {
		let _ = monitor;

		matches!(state.mode, OverlayMode::Live) || state.error_message.is_some()
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay::rendering) fn render_hud(
		&mut self,
		ctx: &Context,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		local_cursor: Pos2,
		hud_compact: bool,
		hud_anchor: HudAnchor,
		show_alt_hint_keycap: bool,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		theme: HudTheme,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		let (hud_x, hud_y) = match hud_anchor {
			HudAnchor::Cursor => (local_cursor.x + 14.0, local_cursor.y + 14.0),
		};

		Area::new("hud".into()).order(Order::Foreground).fixed_pos(Pos2::new(hud_x, hud_y)).show(
			ctx,
			|ui| {
				self.render_hud_frame(
					ui,
					state,
					monitor,
					cursor,
					hud_compact,
					show_alt_hint_keycap,
					hud_blur_active,
					hud_opaque,
					hud_opacity,
					hud_milk_amount,
					hud_tint_hue,
					theme,
					hud_pill_out,
				);
			},
		);
	}

	#[allow(clippy::too_many_arguments)]
	fn render_hud_frame(
		&mut self,
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		hud_compact: bool,
		show_alt_hint_keycap: bool,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		theme: HudTheme,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		let body_fill = Self::tinted_hud_body_fill(
			theme,
			hud_blur_active,
			hud_opaque,
			hud_opacity,
			hud_milk_amount,
			hud_tint_hue,
		);
		let pill_frame =
			Self::hud_pill_frame(theme, hud_opaque, hud_opacity, body_fill, !hud_compact);
		let inner = pill_frame.show(ui, |ui| {
			ui.spacing_mut().item_spacing = egui::vec2(10.0, 6.0);

			if let Some(err) = &state.error_message {
				let err_color = match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(235, 235, 245, 235),
					HudTheme::Light => Color32::from_rgba_unmultiplied(28, 28, 32, 235),
				};

				ui.label(RichText::new(err).color(err_color).monospace());
			} else {
				Self::render_hud_content(ui, state, monitor, cursor, show_alt_hint_keycap, theme);
			}
		});
		let pill_rect = inner.response.rect;

		*hud_pill_out = Some(HudPillGeometry {
			rect: pill_rect,
			radius_points: f32::from(HUD_PILL_CORNER_RADIUS_POINTS),
		});

		if hud_compact {
			return;
		}

		let inner_stroke_color = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
			HudTheme::Light => Color32::from_rgba_unmultiplied(255, 255, 255, 140),
		};
		let inner_stroke = Stroke::new(1.0, inner_stroke_color);
		let inner_rect = pill_rect.shrink(1.0);

		ui.painter().rect_stroke(
			inner_rect,
			CornerRadius::same(HUD_PILL_CORNER_RADIUS_POINTS.saturating_sub(1)),
			inner_stroke,
			StrokeKind::Inside,
		);

		if !hud_compact {
			self.render_loupe_tile(
				ui,
				state,
				pill_rect,
				hud_blur_active,
				hud_opaque,
				body_fill,
				theme,
			);
		}
	}

	pub(in crate::overlay::rendering) fn hud_pill_frame(
		theme: HudTheme,
		_hud_opaque: bool,
		_hud_opacity: f32,
		body_fill: Color32,
		with_shadow: bool,
	) -> Frame {
		let outer_stroke_color = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 40),
			HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
		};
		let pill_shadow = if with_shadow {
			egui::epaint::Shadow {
				offset: [0, 0],
				blur: 10,
				spread: 0,
				color: match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 28),
					HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 18),
				},
			}
		} else {
			egui::Shadow::NONE
		};

		Frame {
			fill: body_fill,
			stroke: Stroke::new(1.0, outer_stroke_color),
			shadow: pill_shadow,
			corner_radius: CornerRadius::same(HUD_PILL_CORNER_RADIUS_POINTS),
			inner_margin: Margin::symmetric(12, 8),
			..Frame::default()
		}
	}

	fn render_hud_content(
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		show_alt_hint_keycap: bool,
		theme: HudTheme,
	) {
		let (label_color, secondary_color) = Self::hud_text_colors(theme);
		let pos_text = hud_helpers::format_live_hud_position_text(monitor, cursor);
		let (hex_text, rgb_text) = hud_helpers::format_live_hud_rgb_text(state.rgb);
		let swatch_size = egui::vec2(10.0, 10.0);

		ui.vertical(|ui| {
			ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
				ui.label(RichText::new(pos_text).color(label_color).monospace());
				ui.label(RichText::new("•").color(secondary_color).monospace());

				let (rect, _) = ui.allocate_exact_size(swatch_size, Sense::hover());
				let swatch_color = match state.rgb {
					Some(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
					None => Color32::from_rgba_unmultiplied(255, 255, 255, 26),
				};

				ui.painter().rect_filled(rect, 3.0, swatch_color);
				ui.painter().rect_stroke(
					rect,
					3.0,
					Stroke::new(
						1.0,
						match theme {
							HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 36),
							HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
						},
					),
					StrokeKind::Inside,
				);
				ui.label(RichText::new(hex_text).color(label_color).monospace());
				ui.label(RichText::new(rgb_text).color(secondary_color).monospace());

				if show_alt_hint_keycap {
					let alt_active = state.alt_held;
					let (keycap_fill, keycap_stroke, keycap_text) = match theme {
						HudTheme::Dark if alt_active => (
							Color32::from_rgba_unmultiplied(255, 255, 255, 40),
							Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 70)),
							label_color,
						),
						HudTheme::Dark => (
							Color32::from_rgba_unmultiplied(255, 255, 255, 18),
							Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 30)),
							secondary_color,
						),
						HudTheme::Light if alt_active => (
							Color32::from_rgba_unmultiplied(0, 0, 0, 22),
							Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 64)),
							label_color,
						),
						HudTheme::Light => (
							Color32::from_rgba_unmultiplied(0, 0, 0, 12),
							Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 32)),
							secondary_color,
						),
					};

					Frame {
						fill: keycap_fill,
						stroke: keycap_stroke,
						corner_radius: CornerRadius::same(6),
						inner_margin: Margin::symmetric(6, 2),
						..Frame::default()
					}
					.show(ui, |ui| {
						ui.label(RichText::new("Tab").color(keycap_text).monospace());
					});
				}
			});
		});
	}

	pub(in crate::overlay::rendering) fn hud_text_colors(theme: HudTheme) -> (Color32, Color32) {
		match theme {
			HudTheme::Dark => (
				Color32::from_rgba_unmultiplied(235, 235, 245, 235),
				Color32::from_rgba_unmultiplied(235, 235, 245, 150),
			),
			HudTheme::Light => (
				Color32::from_rgba_unmultiplied(28, 28, 32, 235),
				Color32::from_rgba_unmultiplied(28, 28, 32, 160),
			),
		}
	}

	#[allow(clippy::too_many_arguments)]
	fn render_loupe_tile(
		&mut self,
		ui: &mut Ui,
		state: &OverlayState,
		pill_rect: Rect,
		hud_blur_active: bool,
		hud_opaque: bool,
		body_fill: Color32,
		theme: HudTheme,
	) {
		let ctx = ui.ctx().clone();

		self.loupe_tile = None;

		if !state.alt_held {
			return;
		}

		const CELL: f32 = 10.0;

		let side = hud_helpers::stable_live_loupe_side_points(state, CELL);
		let tile_padding = Margin::same(10);
		let tile_w = side + (tile_padding.left as f32) + (tile_padding.right as f32);
		let tile_h = side + (tile_padding.top as f32) + (tile_padding.bottom as f32);
		let screen = ctx.content_rect();
		let gap = HUD_LOUPE_STRIP_GAP_POINTS as f32;
		let mut x = pill_rect.min.x;

		x = x.clamp(screen.min.x + 6.0, (screen.max.x - tile_w - 6.0).max(screen.min.x + 6.0));

		let below_y = pill_rect.max.y + gap;
		let above_y = pill_rect.min.y - gap - tile_h;
		let mut y = if below_y + tile_h <= screen.max.y { below_y } else { above_y };

		y = y.clamp(screen.min.y + 6.0, (screen.max.y - tile_h - 6.0).max(screen.min.y + 6.0));

		let pos = Pos2::new(x, y);
		let tile = Area::new(Id::new("rsnap-loupe-tile"))
			.order(Order::Foreground)
			.fixed_pos(pos)
			.show(&ctx, |ui| {
				let _ = hud_blur_active;
				let fill = body_fill;
				let outer_stroke_color = match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 40),
					HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
				};
				let outer_stroke = Stroke::new(1.0, outer_stroke_color);
				let shadow = egui::epaint::Shadow {
					offset: [0, 0],
					blur: 10,
					spread: 0,
					color: match theme {
						HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 28),
						HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 18),
					},
				};
				let frame = Frame {
					fill,
					stroke: outer_stroke,
					shadow,
					corner_radius: CornerRadius::same(18),
					inner_margin: tile_padding,
					..Frame::default()
				};

				frame.show(ui, |ui| {
					ui.set_min_size(Vec2::new(side, side));
					self.render_loupe(ui, state, hud_blur_active, hud_opaque, theme);
				});
			});

		self.loupe_tile = Some(tile.response.rect);
	}

	pub(in crate::overlay::rendering) fn render_loupe(
		&mut self,
		ui: &mut Ui,
		state: &OverlayState,
		hud_blur_active: bool,
		hud_opaque: bool,
		theme: HudTheme,
	) {
		const CELL: f32 = 10.0;

		if matches!(state.mode, OverlayMode::Live) {
			self.render_live_loupe(ui, state, CELL, hud_blur_active, hud_opaque, theme);
		}
	}

	fn sync_live_loupe_texture(&mut self, loupe: Option<&LoupeSample>) -> Option<TextureId> {
		let Some(loupe) = loupe else {
			self.live_loupe_texture = None;

			return None;
		};
		let patch_size_px = [loupe.patch.width() as usize, loupe.patch.height() as usize];
		let patch_rgba = loupe.patch.as_raw();

		match self.live_loupe_texture.as_mut() {
			Some(cached) if cached.patch_size_px == patch_size_px => {
				if cached.rgba != *patch_rgba {
					let color_image = ColorImage::from_rgba_unmultiplied(
						[patch_size_px[0], patch_size_px[1]],
						patch_rgba,
					);

					cached.texture.set(color_image, TextureOptions::NEAREST);
					cached.rgba.clone_from(patch_rgba);
				}
			},
			_ => {
				let color_image = ColorImage::from_rgba_unmultiplied(
					[patch_size_px[0], patch_size_px[1]],
					patch_rgba,
				);
				let texture = self.egui_ctx.load_texture(
					String::from("live-loupe-image"),
					color_image,
					TextureOptions::NEAREST,
				);

				self.live_loupe_texture =
					Some(LiveLoupeTexture { texture, patch_size_px, rgba: patch_rgba.clone() });
			},
		}

		self.live_loupe_texture.as_ref().map(|cached| cached.texture.id())
	}

	fn render_live_loupe(
		&mut self,
		ui: &mut Ui,
		state: &OverlayState,
		cell: f32,
		_hud_blur_active: bool,
		hud_opaque: bool,
		theme: HudTheme,
	) {
		let fallback_side_px = state.loupe_patch_side_px.max(1);
		let (w, h) = state
			.loupe
			.as_ref()
			.map(|loupe| loupe.patch.dimensions())
			.unwrap_or((fallback_side_px, fallback_side_px));
		let side = hud_helpers::stable_live_loupe_side_points(state, cell);
		let (rect, _) = ui.allocate_exact_size(Vec2::new(side, side), Sense::hover());
		let body_fill = hud_helpers::hud_body_fill_srgba8(theme, hud_opaque);
		let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 140));
		let placeholder_fill =
			Color32::from_rgba_unmultiplied(body_fill[0], body_fill[1], body_fill[2], 255);
		let image_rect =
			Rect::from_center_size(rect.center(), Vec2::new((w as f32) * cell, (h as f32) * cell));

		if let Some(texture_id) = self.sync_live_loupe_texture(state.loupe.as_ref()) {
			ui.painter().rect_filled(rect, 3.0, placeholder_fill);
			ui.painter().image(
				texture_id,
				image_rect,
				Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
				Color32::WHITE,
			);
		} else {
			ui.painter().rect_filled(rect, 3.0, placeholder_fill);
		}

		ui.painter().rect_stroke(rect, 3.0, stroke, StrokeKind::Outside);

		let center_x = (w / 2) as f32;
		let center_y = (h / 2) as f32;
		let center_min =
			Pos2::new(image_rect.min.x + center_x * cell, image_rect.min.y + center_y * cell);
		let center_rect = Rect::from_min_size(center_min, Vec2::splat(cell));

		ui.painter().rect_stroke(
			center_rect,
			0.0,
			Stroke::new(2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 180)),
			StrokeKind::Inside,
		);
	}
}
