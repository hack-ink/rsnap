use std::path::PathBuf;
use std::time::Duration;

use egui::CentralPanel;
use egui::Context;
use egui::Pos2;
use egui::Rect;
use egui::ScrollArea;
use egui::Vec2;
use egui::ViewportId;
use egui::Visuals;
use egui::epaint::{ClippedPrimitive, Primitive};
use rsnap_overlay::{OutputNaming, ThemeMode, ToolbarPlacement, WindowCaptureAlphaMode};
use winit::keyboard::ModifiersState;

use crate::settings::{AltActivationMode, AppSettings, LoupeSampleSize};

use super::CaptureHotkeyNotice;
use super::SETTINGS_COMBO_WIDTH;
use super::hotkey::SettingsUiHotkeyHost;
use super::sections::{self, SettingsUiHost, SettingsUiSectionDefaults};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsUiBenchScenario {
	Default,
	ExpandedAll,
	HotkeyRecording,
}

impl SettingsUiBenchScenario {
	pub const ALL: [Self; 3] = [Self::Default, Self::ExpandedAll, Self::HotkeyRecording];

	#[must_use]
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Default => "default",
			Self::ExpandedAll => "expanded_all",
			Self::HotkeyRecording => "hotkey_recording",
		}
	}

	const fn section_defaults(self) -> SettingsUiSectionDefaults {
		match self {
			Self::Default => SettingsUiSectionDefaults::standard(),
			Self::ExpandedAll => SettingsUiSectionDefaults::all_open(),
			Self::HotkeyRecording => SettingsUiSectionDefaults::hotkeys_expanded(),
		}
	}
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SettingsUiLayoutMetrics {
	pub shape_count: usize,
	pub texture_upload_count: usize,
	pub texture_free_count: usize,
	pub repaint_delay_micros: u128,
	pub settings_changed: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SettingsUiFrameMetrics {
	pub shape_count: usize,
	pub clipped_primitive_count: usize,
	pub mesh_count: usize,
	pub callback_primitive_count: usize,
	pub vertex_count: usize,
	pub index_count: usize,
	pub texture_upload_count: usize,
	pub texture_free_count: usize,
	pub settings_changed: bool,
}

pub struct SettingsUiBenchHarness {
	ctx: Context,
	frame_index: u64,
	host: BenchSettingsUiHost,
	settings: AppSettings,
	section_defaults: SettingsUiSectionDefaults,
	screen_size_points: Vec2,
	pixels_per_point: f32,
	max_texture_side: usize,
}

impl SettingsUiBenchHarness {
	#[must_use]
	pub fn new(scenario: SettingsUiBenchScenario) -> Self {
		let ctx = Context::default();

		ctx.set_visuals(Visuals::dark());

		Self {
			ctx,
			frame_index: 0,
			host: BenchSettingsUiHost::for_scenario(scenario),
			settings: settings_for_scenario(scenario),
			section_defaults: scenario.section_defaults(),
			screen_size_points: egui::vec2(720.0, 720.0),
			pixels_per_point: 2.0,
			max_texture_side: 4096,
		}
	}

	#[must_use]
	pub fn run_layout(&mut self) -> SettingsUiLayoutMetrics {
		let (full_output, shape_count, settings_changed) = self.run_full_output();
		let repaint_delay_micros = full_output
			.viewport_output
			.get(&ViewportId::ROOT)
			.map(|viewport_output| viewport_output.repaint_delay.as_micros())
			.unwrap_or(Duration::ZERO.as_micros());

		SettingsUiLayoutMetrics {
			shape_count,
			texture_upload_count: full_output.textures_delta.set.len(),
			texture_free_count: full_output.textures_delta.free.len(),
			repaint_delay_micros,
			settings_changed,
		}
	}

	#[must_use]
	pub fn run_frame(&mut self) -> SettingsUiFrameMetrics {
		let (full_output, shape_count, settings_changed) = self.run_full_output();
		let texture_upload_count = full_output.textures_delta.set.len();
		let texture_free_count = full_output.textures_delta.free.len();
		let primitives = self.ctx.tessellate(full_output.shapes, self.pixels_per_point);
		let (mesh_count, callback_primitive_count, vertex_count, index_count) =
			primitive_stats(&primitives);

		SettingsUiFrameMetrics {
			shape_count,
			clipped_primitive_count: primitives.len(),
			mesh_count,
			callback_primitive_count,
			vertex_count,
			index_count,
			texture_upload_count,
			texture_free_count,
			settings_changed,
		}
	}

	fn raw_input(&self) -> egui::RawInput {
		let mut raw_input = egui::RawInput {
			screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.screen_size_points)),
			focused: true,
			time: Some(self.frame_index as f64 / 120.0),
			predicted_dt: 1.0 / 120.0,
			..Default::default()
		};

		raw_input.max_texture_side = Some(self.max_texture_side);

		if let Some(viewport) = raw_input.viewports.get_mut(&ViewportId::ROOT) {
			viewport.native_pixels_per_point = Some(self.pixels_per_point);
			viewport.inner_rect = raw_input.screen_rect;
			viewport.focused = Some(true);
		}

		raw_input
	}

	fn run_full_output(&mut self) -> (egui::FullOutput, usize, bool) {
		self.frame_index += 1;
		self.ctx.input_mut(|input| input.max_texture_side = self.max_texture_side);

		let raw_input = self.raw_input();
		let combo_width = self.host.combo_width;
		let section_defaults = self.section_defaults;
		let mut settings_changed = false;
		let host = &mut self.host;
		let settings = &mut self.settings;
		let full_output = self.ctx.run(raw_input, |ctx| {
			CentralPanel::default().show(ctx, |ui| {
				sections::with_settings_density(ui, combo_width, |ui| {
					ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
						settings_changed |= sections::render_all_sections_with_defaults(
							host,
							ui,
							ctx,
							settings,
							section_defaults,
						);
					});
				});
			});
		});
		let shape_count = full_output.shapes.len();

		(full_output, shape_count, settings_changed)
	}
}

struct BenchSettingsUiHost {
	combo_width: f32,
	capture_hotkey_recording: bool,
	capture_hotkey_notice: Option<CaptureHotkeyNotice>,
	modifiers: ModifiersState,
}

impl BenchSettingsUiHost {
	fn for_scenario(scenario: SettingsUiBenchScenario) -> Self {
		match scenario {
			SettingsUiBenchScenario::Default | SettingsUiBenchScenario::ExpandedAll => Self {
				combo_width: SETTINGS_COMBO_WIDTH,
				capture_hotkey_recording: false,
				capture_hotkey_notice: None,
				modifiers: ModifiersState::default(),
			},
			SettingsUiBenchScenario::HotkeyRecording => Self {
				combo_width: SETTINGS_COMBO_WIDTH,
				capture_hotkey_recording: true,
				capture_hotkey_notice: Some(CaptureHotkeyNotice::Hint(String::from(
					super::hotkey::CAPTURE_HOTKEY_GUIDANCE_PRESS_NONMOD,
				))),
				modifiers: ModifiersState::default(),
			},
		}
	}
}

impl SettingsUiHotkeyHost for BenchSettingsUiHost {
	fn capture_hotkey_recording(&self) -> bool {
		self.capture_hotkey_recording
	}

	fn capture_hotkey_notice(&self) -> Option<&CaptureHotkeyNotice> {
		self.capture_hotkey_notice.as_ref()
	}

	fn modifiers(&self) -> &ModifiersState {
		&self.modifiers
	}

	fn begin_recording_capture_hotkey(&mut self) {
		self.capture_hotkey_recording = true;
		self.capture_hotkey_notice = None;
	}

	fn cancel_recording_capture_hotkey(&mut self) {
		self.capture_hotkey_recording = false;
		self.capture_hotkey_notice = None;
	}
}

impl SettingsUiHost for BenchSettingsUiHost {
	fn combo_width(&self) -> f32 {
		self.combo_width
	}
}

fn settings_for_scenario(scenario: SettingsUiBenchScenario) -> AppSettings {
	let mut settings = AppSettings::default();

	if matches!(
		scenario,
		SettingsUiBenchScenario::ExpandedAll | SettingsUiBenchScenario::HotkeyRecording
	) {
		settings.show_alt_hint_keycap = false;
		settings.hud_glass_enabled = true;
		settings.capture_hotkey = String::from("Alt+Shift+X");
		settings.hud_opacity = 0.72;
		settings.hud_blur = 0.34;
		settings.hud_tint = 0.68;
		settings.hud_tint_hue = 0.88;
		settings.alt_activation = AltActivationMode::Toggle;
		settings.selection_particles = true;
		settings.selection_flow_stroke_width_px = 6.4;
		settings.log_filter = Some(String::from("rsnap=trace,rsnap_overlay=trace"));
		settings.output_dir =
			PathBuf::from("/Users/example/Library/Application Support/rsnap/captures");
		settings.output_filename_prefix = String::from("release_candidate_capture");
		settings.output_naming = OutputNaming::Sequence;
		settings.window_capture_alpha_mode = WindowCaptureAlphaMode::MatteDark;
		settings.toolbar_placement = ToolbarPlacement::Top;
		settings.loupe_sample_size = LoupeSampleSize::Large;
		settings.theme_mode = ThemeMode::Dark;
	}

	settings
}

fn primitive_stats(primitives: &[ClippedPrimitive]) -> (usize, usize, usize, usize) {
	let mut mesh_count = 0;
	let mut callback_primitive_count = 0;
	let mut vertex_count = 0;
	let mut index_count = 0;

	for primitive in primitives {
		match &primitive.primitive {
			Primitive::Mesh(mesh) => {
				mesh_count += 1;
				vertex_count += mesh.vertices.len();
				index_count += mesh.indices.len();
			},
			Primitive::Callback(_) => {
				callback_primitive_count += 1;
			},
		}
	}

	(mesh_count, callback_primitive_count, vertex_count, index_count)
}

#[cfg(test)]
mod tests {
	use super::{SettingsUiBenchHarness, SettingsUiBenchScenario};

	#[test]
	fn expanded_settings_benchmark_harness_produces_tessellated_output() {
		let mut harness = SettingsUiBenchHarness::new(SettingsUiBenchScenario::ExpandedAll);
		let layout_metrics = harness.run_layout();
		let frame_metrics = harness.run_frame();

		assert!(layout_metrics.shape_count > 0);
		assert!(frame_metrics.clipped_primitive_count > 0);
		assert!(frame_metrics.vertex_count > 0);
		assert!(frame_metrics.index_count > 0);
	}
}
