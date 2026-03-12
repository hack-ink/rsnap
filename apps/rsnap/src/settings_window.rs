mod chrome;
mod hotkey;
mod platform;
mod render;
mod sections;

use std::collections::VecDeque;
use std::mem;
use std::time::Instant;

use color_eyre::eyre::{Result, WrapErr};
use egui::{self};
use egui_phosphor::{Variant, regular};
use egui_wgpu::Renderer;
use global_hotkey::hotkey::HotKey;
use wgpu::Surface;
use wgpu::SurfaceConfiguration;
use winit::event::ElementState;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::Theme;
use winit::window::{Window, WindowId};

use render::GpuContext;

const SETTINGS_ROW_HEIGHT: f32 = 22.0;
const SETTINGS_SECTION_GAP: f32 = 6.0;
const SETTINGS_COMBO_WIDTH: f32 = 220.0;
const SETTINGS_SLIDER_RAIL_HEIGHT: f32 = 4.0;
const SETTINGS_HUE_SLIDER_HEIGHT: f32 = 12.0;
// egui slider knob size is derived from widget height (`height / 2.5` radius).
// Render the slider itself shorter so the knob matches the Hue handle (12px diameter).
const SETTINGS_SLIDER_WIDGET_HEIGHT: f32 = 15.0;
const SETTINGS_VALUE_BOX_WIDTH: f32 = 56.0;
const SETTINGS_HUE_SLIDER_STEPS: usize = 64;
const SETTINGS_HUE_SLIDER_SATURATION: f32 = 0.9;
const SETTINGS_HUE_SLIDER_LIGHTNESS: f32 = 0.58;
const SETTINGS_TITLEBAR_HEIGHT: f32 = 28.0;
const SETTINGS_THEME_ICON_SIZE: f32 = 16.0;

pub enum SettingsControl {
	Continue,
	CloseRequested,
}

#[derive(Clone, Debug)]
pub enum SettingsWindowAction {
	BeginCaptureHotkey,
	CancelCaptureHotkey,
	ApplyCaptureHotkey(HotKey),
}

#[derive(Clone, Debug)]
pub enum CaptureHotkeyNotice {
	Error(String),
	Hint(String),
	Success(String),
}

pub struct SettingsWindow {
	window: std::sync::Arc<Window>,
	gpu: GpuContext,
	surface: Surface<'static>,
	surface_config: SurfaceConfiguration,
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
	capture_hotkey_recording: bool,
	capture_hotkey_notice: Option<CaptureHotkeyNotice>,
	action_queue: VecDeque<SettingsWindowAction>,
}
impl SettingsWindow {
	pub fn open(event_loop: &ActiveEventLoop) -> Result<Self> {
		let attrs = platform::settings_window_attributes();
		let window = event_loop.create_window(attrs).wrap_err("create settings window")?;
		let window = std::sync::Arc::new(window);
		let (gpu, surface, surface_config) =
			GpuContext::new_with_surface(std::sync::Arc::clone(&window))?;
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
			capture_hotkey_recording: false,
			capture_hotkey_notice: None,
			action_queue: VecDeque::new(),
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
			WindowEvent::ModifiersChanged(modifiers) => {
				self.modifiers = modifiers.state();

				if self.capture_hotkey_recording {
					let has_any = self.modifiers.alt_key()
						|| self.modifiers.shift_key()
						|| self.modifiers.control_key()
						|| self.modifiers.super_key();

					if has_any && self.capture_hotkey_notice.is_none() {
						self.capture_hotkey_notice = Some(CaptureHotkeyNotice::Hint(String::from(
							hotkey::CAPTURE_HOTKEY_GUIDANCE_PRESS_NONMOD,
						)));
					}
					if !has_any
						&& matches!(
							self.capture_hotkey_notice.as_ref(),
							Some(CaptureHotkeyNotice::Hint(text))
								if text == hotkey::CAPTURE_HOTKEY_GUIDANCE_PRESS_NONMOD
						) {
						self.capture_hotkey_notice = None;
					}

					self.window.request_redraw();
				}
			},
			WindowEvent::Focused(false) if self.capture_hotkey_recording => {
				self.cancel_recording_capture_hotkey();
			},
			WindowEvent::Ime(_) if self.capture_hotkey_recording => {
				self.capture_hotkey_notice = Some(CaptureHotkeyNotice::Error(String::from(
					"Unsupported key for hotkey binding.",
				)));

				self.window.request_redraw();
			},
			WindowEvent::KeyboardInput { event, .. } if self.capture_hotkey_recording => {
				if event.state == ElementState::Pressed {
					self.handle_capture_hotkey_recording_input(event);
				}
			},
			WindowEvent::ThemeChanged(_) => {
				// Follow system theme changes when ThemeMode::System is active.
				self.window.request_redraw();
			},
			WindowEvent::KeyboardInput { event, .. } => {
				if platform::should_close_from_keyboard(self.modifiers, event) {
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

	pub fn drain_actions(&mut self) -> VecDeque<SettingsWindowAction> {
		mem::take(&mut self.action_queue)
	}

	fn queue_action(&mut self, action: SettingsWindowAction) {
		self.action_queue.push_back(action);
	}
}
