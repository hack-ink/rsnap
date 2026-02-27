use std::sync::Arc;
use std::time::{Duration, Instant};

use color_eyre::eyre::{self, Result, WrapErr};
use egui::Ui;
use egui::{Align, Layout};
use egui_phosphor::{Variant, regular};
use egui_wgpu::{Renderer, ScreenDescriptor};
use wgpu::SurfaceTexture;
use wgpu::TextureFormat;
use wgpu::{Adapter, CompositeAlphaMode, Device, Queue, Surface, SurfaceCapabilities};
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::ElementState;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState};
use winit::window::Theme;
use winit::window::{Window, WindowId};

use crate::settings::{AltActivationMode, AppSettings, LoupeSampleSize};
use rsnap_overlay::ThemeMode;

const SETTINGS_ROW_HEIGHT: f32 = 22.0;
const SETTINGS_SECTION_GAP: f32 = 6.0;
const SETTINGS_COMBO_WIDTH: f32 = 220.0;
const SETTINGS_SLIDER_RAIL_HEIGHT: f32 = 4.0;
const SETTINGS_TITLEBAR_HEIGHT: f32 = 28.0;
const SETTINGS_THEME_ICON_SIZE: f32 = 16.0;
#[cfg(target_os = "macos")]
const SETTINGS_TITLEBAR_THEME_BUTTONS_Y_OFFSET: f32 = -3.0;
#[cfg(not(target_os = "macos"))]
const SETTINGS_TITLEBAR_THEME_BUTTONS_Y_OFFSET: f32 = 0.0;

pub enum SettingsControl {
	Continue,
	CloseRequested,
}

pub struct SettingsWindow {
	window: Arc<Window>,
	gpu: GpuContext,
	surface: Surface<'static>,
	surface_config: wgpu::SurfaceConfiguration,
	egui_ctx: egui::Context,
	egui_state: egui_winit::State,
	renderer: Renderer,
	modifiers: ModifiersState,
	last_redraw: Instant,
	did_autosize: bool,
	combo_width: f32,
	requested_theme: Option<Theme>,
	effective_theme: Option<Theme>,
	theme_icon_system: String,
	theme_icon_dark: String,
	theme_icon_light: String,
}
impl SettingsWindow {
	pub fn open(event_loop: &ActiveEventLoop) -> Result<Self> {
		let mut attrs = Window::default_attributes()
			.with_title("Settings")
			.with_inner_size(LogicalSize::new(520.0, 360.0))
			.with_resizable(false)
			.with_visible(true);

		#[cfg(target_os = "macos")]
		{
			use winit::platform::macos::WindowAttributesExtMacOS;

			attrs = attrs
				.with_titlebar_transparent(true)
				.with_title_hidden(true)
				.with_fullsize_content_view(true)
				.with_movable_by_window_background(true);
		}

		let window = event_loop.create_window(attrs).wrap_err("create settings window")?;
		let window = Arc::new(window);
		let (gpu, surface, surface_config) = GpuContext::new_with_surface(Arc::clone(&window))?;
		let egui_ctx = egui::Context::default();
		let theme_icon_system = regular::MONITOR.to_owned();
		let theme_icon_dark = regular::MOON.to_owned();
		let theme_icon_light = regular::SUN.to_owned();
		let mut fonts = egui::FontDefinitions::default();

		egui_phosphor::add_to_fonts(&mut fonts, Variant::Regular);

		egui_ctx.set_fonts(fonts);

		let egui_state = egui_winit::State::new(
			egui_ctx.clone(),
			egui::ViewportId::ROOT,
			window.as_ref(),
			None,
			None,
			None,
		);
		let renderer = Renderer::new(
			&gpu.device,
			surface_config.format,
			egui_wgpu::RendererOptions {
				msaa_samples: 1,
				depth_stencil_format: None,
				dithering: false,
				predictable_texture_filtering: false,
			},
		);

		Ok(Self {
			window,
			gpu,
			surface,
			surface_config,
			egui_ctx,
			egui_state,
			renderer,
			modifiers: ModifiersState::default(),
			last_redraw: Instant::now(),
			did_autosize: false,
			combo_width: SETTINGS_COMBO_WIDTH,
			requested_theme: None,
			effective_theme: None,
			theme_icon_system,
			theme_icon_dark,
			theme_icon_light,
		})
	}

	#[must_use]
	pub fn window_id(&self) -> WindowId {
		self.window.id()
	}

	pub fn focus(&self) {
		self.window.focus_window();
		self.window.request_redraw();
	}

	pub fn handle_window_event(&mut self, event: &WindowEvent) -> SettingsControl {
		match event {
			WindowEvent::CloseRequested => return SettingsControl::CloseRequested,
			WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
			WindowEvent::ThemeChanged(_) => {
				// Follow system theme changes when ThemeMode::System is active.
				self.window.request_redraw();
			},
			WindowEvent::KeyboardInput { event, .. } => {
				if cfg!(target_os = "macos")
					&& event.state == ElementState::Pressed
					&& self.modifiers.super_key()
					&& matches!(&event.logical_key, Key::Character(c) if c.as_str().eq_ignore_ascii_case("w"))
				{
					return SettingsControl::CloseRequested;
				}
			},
			WindowEvent::Resized(size) => self.resize(*size),
			WindowEvent::ScaleFactorChanged { .. } => self.resize(self.window.inner_size()),
			_ => {},
		}

		let _ = self.egui_state.on_window_event(&self.window, event);

		self.window.request_redraw();

		SettingsControl::Continue
	}

	pub fn draw(&mut self, settings: &mut AppSettings) -> Result<bool> {
		if self.last_redraw.elapsed().as_millis() > 1_500 {
			self.window.request_redraw();
		}

		self.last_redraw = Instant::now();

		let raw_input = self.egui_state.take_egui_input(&self.window);
		let mut settings_changed = false;
		let egui_ctx = self.egui_ctx.clone();
		let full_output = egui_ctx.run(raw_input, |ctx| {
			settings_changed = self.ui(ctx, settings);
		});

		if let Some(repaint_delay) = full_output
			.viewport_output
			.get(&egui::ViewportId::ROOT)
			.map(|viewport_output| viewport_output.repaint_delay)
			&& repaint_delay < Duration::from_secs(1)
			&& repaint_delay != Duration::MAX
		{
			self.window.request_redraw();
		}

		self.egui_state.handle_platform_output(&self.window, full_output.platform_output);

		for (id, delta) in &full_output.textures_delta.set {
			self.renderer.update_texture(&self.gpu.device, &self.gpu.queue, *id, delta);
		}
		for id in &full_output.textures_delta.free {
			self.renderer.free_texture(id);
		}

		let paint_jobs =
			self.egui_ctx.tessellate(full_output.shapes, self.window.scale_factor() as f32);
		let size = self.window.inner_size();
		let screen_descriptor = ScreenDescriptor {
			size_in_pixels: [size.width.max(1), size.height.max(1)],
			pixels_per_point: self.window.scale_factor() as f32,
		};
		let frame = self.acquire_frame()?;
		let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
		let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
			label: Some("rsnap-settings encoder"),
		});

		self.renderer.update_buffers(
			&self.gpu.device,
			&self.gpu.queue,
			&mut encoder,
			&paint_jobs,
			&screen_descriptor,
		);

		{
			let panel_fill = self.egui_ctx.style().visuals.panel_fill;
			let clear = wgpu::Color {
				r: f64::from(panel_fill.r()) / 255.0,
				g: f64::from(panel_fill.g()) / 255.0,
				b: f64::from(panel_fill.b()) / 255.0,
				a: f64::from(panel_fill.a()) / 255.0,
			};
			let rpass_desc = wgpu::RenderPassDescriptor {
				label: Some("rsnap-settings rpass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					depth_slice: None,
					resolve_target: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(clear),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
			};
			let mut rpass = encoder.begin_render_pass(&rpass_desc).forget_lifetime();

			self.renderer.render(&mut rpass, &paint_jobs, &screen_descriptor);
		}

		self.gpu.queue.submit(Some(encoder.finish()));
		frame.present();

		Ok(settings_changed)
	}

	fn acquire_frame(&mut self) -> Result<SurfaceTexture> {
		match self.surface.get_current_texture() {
			Ok(frame) => Ok(frame),
			Err(wgpu::SurfaceError::Outdated) => {
				self.reconfigure_surface();

				self.surface.get_current_texture().wrap_err("get_current_texture after reconfigure")
			},
			Err(wgpu::SurfaceError::Lost) => {
				self.recreate_surface().wrap_err("recreate surface")?;

				self.surface.get_current_texture().wrap_err("get_current_texture after recreate")
			},
			Err(err) => Err(eyre::eyre!("get_current_texture failed: {err:?}")),
		}
	}

	fn recreate_surface(&mut self) -> Result<()> {
		let surface = self
			.gpu
			.instance
			.create_surface(Arc::clone(&self.window))
			.wrap_err("create_surface")?;

		self.surface = surface;

		self.reconfigure_surface();

		Ok(())
	}

	fn reconfigure_surface(&mut self) {
		let caps = self.surface.get_capabilities(&self.gpu.adapter);

		self.surface_config.present_mode = caps.present_modes[0];
		self.surface_config.alpha_mode = pick_surface_alpha(&caps);

		self.surface.configure(&self.gpu.device, &self.surface_config);
	}

	fn resize(&mut self, size: PhysicalSize<u32>) {
		self.surface_config.width = size.width.max(1);
		self.surface_config.height = size.height.max(1);

		self.reconfigure_surface();
	}

	fn ui(&mut self, ctx: &egui::Context, settings: &mut AppSettings) -> bool {
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

	fn with_settings_density<R>(
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
			spacing.slider_rail_height = SETTINGS_SLIDER_RAIL_HEIGHT;

			add_contents(ui)
		})
		.inner
	}

	fn render_all_sections(
		&mut self,
		ui: &mut Ui,
		ctx: &egui::Context,
		settings: &mut AppSettings,
	) -> bool {
		let mut changed = false;

		egui::CollapsingHeader::new("General").default_open(true).show(ui, |ui| {
			changed |= self.render_general_section(ui, ctx, settings);
		});

		ui.add_space(SETTINGS_SECTION_GAP);

		egui::CollapsingHeader::new("Overlay").default_open(true).show(ui, |ui| {
			changed |= self.render_overlay_section(ui, settings);
		});

		ui.add_space(SETTINGS_SECTION_GAP);

		egui::CollapsingHeader::new("Hotkeys").default_open(false).show(ui, |ui| {
			ui.label("Hotkey customization is coming soon.");
		});

		ui.add_space(SETTINGS_SECTION_GAP);

		egui::CollapsingHeader::new("Capture").default_open(false).show(ui, |ui| {
			ui.label("Capture mode settings are coming soon.");
		});

		ui.add_space(SETTINGS_SECTION_GAP);

		egui::CollapsingHeader::new("Output").default_open(false).show(ui, |ui| {
			ui.label("Output settings are coming soon.");
		});

		ui.add_space(SETTINGS_SECTION_GAP);

		egui::CollapsingHeader::new("Advanced").default_open(false).show(ui, |ui| {
			ui.label("Advanced options are coming soon.");
		});

		ui.add_space(SETTINGS_SECTION_GAP);

		egui::CollapsingHeader::new("About").default_open(false).show(ui, |ui| {
			ui.label(format!("rsnap {}", env!("CARGO_PKG_VERSION")));
		});

		changed
	}

	fn render_general_section(
		&mut self,
		ui: &mut Ui,
		ctx: &egui::Context,
		settings: &mut AppSettings,
	) -> bool {
		let _ = ctx;
		let _ = settings;

		ui.label("General settings will live here.");

		false
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
			"Show Alt hint in HUD",
			"Glass HUD",
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
			"System",
			"Dark",
			"Light",
			"Hold",
			"Toggle",
			"Small (15x15)",
			"Medium (21x21)",
			"Large (31x31)",
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

		ui.painter().rect_filled(bar_rect, 0.0, ui.visuals().panel_fill);

		let row_height = ui.spacing().interact_size.y;
		let y_pad = ((bar_rect.height() - row_height) * 0.5).round();
		let theme_y = (bar_rect.min.y + y_pad + SETTINGS_TITLEBAR_THEME_BUTTONS_Y_OFFSET)
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

				// System / Dark / Light
				ui.selectable_value(
					&mut settings.theme_mode,
					ThemeMode::Light,
					egui::RichText::new(&self.theme_icon_light).size(SETTINGS_THEME_ICON_SIZE),
				)
				.on_hover_text("Light");
				ui.selectable_value(
					&mut settings.theme_mode,
					ThemeMode::Dark,
					egui::RichText::new(&self.theme_icon_dark).size(SETTINGS_THEME_ICON_SIZE),
				)
				.on_hover_text("Dark");
				ui.selectable_value(
					&mut settings.theme_mode,
					ThemeMode::System,
					egui::RichText::new(&self.theme_icon_system).size(SETTINGS_THEME_ICON_SIZE),
				)
				.on_hover_text("System");

				if settings.theme_mode != before {
					self.sync_theme(ctx, settings.theme_mode);

					changed = true;
				}
			},
		);

		changed
	}

	fn render_overlay_section(&mut self, ui: &mut Ui, settings: &mut AppSettings) -> bool {
		let mut changed = false;

		changed |=
			ui.checkbox(&mut settings.show_alt_hint_keycap, "Show Alt hint in HUD").changed();
		changed |= ui.checkbox(&mut settings.hud_glass_enabled, "Glass HUD").changed();

		ui.add_space(SETTINGS_SECTION_GAP);
		ui.separator();
		ui.add_space(SETTINGS_SECTION_GAP);

		let before_alt = settings.alt_activation;

		egui::ComboBox::from_label("Alt activation")
			.selected_text(Self::alt_activation_label(settings.alt_activation))
			.width(self.combo_width)
			.show_ui(ui, |ui| {
				ui.selectable_value(&mut settings.alt_activation, AltActivationMode::Hold, "Hold");
				ui.selectable_value(
					&mut settings.alt_activation,
					AltActivationMode::Toggle,
					"Toggle",
				);
			});

		if settings.alt_activation != before_alt {
			changed = true;
		}

		let before_loupe = settings.loupe_sample_size;

		egui::ComboBox::from_label("Loupe sample size")
			.selected_text(Self::loupe_sample_size_label(settings.loupe_sample_size))
			.width(self.combo_width)
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

		let enabled = settings.hud_glass_enabled;

		changed |= self.overlay_slider_row(ui, "Opacity", &mut settings.hud_opacity, enabled);
		changed |= self.overlay_slider_row(ui, "Blur", &mut settings.hud_blur, enabled);
		changed |= self.overlay_slider_row(ui, "Tint", &mut settings.hud_tint, enabled);

		changed
	}

	fn overlay_slider_row(
		&self,
		ui: &mut Ui,
		label: &str,
		amount: &mut f32,
		enabled: bool,
	) -> bool {
		let slider = egui::Slider::new(amount, 0.0..=1.0)
			.text(label)
			.custom_formatter(|value, _| format!("{:.0}%", value.clamp(0.0, 1.0) * 100.0));

		ui.add_enabled(enabled, slider).changed()
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

struct GpuContext {
	instance: wgpu::Instance,
	adapter: Adapter,
	device: Device,
	queue: Queue,
}
impl GpuContext {
	fn new_with_surface(
		window: Arc<Window>,
	) -> Result<(Self, Surface<'static>, wgpu::SurfaceConfiguration)> {
		let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
		let surface = instance.create_surface(Arc::clone(&window)).wrap_err("create_surface")?;
		let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::LowPower,
			compatible_surface: Some(&surface),
			force_fallback_adapter: false,
		}))
		.map_err(|err| eyre::eyre!("Failed to request GPU adapter: {err}"))?;
		let limits = adapter.limits();
		let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
			label: Some("rsnap-settings device"),
			required_features: wgpu::Features::empty(),
			required_limits: limits,
			experimental_features: wgpu::ExperimentalFeatures::default(),
			memory_hints: wgpu::MemoryHints::Performance,
			trace: wgpu::Trace::Off,
		}))
		.wrap_err("request_device")?;
		let caps = surface.get_capabilities(&adapter);
		let format = pick_surface_format(&caps);
		let alpha = pick_surface_alpha(&caps);
		let size = window.inner_size();
		let surface_config = wgpu::SurfaceConfiguration {
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			format,
			width: size.width.max(1),
			height: size.height.max(1),
			present_mode: caps.present_modes[0],
			alpha_mode: alpha,
			view_formats: vec![format],
			desired_maximum_frame_latency: 2,
		};

		surface.configure(&device, &surface_config);

		Ok((Self { instance, adapter, device, queue }, surface, surface_config))
	}
}

fn pick_surface_format(caps: &SurfaceCapabilities) -> TextureFormat {
	caps.formats
		.iter()
		.copied()
		.find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
		.or_else(|| {
			caps.formats.iter().copied().find(|f| {
				matches!(
					f,
					wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Rgba8UnormSrgb
				)
			})
		})
		.unwrap_or(caps.formats[0])
}

fn pick_surface_alpha(caps: &SurfaceCapabilities) -> CompositeAlphaMode {
	caps.alpha_modes
		.iter()
		.copied()
		.find(|m| matches!(m, wgpu::CompositeAlphaMode::PreMultiplied))
		.or_else(|| {
			caps.alpha_modes
				.iter()
				.copied()
				.find(|m| matches!(m, wgpu::CompositeAlphaMode::PostMultiplied))
		})
		.or_else(|| {
			caps.alpha_modes
				.iter()
				.copied()
				.find(|m| !matches!(m, wgpu::CompositeAlphaMode::Opaque))
		})
		.unwrap_or(caps.alpha_modes[0])
}
