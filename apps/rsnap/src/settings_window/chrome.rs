use egui::{self, Align, Layout, Ui};
use winit::dpi::LogicalSize;
use winit::window::Theme;

use crate::settings::AppSettings;
use rsnap_overlay::ThemeMode;

use super::{
	SETTINGS_SECTION_GAP, SETTINGS_THEME_ICON_SIZE, SETTINGS_TITLEBAR_HEIGHT, SettingsWindow,
	platform,
};

impl SettingsWindow {
	pub(super) fn ui(&mut self, ctx: &egui::Context, settings: &mut AppSettings) -> bool {
		self.sync_theme(ctx, settings.theme_mode);
		self.maybe_autosize_window(ctx);

		let mut changed = false;

		egui::CentralPanel::default().show(ctx, |ui| {
			let combo_width = self.combo_width;

			Self::with_settings_density(ui, combo_width, |ui| {
				changed |= self.render_titlebar_controls(ui, ctx, settings);
				egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
					changed |= self.render_all_sections(ui, ctx, settings);
				});
			});
		});

		changed
	}

	fn maybe_autosize_window(&mut self, ctx: &egui::Context) {
		if self.did_autosize {
			return;
		}

		let font_id = egui::TextStyle::Body.resolve(&ctx.style());
		let measure = |text: &str| -> f32 {
			ctx.fonts_mut(|fonts| {
				fonts
					.layout_no_wrap(text.to_owned(), font_id.clone(), egui::Color32::WHITE)
					.size()
					.x
			})
		};
		let max_label = [
			"Capture hotkey",
			"Log level",
			"Output directory",
			"Filename prefix",
			"Filename naming",
			"Show Alt hint in HUD",
			"Glass HUD",
			"Selection particles",
			"Flow thickness",
			"Alt activation",
			"Loupe sample size",
			"Opacity",
			"Blur",
			"Tint",
			"Theme",
		]
		.into_iter()
		.map(measure)
		.fold(0.0_f32, f32::max);
		let max_combo_value = [
			"Default (rsnap info)",
			"Warn",
			"Debug (rsnap + overlay)",
			"Trace (rsnap + overlay)",
			"Custom…",
			"System",
			"Dark",
			"Light",
			"Hold",
			"Toggle",
			"Small (15x15)",
			"Medium (21x21)",
			"Large (31x31)",
			"Timestamp (unix ms)",
			"Sequence (0001)",
		]
		.into_iter()
		.map(measure)
		.fold(0.0_f32, f32::max);
		// Rough padding for combo box arrow + inner margins.
		let combo_width = (max_combo_value + 56.0).clamp(160.0, 360.0);
		let row_width = max_label + 8.0 + combo_width;
		// Outer padding + some slack so nothing feels cramped.
		let target_width = (row_width + 56.0).clamp(420.0, 720.0);
		let height = self.window.inner_size().height.max(1) as f64 / self.window.scale_factor();
		let _ = self.window.request_inner_size(LogicalSize::new(f64::from(target_width), height));

		self.combo_width = combo_width;
		self.did_autosize = true;
	}

	fn render_titlebar_controls(
		&mut self,
		ui: &mut Ui,
		ctx: &egui::Context,
		settings: &mut AppSettings,
	) -> bool {
		let bar_width = ui.available_width();
		let (_id, bar_rect) = ui.allocate_space(egui::vec2(bar_width, SETTINGS_TITLEBAR_HEIGHT));

		platform::install_titlebar_drag(ui, bar_rect, self.window.as_ref());

		ui.painter().rect_filled(bar_rect, 0.0, ui.visuals().panel_fill);

		let row_height = ui.spacing().interact_size.y;
		let y_pad = ((bar_rect.height() - row_height) * 0.5).round();
		let theme_y = (bar_rect.min.y + y_pad + platform::theme_buttons_y_offset())
			.clamp(bar_rect.min.y, bar_rect.max.y - row_height);
		let theme_rect = egui::Rect::from_min_size(
			egui::pos2(bar_rect.min.x, theme_y),
			egui::vec2(bar_rect.width(), row_height),
		);
		let mut changed = false;

		ui.scope_builder(egui::UiBuilder::new().max_rect(theme_rect), |ui| {
			changed |= self.render_theme_mode_buttons(ui, ctx, settings);
		});

		ui.add_space(SETTINGS_SECTION_GAP);

		changed
	}

	fn render_theme_mode_buttons(
		&mut self,
		ui: &mut Ui,
		ctx: &egui::Context,
		settings: &mut AppSettings,
	) -> bool {
		let row_height = ui.spacing().interact_size.y;
		let mut changed = false;

		ui.allocate_ui_with_layout(
			egui::vec2(ui.available_width(), row_height),
			Layout::right_to_left(Align::Center),
			|ui| {
				let before = settings.theme_mode;

				// Render in reverse order for RTL layout so visible order is Light / Dark / System.
				ui.selectable_value(
					&mut settings.theme_mode,
					ThemeMode::System,
					egui::RichText::new(&self.theme_icon_system).size(SETTINGS_THEME_ICON_SIZE),
				)
				.on_hover_text("System");
				ui.selectable_value(
					&mut settings.theme_mode,
					ThemeMode::Dark,
					egui::RichText::new(&self.theme_icon_dark).size(SETTINGS_THEME_ICON_SIZE),
				)
				.on_hover_text("Dark");
				ui.selectable_value(
					&mut settings.theme_mode,
					ThemeMode::Light,
					egui::RichText::new(&self.theme_icon_light).size(SETTINGS_THEME_ICON_SIZE),
				)
				.on_hover_text("Light");

				if settings.theme_mode != before {
					self.sync_theme(ctx, settings.theme_mode);

					changed = true;
				}
			},
		);

		changed
	}

	fn requested_window_theme(mode: ThemeMode) -> Option<Theme> {
		match mode {
			ThemeMode::System => None,
			ThemeMode::Dark => Some(Theme::Dark),
			ThemeMode::Light => Some(Theme::Light),
		}
	}

	fn effective_theme(&self, mode: ThemeMode) -> Theme {
		match mode {
			ThemeMode::System => self.window.theme().unwrap_or(Theme::Dark),
			ThemeMode::Dark => Theme::Dark,
			ThemeMode::Light => Theme::Light,
		}
	}

	fn sync_theme(&mut self, ctx: &egui::Context, mode: ThemeMode) {
		let requested = Self::requested_window_theme(mode);

		if requested != self.requested_theme {
			self.window.set_theme(requested);

			self.requested_theme = requested;
		}

		let effective = self.effective_theme(mode);

		if Some(effective) != self.effective_theme {
			match effective {
				Theme::Dark => ctx.set_visuals(egui::Visuals::dark()),
				Theme::Light => ctx.set_visuals(egui::Visuals::light()),
			}

			self.effective_theme = Some(effective);
		}
	}
}
