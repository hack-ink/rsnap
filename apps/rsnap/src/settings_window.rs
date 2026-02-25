use std::sync::Arc;
use std::time::Instant;

use color_eyre::eyre::{self, Result, WrapErr};
use egui::Ui;
use egui::{Align, Layout};
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

use crate::settings::AppSettings;
use rsnap_overlay::ThemeMode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsPane {
	General,
	Hotkeys,
	Capture,
	Output,
	Overlay,
	Advanced,
	About,
}
impl SettingsPane {
	#[must_use]
	pub fn title(self) -> &'static str {
		match self {
			Self::General => "General",
			Self::Hotkeys => "Hotkeys",
			Self::Capture => "Capture",
			Self::Output => "Output",
			Self::Overlay => "Overlay",
			Self::Advanced => "Advanced",
			Self::About => "About",
		}
	}
}

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
	pane: SettingsPane,
	search: String,
	modifiers: ModifiersState,
	last_redraw: Instant,
	requested_theme: Option<Theme>,
	effective_theme: Option<Theme>,
}
impl SettingsWindow {
	pub fn open(event_loop: &ActiveEventLoop) -> Result<Self> {
		let attrs = Window::default_attributes()
			.with_title("Settings")
			.with_inner_size(LogicalSize::new(720.0, 520.0))
			.with_resizable(false)
			.with_visible(true);
		let window = event_loop.create_window(attrs).wrap_err("create settings window")?;
		let window = Arc::new(window);
		let (gpu, surface, surface_config) = GpuContext::new_with_surface(Arc::clone(&window))?;
		let egui_ctx = egui::Context::default();
		let egui_state = egui_winit::State::new(
			egui_ctx.clone(),
			egui::ViewportId::ROOT,
			window.as_ref(),
			None,
			None,
			None,
		);
		let renderer = Renderer::new(&gpu.device, surface_config.format, None, 1, false);

		Ok(Self {
			window,
			gpu,
			surface,
			surface_config,
			egui_ctx,
			egui_state,
			renderer,
			pane: SettingsPane::General,
			search: String::new(),
			modifiers: ModifiersState::default(),
			last_redraw: Instant::now(),
			requested_theme: None,
			effective_theme: None,
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
			let clear = wgpu::Color { r: 0.06, g: 0.06, b: 0.07, a: 1.0 };
			let rpass_desc = wgpu::RenderPassDescriptor {
				label: Some("rsnap-settings rpass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
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
		self.render_top_panel(ctx);
		self.render_sidebar(ctx);

		self.render_central_panel(ctx, settings)
	}

	fn render_top_panel(&mut self, ctx: &egui::Context) {
		egui::TopBottomPanel::top("settings_top").show(ctx, |ui| {
			ui.add_space(6.0);
			ui.horizontal(|ui| {
				ui.heading("Settings");
				ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
					ui.add(
						egui::TextEdit::singleline(&mut self.search)
							.hint_text("Search")
							.desired_width(220.0),
					);
				});
			});
			ui.add_space(6.0);
		});
	}

	fn render_sidebar(&mut self, ctx: &egui::Context) {
		egui::SidePanel::left("settings_sidebar").resizable(false).default_width(170.0).show(
			ctx,
			|ui| {
				ui.add_space(8.0);
				self.sidebar_row(ui, SettingsPane::General);
				self.sidebar_row(ui, SettingsPane::Hotkeys);
				self.sidebar_row(ui, SettingsPane::Capture);
				self.sidebar_row(ui, SettingsPane::Output);
				self.sidebar_row(ui, SettingsPane::Overlay);
				ui.separator();
				self.sidebar_row(ui, SettingsPane::Advanced);
				self.sidebar_row(ui, SettingsPane::About);
			},
		);
	}

	fn render_central_panel(&mut self, ctx: &egui::Context, settings: &mut AppSettings) -> bool {
		let mut changed = false;

		egui::CentralPanel::default().show(ctx, |ui| {
			ui.add_space(12.0);
			ui.heading(self.pane.title());
			ui.add_space(8.0);
			ui.separator();
			ui.add_space(10.0);

			changed |= self.render_pane(ui, ctx, settings);
		});

		changed
	}

	fn render_pane(
		&mut self,
		ui: &mut Ui,
		ctx: &egui::Context,
		settings: &mut AppSettings,
	) -> bool {
		let mut changed = false;

		match self.pane {
			SettingsPane::Overlay => {
				changed |= ui
					.checkbox(&mut settings.show_alt_hint_keycap, "Show Alt hint in HUD")
					.changed();
				changed |= ui.checkbox(&mut settings.hud_opaque, "Opaque HUD").changed();
				ui.add_enabled_ui(!settings.hud_opaque, |ui| {
					changed |=
						ui.checkbox(&mut settings.show_hud_blur, "Enable HUD blur").changed();
				});

				let hud_blur_effective = settings.show_hud_blur && !settings.hud_opaque;

				ui.add_space(6.0);

				ui.add_enabled_ui(hud_blur_effective, |ui| {
					changed |= Self::checkbox_slider_row(
						ui,
						&mut settings.hud_fog_enabled,
						&mut settings.hud_fog_amount,
						"Fog",
					);
					changed |= Self::checkbox_slider_row(
						ui,
						&mut settings.hud_milk_enabled,
						&mut settings.hud_milk_amount,
						"Milkiness",
					);
				});

				ui.add_space(8.0);
				ui.label("More overlay options will live here.");
			},
			SettingsPane::General => {
				ui.horizontal(|ui| {
					ui.label("Theme");

					let before = settings.theme_mode;

					egui::ComboBox::from_id_salt("theme_mode")
						.selected_text(Self::theme_mode_label(settings.theme_mode))
						.show_ui(ui, |ui| {
							ui.selectable_value(
								&mut settings.theme_mode,
								ThemeMode::System,
								"System",
							);
							ui.selectable_value(&mut settings.theme_mode, ThemeMode::Dark, "Dark");
							ui.selectable_value(
								&mut settings.theme_mode,
								ThemeMode::Light,
								"Light",
							);
						});

					if settings.theme_mode != before {
						self.sync_theme(ctx, settings.theme_mode);

						changed = true;
					}
				});
			},
			SettingsPane::Hotkeys => {
				ui.label("Hotkey customization is coming soon.");
			},
			SettingsPane::Capture => {
				ui.label("Capture mode settings are coming soon.");
			},
			SettingsPane::Output => {
				ui.label("Output settings are coming soon.");
			},
			SettingsPane::Advanced => {
				ui.label("Advanced options are coming soon.");
			},
			SettingsPane::About => {
				ui.label(format!("rsnap {}", env!("CARGO_PKG_VERSION")));
			},
		}

		changed
	}

	fn checkbox_slider_row(
		ui: &mut Ui,
		enabled: &mut bool,
		amount: &mut f32,
		label: &'static str,
	) -> bool {
		let mut changed = false;

		ui.horizontal(|ui| {
			changed |= ui.checkbox(enabled, label).changed();
			ui.add_enabled_ui(*enabled, |ui| {
				changed |= ui
					.add(egui::Slider::new(amount, 0.0..=1.0).show_value(false).trailing_fill(true))
					.changed();
			});
		});

		changed
	}

	fn theme_mode_label(mode: ThemeMode) -> &'static str {
		match mode {
			ThemeMode::System => "System",
			ThemeMode::Dark => "Dark",
			ThemeMode::Light => "Light",
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

	fn sidebar_row(&mut self, ui: &mut Ui, pane: SettingsPane) {
		let is_selected = self.pane == pane;

		if ui.selectable_label(is_selected, pane.title()).clicked() {
			self.pane = pane;
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
		.ok_or_else(|| eyre::eyre!("No suitable GPU adapters found"))?;
		let limits = adapter.limits();
		let (device, queue) = pollster::block_on(adapter.request_device(
			&wgpu::DeviceDescriptor {
				label: Some("rsnap-settings device"),
				required_features: wgpu::Features::empty(),
				required_limits: limits,
				memory_hints: wgpu::MemoryHints::Performance,
			},
			None,
		))
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
