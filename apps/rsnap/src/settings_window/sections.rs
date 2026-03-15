use std::path::PathBuf;

use egui::CollapsingHeader;
use egui::ComboBox;
use egui::Context;
use egui::DragValue;
use egui::Pos2;
use egui::Rect;
use egui::Sense;
use egui::Slider;
use egui::Stroke;
use egui::TextEdit;
use egui::Ui;
use egui::style::HandleShape;

use crate::settings::{self, AltActivationMode, AppSettings, LoupeSampleSize};
use crate::settings_window::hotkey;
use crate::settings_window::hotkey::SettingsUiHotkeyHost;
use crate::settings_window::{
	SETTINGS_HUE_SLIDER_HEIGHT, SETTINGS_HUE_SLIDER_LIGHTNESS, SETTINGS_HUE_SLIDER_SATURATION,
	SETTINGS_HUE_SLIDER_STEPS, SETTINGS_ROW_HEIGHT, SETTINGS_SECTION_GAP,
	SETTINGS_SLIDER_RAIL_HEIGHT, SETTINGS_SLIDER_WIDGET_HEIGHT, SETTINGS_VALUE_BOX_WIDTH,
	SettingsWindow, platform,
};
use rsnap_overlay::{OutputNaming, ToolbarPlacement, WindowCaptureAlphaMode};

pub(super) trait SettingsUiHost: SettingsUiHotkeyHost {
	fn combo_width(&self) -> f32;
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SettingsUiSectionDefaults {
	general: bool,
	overlay: bool,
	hotkeys: bool,
	capture: bool,
	output: bool,
	advanced: bool,
	about: bool,
}
impl SettingsUiSectionDefaults {
	pub(super) const fn standard() -> Self {
		Self {
			general: true,
			overlay: true,
			hotkeys: false,
			capture: false,
			output: false,
			advanced: false,
			about: false,
		}
	}

	pub(super) const fn all_open() -> Self {
		Self {
			general: true,
			overlay: true,
			hotkeys: true,
			capture: true,
			output: true,
			advanced: true,
			about: true,
		}
	}

	pub(super) const fn hotkeys_expanded() -> Self {
		Self {
			general: true,
			overlay: true,
			hotkeys: true,
			capture: false,
			output: false,
			advanced: false,
			about: false,
		}
	}
}

impl Default for SettingsUiSectionDefaults {
	fn default() -> Self {
		Self::standard()
	}
}

impl SettingsUiHost for SettingsWindow {
	fn combo_width(&self) -> f32 {
		self.combo_width
	}
}

pub(super) fn with_settings_density<R>(
	ui: &mut Ui,
	combo_width: f32,
	add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
	ui.scope(|ui| {
		let spacing = ui.spacing_mut();

		spacing.item_spacing = egui::vec2(8.0, 4.0);
		spacing.button_padding = egui::vec2(4.0, 1.0);
		spacing.interact_size.y = SETTINGS_ROW_HEIGHT;
		spacing.combo_width = combo_width;
		spacing.slider_width = combo_width;
		spacing.slider_rail_height = SETTINGS_SLIDER_RAIL_HEIGHT;

		add_contents(ui)
	})
	.inner
}

pub(super) fn render_all_sections(
	host: &mut impl SettingsUiHost,
	ui: &mut Ui,
	ctx: &Context,
	settings: &mut AppSettings,
) -> bool {
	render_all_sections_with_defaults(host, ui, ctx, settings, SettingsUiSectionDefaults::default())
}

pub(super) fn render_all_sections_with_defaults(
	host: &mut impl SettingsUiHost,
	ui: &mut Ui,
	ctx: &Context,
	settings: &mut AppSettings,
	defaults: SettingsUiSectionDefaults,
) -> bool {
	let combo_width = host.combo_width();
	let mut changed = false;

	CollapsingHeader::new("General").default_open(defaults.general).show(ui, |ui| {
		changed |= render_general_section(combo_width, ui, ctx, settings);
	});

	ui.add_space(SETTINGS_SECTION_GAP);

	CollapsingHeader::new("Overlay").default_open(defaults.overlay).show(ui, |ui| {
		changed |= render_overlay_section(combo_width, ui, settings);
	});

	ui.add_space(SETTINGS_SECTION_GAP);

	CollapsingHeader::new("Hotkeys").default_open(defaults.hotkeys).show(ui, |ui| {
		changed |= hotkey::render_hotkeys_section(host, ui, settings);
	});

	ui.add_space(SETTINGS_SECTION_GAP);

	CollapsingHeader::new("Capture").default_open(defaults.capture).show(ui, |ui| {
		changed |= render_capture_section(combo_width, ui, settings);
	});

	ui.add_space(SETTINGS_SECTION_GAP);

	CollapsingHeader::new("Output").default_open(defaults.output).show(ui, |ui| {
		changed |= render_output_section(combo_width, ui, settings);
	});

	ui.add_space(SETTINGS_SECTION_GAP);

	CollapsingHeader::new("Advanced").default_open(defaults.advanced).show(ui, |ui| {
		ui.label("Advanced options are coming soon.");
	});

	ui.add_space(SETTINGS_SECTION_GAP);

	CollapsingHeader::new("About").default_open(defaults.about).show(ui, |ui| {
		ui.label(format!("rsnap {}", env!("CARGO_PKG_VERSION")));
	});

	changed
}

fn render_capture_section(combo_width: f32, ui: &mut Ui, settings: &mut AppSettings) -> bool {
	let previous_alpha_mode = settings.window_capture_alpha_mode;
	let mut changed = false;

	ComboBox::from_label("Window background")
		.selected_text(match settings.window_capture_alpha_mode {
			WindowCaptureAlphaMode::Background => "Background (match screen)",
			WindowCaptureAlphaMode::MatteLight => "Matte light",
			WindowCaptureAlphaMode::MatteDark => "Matte dark",
		})
		.width(combo_width)
		.show_ui(ui, |ui| {
			ui.selectable_value(
				&mut settings.window_capture_alpha_mode,
				WindowCaptureAlphaMode::Background,
				"Background (match screen)",
			);
			ui.selectable_value(
				&mut settings.window_capture_alpha_mode,
				WindowCaptureAlphaMode::MatteLight,
				"Matte light",
			);
			ui.selectable_value(
				&mut settings.window_capture_alpha_mode,
				WindowCaptureAlphaMode::MatteDark,
				"Matte dark",
			);
		});

	if settings.window_capture_alpha_mode != previous_alpha_mode {
		changed = true;
	}

	ui.small("Applies to window-lock capture preview and export.");
	ui.small("Background matches region-style capture inside the window bounds.");
	ui.small("Matte modes flatten transparency onto a solid background.");

	changed
}

fn render_output_section(combo_width: f32, ui: &mut Ui, settings: &mut AppSettings) -> bool {
	let row_height = ui.spacing().interact_size.y;
	let value_width = ui.spacing().slider_width;
	let mut changed = false;
	let mut output_dir = settings.output_dir.to_string_lossy().to_string();

	ui.horizontal(|ui| {
		let dir_response = ui.add_sized(
			egui::vec2(value_width, row_height),
			TextEdit::singleline(&mut output_dir).hint_text("~/Desktop"),
		);

		if dir_response.changed() {
			let trimmed = output_dir.trim();

			settings.output_dir = if trimmed.is_empty() {
				AppSettings::default().output_dir
			} else {
				PathBuf::from(trimmed)
			};
			changed = true;
		}

		dir_response.on_hover_text("Directory where Save writes PNG files.");
		ui.label("Output directory");
	});

	let mut prefix = settings.output_filename_prefix.clone();

	ui.horizontal(|ui| {
		let prefix_response = ui.add_sized(
			egui::vec2(value_width, row_height),
			TextEdit::singleline(&mut prefix).hint_text("rsnap"),
		);

		if prefix_response.changed() {
			settings.output_filename_prefix = settings::sanitize_output_filename_prefix(&prefix);
			changed = true;
		}

		prefix_response.on_hover_text("Filename prefix used for saved captures.");
		ui.label("Filename prefix");
	});

	let previous_naming = settings.output_naming;

	ComboBox::from_label("Filename naming")
		.selected_text(match settings.output_naming {
			OutputNaming::Timestamp => "Timestamp (unix ms)",
			OutputNaming::Sequence => "Sequence (0001)",
		})
		.width(combo_width)
		.show_ui(ui, |ui| {
			ui.selectable_value(
				&mut settings.output_naming,
				OutputNaming::Timestamp,
				"Timestamp (unix ms)",
			);
			ui.selectable_value(
				&mut settings.output_naming,
				OutputNaming::Sequence,
				"Sequence (0001)",
			);
		});

	if settings.output_naming != previous_naming {
		changed = true;
	}

	ui.small(format!(
		"Space/Copy -> clipboard. {}/Save -> write PNG to output directory.",
		platform::save_shortcut_label()
	));

	changed
}

fn render_general_section(
	combo_width: f32,
	ui: &mut Ui,
	_ctx: &Context,
	settings: &mut AppSettings,
) -> bool {
	let mut changed = false;

	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	enum LogLevelPreset {
		DefaultInfo,
		Warn,
		DebugRsn,
		TraceRsn,
		Custom,
	}

	let log_filter_current = settings.log_filter.clone();
	let (current_preset, current_custom) = match log_filter_current.as_deref() {
		None => (LogLevelPreset::DefaultInfo, None),
		Some("warn") => (LogLevelPreset::Warn, None),
		Some("rsnap=debug,rsnap_overlay=debug") => (LogLevelPreset::DebugRsn, None),
		Some("rsnap=trace,rsnap_overlay=trace") => (LogLevelPreset::TraceRsn, None),
		Some(other) => (LogLevelPreset::Custom, Some(other.to_owned())),
	};
	let mut selected_preset = current_preset;

	ComboBox::from_label("Log level")
		.selected_text(match selected_preset {
			LogLevelPreset::DefaultInfo => "Default (rsnap info)",
			LogLevelPreset::Warn => "Warn",
			LogLevelPreset::DebugRsn => "Debug (rsnap + overlay)",
			LogLevelPreset::TraceRsn => "Trace (rsnap + overlay)",
			LogLevelPreset::Custom => "Custom…",
		})
		.width(combo_width)
		.show_ui(ui, |ui| {
			ui.selectable_value(
				&mut selected_preset,
				LogLevelPreset::DefaultInfo,
				"Default (rsnap info)",
			);
			ui.selectable_value(&mut selected_preset, LogLevelPreset::Warn, "Warn");
			ui.selectable_value(
				&mut selected_preset,
				LogLevelPreset::DebugRsn,
				"Debug (rsnap + overlay)",
			);
			ui.selectable_value(
				&mut selected_preset,
				LogLevelPreset::TraceRsn,
				"Trace (rsnap + overlay)",
			);
			ui.selectable_value(&mut selected_preset, LogLevelPreset::Custom, "Custom…");
		});

	if selected_preset != current_preset {
		settings.log_filter = match selected_preset {
			LogLevelPreset::DefaultInfo => None,
			LogLevelPreset::Warn => Some(String::from("warn")),
			LogLevelPreset::DebugRsn => Some(String::from("rsnap=debug,rsnap_overlay=debug")),
			LogLevelPreset::TraceRsn => Some(String::from("rsnap=trace,rsnap_overlay=trace")),
			LogLevelPreset::Custom => settings.log_filter.clone().or_else(|| Some(String::new())),
		};
		changed = true;
	}
	if selected_preset == LogLevelPreset::Custom {
		let mut custom = current_custom.unwrap_or_default();
		let response = ui
			.add(TextEdit::singleline(&mut custom).hint_text("rsnap=debug,rsnap_overlay=debug"))
			.on_hover_text("Uses the same syntax as RUST_LOG (tracing-subscriber EnvFilter).");

		if response.changed() {
			settings.log_filter = Some(custom);
			changed = true;
		}
	}

	ui.small("Log level changes require restarting rsnap.");

	changed
}

fn render_overlay_section(combo_width: f32, ui: &mut Ui, settings: &mut AppSettings) -> bool {
	let mut changed = false;

	changed |= ui.checkbox(&mut settings.show_alt_hint_keycap, "Show Alt hint in HUD").changed();
	changed |= ui.checkbox(&mut settings.hud_glass_enabled, "Glass HUD").changed();
	changed |= ui.checkbox(&mut settings.selection_particles, "Selection particles").changed();
	changed |= overlay_range_slider_row(
		ui,
		"Flow thickness",
		&mut settings.selection_flow_stroke_width_px,
		settings.selection_particles,
	);

	ui.add_space(SETTINGS_SECTION_GAP);
	ui.separator();
	ui.add_space(SETTINGS_SECTION_GAP);

	let before_alt = settings.alt_activation;

	ComboBox::from_label("Alt activation")
		.selected_text(alt_activation_label(settings.alt_activation))
		.width(combo_width)
		.show_ui(ui, |ui| {
			ui.selectable_value(&mut settings.alt_activation, AltActivationMode::Hold, "Hold");
			ui.selectable_value(&mut settings.alt_activation, AltActivationMode::Toggle, "Toggle");
		});

	if settings.alt_activation != before_alt {
		changed = true;
	}

	let before_loupe = settings.loupe_sample_size;

	ComboBox::from_label("Loupe sample size")
		.selected_text(loupe_sample_size_label(settings.loupe_sample_size))
		.width(combo_width)
		.show_ui(ui, |ui| {
			ui.selectable_value(
				&mut settings.loupe_sample_size,
				LoupeSampleSize::Small,
				"Small (15x15)",
			);
			ui.selectable_value(
				&mut settings.loupe_sample_size,
				LoupeSampleSize::Medium,
				"Medium (21x21)",
			);
			ui.selectable_value(
				&mut settings.loupe_sample_size,
				LoupeSampleSize::Large,
				"Large (31x31)",
			);
		});

	if settings.loupe_sample_size != before_loupe {
		changed = true;
	}

	let before_toolbar_placement = settings.toolbar_placement;

	ComboBox::from_label("Toolbar placement")
		.selected_text(toolbar_placement_label(settings.toolbar_placement))
		.width(combo_width)
		.show_ui(ui, |ui| {
			ui.selectable_value(
				&mut settings.toolbar_placement,
				ToolbarPlacement::Bottom,
				"Bottom",
			);
			ui.selectable_value(&mut settings.toolbar_placement, ToolbarPlacement::Top, "Top");
		});

	if settings.toolbar_placement != before_toolbar_placement {
		changed = true;
	}

	let enabled = settings.hud_glass_enabled;

	changed |= overlay_slider_row(ui, "Opacity", &mut settings.hud_opacity, enabled);
	changed |= overlay_slider_row(ui, "Blur", &mut settings.hud_blur, enabled);
	changed |= overlay_slider_row(ui, "Tint", &mut settings.hud_tint, enabled);
	changed |= overlay_hue_slider_row(ui, "Hue", &mut settings.hud_tint_hue, enabled);

	changed
}

fn overlay_slider_row(ui: &mut Ui, label: &str, amount: &mut f32, enabled: bool) -> bool {
	let mut changed = false;
	let mut value = (*amount).clamp(0.0, 1.0);
	let mut percent = (value * 100.0).round() as i32;

	percent = percent.clamp(0, 100);
	ui.horizontal(|ui| {
		let slider = Slider::new(&mut value, 0.0..=1.0)
			.handle_shape(HandleShape::Circle)
			.show_value(false)
			.text("");
		let slider_response = ui
			.add_enabled_ui(enabled, |ui| {
				ui.scope(|ui| {
					ui.spacing_mut().interact_size.y = SETTINGS_SLIDER_WIDGET_HEIGHT;

					ui.add(slider)
				})
				.inner
			})
			.inner;

		changed |= slider_response.changed();

		let percent_changed = ui
			.add_enabled_ui(enabled, |ui| {
				ui.add_sized(
					egui::vec2(SETTINGS_VALUE_BOX_WIDTH, ui.spacing().interact_size.y),
					DragValue::new(&mut percent)
						.range(0..=100)
						.speed(1.0)
						.suffix("%")
						.custom_parser(|text| {
							let text = text.trim();
							let text = text.strip_suffix('%').unwrap_or(text).trim();

							text.parse::<i32>().ok().map(f64::from)
						}),
				)
			})
			.inner
			.changed();

		if percent_changed {
			value = (percent as f32 / 100.0).clamp(0.0, 1.0);
			changed = true;
		}

		ui.label(label);
	});

	if (value - *amount).abs() > f32::EPSILON {
		*amount = value;

		true
	} else {
		changed
	}
}

fn overlay_range_slider_row(ui: &mut Ui, label: &str, amount: &mut f32, enabled: bool) -> bool {
	let mut changed = false;
	let mut value = (*amount).clamp(1.0, 8.0);

	ui.horizontal(|ui| {
		let slider_response = ui
			.add_enabled_ui(enabled, |ui| {
				ui.scope(|ui| {
					ui.spacing_mut().interact_size.y = SETTINGS_SLIDER_WIDGET_HEIGHT;

					ui.add(
						Slider::new(&mut value, 1.0..=8.0)
							.step_by(0.1)
							.handle_shape(HandleShape::Circle)
							.show_value(false)
							.text(""),
					)
				})
				.inner
			})
			.inner;

		changed |= slider_response.changed();

		let value_changed = ui
			.add_enabled_ui(enabled, |ui| {
				ui.add_sized(
					egui::vec2(SETTINGS_VALUE_BOX_WIDTH, ui.spacing().interact_size.y),
					DragValue::new(&mut value).range(1.0..=8.0).speed(0.1).fixed_decimals(1),
				)
			})
			.inner
			.changed();

		if value_changed {
			changed = true;
		}

		ui.label(label);
	});

	let snapped = (value * 10.0).round() / 10.0;

	if (snapped - *amount).abs() > f32::EPSILON {
		*amount = snapped;

		true
	} else {
		changed
	}
}

fn overlay_hue_slider_row(ui: &mut Ui, label: &str, hue: &mut f32, enabled: bool) -> bool {
	let mut changed = false;
	let mut current_hue = hue.clamp(0.0, 1.0);
	let mut hue_degrees = (current_hue * 360.0).round().clamp(0.0, 360.0);

	ui.horizontal(|ui| {
		let bar_height = SETTINGS_HUE_SLIDER_HEIGHT.max(SETTINGS_SLIDER_RAIL_HEIGHT);
		let bar_width = ui.spacing().slider_width;
		let (bar_rect, response) =
			ui.allocate_exact_size(egui::vec2(bar_width, bar_height), Sense::click_and_drag());

		if enabled
			&& (response.clicked() || response.dragged())
			&& let Some(pointer) = response.interact_pointer_pos()
		{
			let ratio = (pointer.x - bar_rect.left()) / bar_rect.width();
			let next_hue = ratio.clamp(0.0, 1.0);

			if (next_hue - current_hue).abs() > f32::EPSILON {
				current_hue = next_hue;
				hue_degrees = (current_hue * 360.0).round();
				changed = true;
			}
		}

		let step_width = bar_rect.width() / SETTINGS_HUE_SLIDER_STEPS as f32;

		for step in 0..SETTINGS_HUE_SLIDER_STEPS {
			let left = bar_rect.left() + (step as f32 * step_width);
			let right = (left + step_width).min(bar_rect.right());
			let step_rect = Rect::from_min_max(
				Pos2::new(left, bar_rect.top()),
				Pos2::new(right, bar_rect.bottom()),
			);
			let step_hue = if step == SETTINGS_HUE_SLIDER_STEPS - 1 {
				1.0
			} else {
				step as f32 / SETTINGS_HUE_SLIDER_STEPS as f32
			};
			let color = hsl_to_color32(
				step_hue,
				SETTINGS_HUE_SLIDER_SATURATION,
				SETTINGS_HUE_SLIDER_LIGHTNESS,
			);

			ui.painter().rect_filled(step_rect, 0.0, color);
		}

		let handle_x = (bar_rect.left() + current_hue * bar_rect.width())
			.clamp(bar_rect.left(), bar_rect.right());
		let handle = Pos2::new(handle_x, bar_rect.center().y);
		let handle_color = hsl_to_color32(
			current_hue,
			SETTINGS_HUE_SLIDER_SATURATION,
			SETTINGS_HUE_SLIDER_LIGHTNESS,
		);

		ui.painter().circle_filled(handle, 6.0, handle_color);
		ui.painter().circle_stroke(handle, 6.0, Stroke::new(1.0, egui::Color32::from_gray(220)));

		let value_changed = ui
			.add_enabled_ui(enabled, |ui| {
				ui.add_sized(
					egui::vec2(SETTINGS_VALUE_BOX_WIDTH, ui.spacing().interact_size.y),
					DragValue::new(&mut hue_degrees)
						.range(0.0..=360.0)
						.fixed_decimals(0)
						.suffix("°"),
				)
			})
			.inner
			.changed();

		if value_changed {
			let next_hue = (hue_degrees / 360.0).clamp(0.0, 1.0);

			if (next_hue - current_hue).abs() > f32::EPSILON {
				current_hue = next_hue;
				changed = true;
			}
		}

		ui.label(label);
	});

	if (*hue - current_hue).abs() > f32::EPSILON {
		*hue = current_hue;

		true
	} else {
		changed
	}
}

fn hsl_to_color32(hue: f32, saturation: f32, lightness: f32) -> egui::Color32 {
	let hue = hue.rem_euclid(1.0);
	let saturation = saturation.clamp(0.0, 1.0);
	let lightness = lightness.clamp(0.0, 1.0);

	if saturation <= 0.0 {
		let value = (lightness * 255.0).round().clamp(0.0, 255.0) as u8;

		return egui::Color32::from_rgb(value, value, value);
	}

	let q = if lightness < 0.5 {
		lightness * (1.0 + saturation)
	} else {
		lightness + saturation - lightness * saturation
	};
	let p = 2.0 * lightness - q;
	let red = hue_to_rgb(p, q, hue + 1.0 / 3.0);
	let green = hue_to_rgb(p, q, hue);
	let blue = hue_to_rgb(p, q, hue - 1.0 / 3.0);
	let r = (red * 255.0).round().clamp(0.0, 255.0) as u8;
	let g = (green * 255.0).round().clamp(0.0, 255.0) as u8;
	let b = (blue * 255.0).round().clamp(0.0, 255.0) as u8;

	egui::Color32::from_rgb(r, g, b)
}

fn hue_to_rgb(p: f32, q: f32, hue: f32) -> f32 {
	let normalized_hue = hue.rem_euclid(1.0);

	if normalized_hue < 1.0 / 6.0 {
		return p + (q - p) * 6.0 * normalized_hue;
	}
	if normalized_hue < 1.0 / 2.0 {
		return q;
	}
	if normalized_hue < 2.0 / 3.0 {
		return p + (q - p) * (2.0 / 3.0 - normalized_hue) * 6.0;
	}

	p
}

fn alt_activation_label(mode: AltActivationMode) -> &'static str {
	match mode {
		AltActivationMode::Hold => "Hold",
		AltActivationMode::Toggle => "Toggle",
	}
}

fn loupe_sample_size_label(size: LoupeSampleSize) -> &'static str {
	match size {
		LoupeSampleSize::Small => "Small (15x15)",
		LoupeSampleSize::Medium => "Medium (21x21)",
		LoupeSampleSize::Large => "Large (31x31)",
	}
}

fn toolbar_placement_label(placement: ToolbarPlacement) -> &'static str {
	match placement {
		ToolbarPlacement::Top => "Top",
		ToolbarPlacement::Bottom => "Bottom",
	}
}
