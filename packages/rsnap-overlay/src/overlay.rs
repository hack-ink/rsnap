use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use color_eyre::eyre::{self, Result, WrapErr};
use device_query::{DeviceQuery, Keycode};
use egui::ClippedPrimitive;
use egui::FullOutput;
use egui::Ui;
use egui::{
	Align, Align2, Color32, CornerRadius, Event, FontDefinitions, FontId, Frame, Id, Layout,
	Margin, PointerButton, Pos2, Rect, Sense, Vec2, ViewportId,
};
use egui_phosphor::{Variant, regular};
use egui_wgpu::{Renderer, ScreenDescriptor};
use image::{RgbaImage, imageops::FilterType};
#[cfg(target_os = "macos")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use wgpu::Adapter;
use wgpu::BindGroup;
use wgpu::BindGroupLayout;
use wgpu::CompositeAlphaMode;
use wgpu::Device;
use wgpu::Queue;
use wgpu::RenderPipeline;
use wgpu::Surface;
use wgpu::SurfaceCapabilities;
use wgpu::SurfaceError;
use wgpu::SurfaceTexture;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::KeyEvent;
use winit::{
	dpi::PhysicalSize,
	event::{ElementState, MouseButton, WindowEvent},
	event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
	keyboard::{Key, NamedKey},
	window::{Theme, WindowId, WindowLevel},
};

use crate::{
	state::{GlobalPoint, MonitorRect, OverlayMode, OverlayState},
	worker::{OverlayWorker, WorkerRequestSendError, WorkerResponse},
};

const HUD_PILL_BODY_FILL_DARK_SRGBA8: [u8; 4] = [28, 28, 32, 156];
const HUD_PILL_BODY_FILL_LIGHT_SRGBA8: [u8; 4] = [232, 236, 243, 176];
const HUD_PILL_BLUR_TINT_ALPHA_DARK: f32 = 0.18;
const HUD_PILL_BLUR_TINT_ALPHA_LIGHT: f32 = 0.22;
const LOUPE_TILE_CORNER_RADIUS_POINTS: f64 = 12.0;
const MACOS_HUD_WINDOW_LEVEL: isize = 25;
const FROZEN_CAPTURE_POLL_INTERVAL: Duration = Duration::from_millis(16);
const FROZEN_TOOLBAR_TOOL_COUNT: usize = 9;
const FROZEN_TOOLBAR_BUTTON_SIZE_POINTS: f32 = 24.0;
const FROZEN_TOOLBAR_ITEM_SPACING_POINTS: f32 = 4.0;
const LIVE_EVENT_CURSOR_CACHE_TTL: Duration = Duration::from_millis(120);
const HUD_PILL_INNER_MARGIN_X_POINTS: f32 = 12.0;
const HUD_PILL_INNER_MARGIN_Y_POINTS: f32 = 8.0;
const HUD_PILL_STROKE_WIDTH_POINTS: f32 = 1.0;
const TOOLBAR_EXPANDED_WIDTH_PX: f32 = (FROZEN_TOOLBAR_TOOL_COUNT as f32)
	* FROZEN_TOOLBAR_BUTTON_SIZE_POINTS
	+ ((FROZEN_TOOLBAR_TOOL_COUNT as f32) - 1.0) * FROZEN_TOOLBAR_ITEM_SPACING_POINTS
	+ 2.0 * HUD_PILL_INNER_MARGIN_X_POINTS
	+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
const TOOLBAR_EXPANDED_HEIGHT_PX: f32 = FROZEN_TOOLBAR_BUTTON_SIZE_POINTS
	+ 2.0 * HUD_PILL_INNER_MARGIN_Y_POINTS
	+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
const TOOLBAR_CAPTURE_GAP_PX: f32 = 10.0;
const TOOLBAR_SCREEN_MARGIN_PX: f32 = 10.0;
const HUD_PILL_CORNER_RADIUS_POINTS: u8 = 18;
const TOOLBAR_DRAG_START_THRESHOLD_PX: f32 = 6.0;
const TOOLBAR_WINDOW_WARMUP_REDRAWS: u8 = 30;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HudAnchor {
	Cursor,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
	#[default]
	System,
	Dark,
	Light,
}

#[derive(Debug)]
pub enum OverlayExit {
	Cancelled,
	PngBytes(Vec<u8>),
	Error(String),
}

#[derive(Debug)]
pub enum OverlayControl {
	Continue,
	Exit(OverlayExit),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AltActivationMode {
	#[default]
	Hold,
	Toggle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HudTheme {
	Dark,
	Light,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrozenToolbarTool {
	Pointer,
	Pen,
	Text,
	Mosaic,
	Undo,
	Redo,
	Copy,
	Save,
	Done,
}
impl FrozenToolbarTool {
	const fn label(self) -> &'static str {
		match self {
			Self::Pointer => "Pointer",
			Self::Pen => "Pen",
			Self::Text => "Text",
			Self::Mosaic => "Mosaic",
			Self::Undo => "Undo",
			Self::Redo => "Redo",
			Self::Copy => "Copy",
			Self::Save => "Save",
			Self::Done => "Done",
		}
	}

	const fn icon(self) -> &'static str {
		match self {
			Self::Pointer => regular::CURSOR,
			Self::Pen => regular::PENCIL_SIMPLE,
			Self::Text => regular::TEXT_T,
			Self::Mosaic => regular::CHECKERBOARD,
			Self::Undo => regular::ARROW_COUNTER_CLOCKWISE,
			Self::Redo => regular::ARROW_CLOCKWISE,
			Self::Copy => regular::COPY,
			Self::Save => regular::FLOPPY_DISK,
			Self::Done => regular::CHECK,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeviceCursorPointSource {
	DevicePoints,
	DevicePixelsFallback,
	EventRecentFallback,
	Event,
}
impl DeviceCursorPointSource {
	const fn as_str(self) -> &'static str {
		match self {
			Self::DevicePoints => "device_points",
			Self::DevicePixelsFallback => "device_pixels_fallback",
			Self::EventRecentFallback => "event_recent_fallback",
			Self::Event => "event",
		}
	}
}

#[derive(Clone, Debug)]
pub struct OverlayConfig {
	pub hud_anchor: HudAnchor,
	pub show_alt_hint_keycap: bool,
	pub show_hud_blur: bool,
	pub hud_opaque: bool,
	/// 0..=1. Controls HUD background alpha.
	pub hud_opacity: f32,
	/// 0..=1. 0 disables the effect.
	pub hud_fog_amount: f32,
	/// 0..=1. 0 disables the effect.
	pub hud_milk_amount: f32,
	/// Hue value for tint, 0..=1.
	pub hud_tint_hue: f32,
	pub alt_activation: AltActivationMode,
	pub loupe_sample_side_px: u32,
	pub theme_mode: ThemeMode,
}
impl Default for OverlayConfig {
	fn default() -> Self {
		Self {
			hud_anchor: HudAnchor::Cursor,
			show_alt_hint_keycap: true,
			show_hud_blur: true,
			hud_opaque: false,
			hud_opacity: 0.35,
			hud_fog_amount: 0.16,
			hud_milk_amount: 0.0,
			hud_tint_hue: 0.585,
			alt_activation: AltActivationMode::Hold,
			loupe_sample_side_px: 21,
			theme_mode: ThemeMode::System,
		}
	}
}

#[allow(dead_code)]
pub struct OverlayBuilder {
	config: OverlayConfig,
}
#[allow(dead_code)]
impl OverlayBuilder {
	#[must_use]
	pub fn new() -> Self {
		Self { config: OverlayConfig::default() }
	}

	#[must_use]
	pub fn with_config(mut self, config: OverlayConfig) -> Self {
		self.config = config;

		self
	}

	pub fn run(self) -> Result<OverlayExit> {
		struct Runner {
			session: OverlaySession,
			exit: Option<OverlayExit>,
		}

		impl ApplicationHandler<()> for Runner {
			fn resumed(&mut self, event_loop: &ActiveEventLoop) {
				if let Err(err) = self.session.start(event_loop) {
					self.exit = Some(OverlayExit::Error(err));

					event_loop.exit();
				}
			}

			fn window_event(
				&mut self,
				event_loop: &ActiveEventLoop,
				window_id: WindowId,
				event: WindowEvent,
			) {
				match self.session.handle_window_event(window_id, &event) {
					OverlayControl::Continue => {},
					OverlayControl::Exit(exit) => {
						self.exit = Some(exit);

						event_loop.exit();
					},
				}
			}

			fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
				if let OverlayControl::Exit(exit) = self.session.about_to_wait() {
					self.exit = Some(exit);

					event_loop.exit();

					return;
				}

				let now = Instant::now();
				let next_repaint = self.session.consume_egui_repaint_deadline(now);

				match next_repaint {
					Some(deadline) if deadline <= now => {
						event_loop.set_control_flow(ControlFlow::Wait)
					},
					Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
					None => event_loop.set_control_flow(ControlFlow::Wait),
				}
			}
		}

		let event_loop = EventLoop::new()?;
		let mut runner = Runner { session: OverlaySession::with_config(self.config), exit: None };

		event_loop.run_app(&mut runner)?;

		Ok(runner.exit.unwrap_or(OverlayExit::Cancelled))
	}
}

pub struct OverlaySession {
	config: OverlayConfig,
	worker: Option<OverlayWorker>,
	cursor_device: device_query::DeviceState,
	state: OverlayState,
	cursor_monitor: Option<MonitorRect>,
	egui_repaint_deadline: Arc<Mutex<Option<Instant>>>,
	windows: HashMap<WindowId, OverlayWindow>,
	hud_window: Option<HudOverlayWindow>,
	loupe_window: Option<HudOverlayWindow>,
	toolbar_window: Option<HudOverlayWindow>,
	hud_outer_pos: Option<GlobalPoint>,
	hud_inner_size_points: Option<(u32, u32)>,
	loupe_outer_pos: Option<GlobalPoint>,
	loupe_inner_size_points: Option<(u32, u32)>,
	toolbar_outer_pos: Option<GlobalPoint>,
	toolbar_inner_size_points: Option<(u32, u32)>,
	gpu: Option<GpuContext>,
	last_present_at: Instant,
	last_rgb_request_at: Instant,
	rgb_request_interval: Duration,
	pending_rgb_request_at: Option<Instant>,
	pending_rgb_request_id: Option<u64>,
	rgb_send_sequence: u64,
	rgb_send_full_count: u64,
	rgb_send_disconnected_count: u64,
	last_live_bg_request_at: Instant,
	live_bg_request_interval: Duration,
	last_loupe_request_at: Instant,
	loupe_request_interval: Duration,
	pending_loupe_request_at: Option<Instant>,
	pending_loupe_request_id: Option<u64>,
	loupe_send_sequence: u64,
	loupe_send_full_count: u64,
	loupe_send_disconnected_count: u64,
	last_live_sample_cursor: Option<GlobalPoint>,
	last_event_cursor: Option<(MonitorRect, GlobalPoint)>,
	last_event_cursor_at: Option<Instant>,
	live_sample_stall_started_at: Option<Instant>,
	last_live_sample_stall_log_at: Option<Instant>,
	last_alt_press_at: Option<Instant>,
	alt_modifier_down: bool,
	loupe_patch_width_px: u32,
	loupe_patch_height_px: u32,
	pending_freeze_capture: Option<MonitorRect>,
	pending_freeze_capture_armed: bool,
	capture_windows_hidden: bool,
	pending_encode_png: Option<RgbaImage>,
	toolbar_state: FrozenToolbarState,
	toolbar_left_button_down: bool,
	toolbar_left_button_down_prev: bool,
	toolbar_pointer_local: Option<Pos2>,
	toolbar_window_visible: bool,
	toolbar_window_warmup_redraws_remaining: u8,
}
impl OverlaySession {
	#[must_use]
	pub fn new() -> Self {
		Self::with_config(OverlayConfig::default())
	}

	#[must_use]
	pub fn with_config(config: OverlayConfig) -> Self {
		let live_bg_request_interval = Duration::from_millis(500);
		let loupe_sample_side_px =
			Self::normalized_loupe_sample_side_px(config.loupe_sample_side_px);
		let mut state = OverlayState::new();

		state.loupe_patch_side_px = loupe_sample_side_px;

		Self {
			config,
			worker: None,
			cursor_device: device_query::DeviceState::new(),
			state,
			cursor_monitor: None,
			windows: HashMap::new(),
			hud_window: None,
			loupe_window: None,
			toolbar_window: None,
			hud_outer_pos: None,
			hud_inner_size_points: None,
			loupe_outer_pos: None,
			loupe_inner_size_points: None,
			toolbar_outer_pos: None,
			toolbar_inner_size_points: None,
			gpu: None,
			last_present_at: Instant::now(),
			last_rgb_request_at: Instant::now(),
			rgb_request_interval: Duration::from_millis(16),
			pending_rgb_request_at: None,
			pending_rgb_request_id: None,
			rgb_send_sequence: 0,
			rgb_send_full_count: 0,
			rgb_send_disconnected_count: 0,
			last_live_bg_request_at: Instant::now() - live_bg_request_interval,
			live_bg_request_interval,
			last_loupe_request_at: Instant::now(),
			loupe_request_interval: Duration::from_millis(33),
			pending_loupe_request_at: None,
			pending_loupe_request_id: None,
			loupe_send_sequence: 0,
			loupe_send_full_count: 0,
			loupe_send_disconnected_count: 0,
			last_live_sample_cursor: None,
			last_event_cursor: None,
			last_event_cursor_at: None,
			live_sample_stall_started_at: None,
			last_live_sample_stall_log_at: None,
			last_alt_press_at: None,
			alt_modifier_down: false,
			loupe_patch_width_px: loupe_sample_side_px,
			loupe_patch_height_px: loupe_sample_side_px,
			egui_repaint_deadline: Arc::new(Mutex::new(None)),
			pending_freeze_capture: None,
			pending_freeze_capture_armed: false,
			capture_windows_hidden: false,
			pending_encode_png: None,
			toolbar_state: FrozenToolbarState::default(),
			toolbar_left_button_down: false,
			toolbar_left_button_down_prev: false,
			toolbar_pointer_local: None,
			toolbar_window_visible: false,
			toolbar_window_warmup_redraws_remaining: 0,
		}
	}

	pub fn set_config(&mut self, config: OverlayConfig) {
		let prev = self.config.clone();
		let previous_loupe_patch = self.loupe_patch_width_px;
		let loupe_sample_side = Self::normalized_loupe_sample_side_px(config.loupe_sample_side_px);

		self.config = config;
		self.loupe_patch_width_px = loupe_sample_side;
		self.loupe_patch_height_px = loupe_sample_side;
		self.state.loupe_patch_side_px = loupe_sample_side;

		let patch_changed = self.loupe_patch_width_px != previous_loupe_patch;

		if patch_changed {
			self.state.loupe = None;
		}
		if !self.is_active() {
			return;
		}

		self.configure_hud_windows_for_config();

		let prev_fake_blur = prev.show_hud_blur && !cfg!(target_os = "macos");
		let new_fake_blur = self.use_fake_hud_blur();

		self.handle_fake_hud_blur_toggle(prev_fake_blur, new_fake_blur);

		if patch_changed {
			self.request_loupe_sample_for_patch_change();
		}

		self.request_redraw_all();
	}

	fn configure_hud_windows_for_config(&mut self) {
		if let Some(hud_window) = self.hud_window.as_ref() {
			self.configure_hud_window_common(hud_window.window.as_ref(), None);
		}
		if let Some(loupe_window) = self.loupe_window.as_ref() {
			self.configure_hud_window_common(
				loupe_window.window.as_ref(),
				Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
			);
		}
		if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			self.configure_hud_window_common(
				toolbar_window.window.as_ref(),
				Some(f64::from(HUD_PILL_CORNER_RADIUS_POINTS)),
			);
		}
	}

	fn configure_hud_window_common(
		&self,
		window: &winit::window::Window,
		corner_radius: Option<f64>,
	) {
		window.set_transparent(true);

		#[cfg(target_os = "macos")]
		macos_configure_hud_window(
			window,
			self.macos_hud_window_blur_enabled(),
			self.config.hud_fog_amount,
			corner_radius,
		);

		#[cfg(not(target_os = "macos"))]
		window.set_blur(self.config.show_hud_blur);
	}

	fn handle_fake_hud_blur_toggle(&mut self, prev_fake_blur: bool, new_fake_blur: bool) {
		if prev_fake_blur == new_fake_blur {
			return;
		}
		if new_fake_blur {
			self.last_live_bg_request_at = Instant::now() - self.live_bg_request_interval;

			if matches!(self.state.mode, OverlayMode::Live)
				&& let Some(_cursor) = self.state.cursor
				&& let Some(monitor) = self.active_cursor_monitor()
			{
				self.maybe_request_live_bg(monitor);
			}

			return;
		}

		self.state.live_bg_monitor = None;
		self.state.live_bg_image = None;
	}

	fn request_loupe_sample_for_patch_change(&mut self) {
		if !matches!(self.state.mode, OverlayMode::Live)
			|| !self.state.alt_held
			|| self.state.cursor.is_none()
			|| self.worker.is_none()
			|| self.active_cursor_monitor().is_none()
		{
			return;
		}

		let Some(cursor) = self.state.cursor else {
			return;
		};
		let Some(monitor) = self.active_cursor_monitor() else {
			return;
		};

		self.send_loupe_sample_request(
			monitor,
			cursor,
			self.loupe_patch_width_px,
			self.loupe_patch_height_px,
			false,
		);
	}

	#[must_use]
	pub fn is_active(&self) -> bool {
		!self.windows.is_empty()
	}

	fn use_fake_hud_blur(&self) -> bool {
		self.config.show_hud_blur && !cfg!(target_os = "macos")
	}

	fn macos_hud_window_blur_enabled(&self) -> bool {
		self.config.show_hud_blur
	}

	fn normalized_loupe_sample_side_px(side_px: u32) -> u32 {
		let side_px = side_px.max(3);

		if side_px & 1 == 0 { side_px + 1 } else { side_px }
	}

	pub fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		if self.is_active() {
			return Ok(());
		}

		self.reset_for_start();

		self.worker = Some(OverlayWorker::new(crate::backend::default_capture_backend()));

		let monitors =
			xcap::Monitor::all().map_err(|err| format!("xcap Monitor::all failed: {err:?}"))?;

		if monitors.is_empty() {
			return Err(String::from("No monitors detected"));
		}

		self.gpu = Some(GpuContext::new().map_err(|err| format!("{err:#}"))?);

		self.create_overlay_windows(event_loop, &monitors)?;
		self.create_hud_window(event_loop)?;
		self.create_loupe_window(event_loop)?;
		self.create_toolbar_window(event_loop)?;
		self.request_redraw_all();
		self.initialize_cursor_state();

		Ok(())
	}

	fn reset_for_start(&mut self) {
		self.hud_inner_size_points = None;
		self.hud_outer_pos = None;
		self.loupe_inner_size_points = None;
		self.loupe_outer_pos = None;
		self.toolbar_inner_size_points = None;
		self.toolbar_outer_pos = None;
		self.cursor_monitor = None;
		self.state = OverlayState::new();
		self.state.loupe_patch_side_px = self.loupe_patch_width_px;
		self.pending_freeze_capture = None;
		self.pending_freeze_capture_armed = false;
		self.pending_rgb_request_at = None;
		self.pending_rgb_request_id = None;
		self.rgb_send_sequence = 0;
		self.rgb_send_full_count = 0;
		self.rgb_send_disconnected_count = 0;
		self.pending_loupe_request_at = None;
		self.pending_loupe_request_id = None;
		self.loupe_send_sequence = 0;
		self.loupe_send_full_count = 0;
		self.loupe_send_disconnected_count = 0;
		self.last_event_cursor = None;
		self.last_event_cursor_at = None;
		self.last_live_sample_cursor = None;
		self.live_sample_stall_started_at = None;
		self.last_live_sample_stall_log_at = None;
		self.toolbar_state = FrozenToolbarState::default();
		self.toolbar_left_button_down = false;
		self.toolbar_left_button_down_prev = false;
		self.toolbar_pointer_local = None;
	}

	fn create_overlay_windows(
		&mut self,
		event_loop: &ActiveEventLoop,
		monitors: &[xcap::Monitor],
	) -> Result<(), String> {
		for monitor in monitors {
			let monitor_rect = MonitorRect {
				id: monitor.id().map_err(|err| {
					format!(
						"Failed to read xcap monitor id while creating overlay windows: {err:?}"
					)
				})?,
				origin: GlobalPoint::new(
					monitor.x().map_err(|err| {
						format!(
							"Failed to read monitor x position while creating overlay windows: {err:?}"
						)
					})?,
					monitor.y().map_err(|err| {
						format!(
							"Failed to read monitor y position while creating overlay windows: {err:?}"
						)
					})?,
				),
				width: monitor.width().map_err(|err| {
					format!("Failed to read monitor width while creating overlay windows: {err:?}")
				})?,
				height: monitor.height().map_err(|err| {
					format!("Failed to read monitor height while creating overlay windows: {err:?}")
				})?,
				scale_factor_x1000: {
					let scale_factor = monitor.scale_factor().map_err(|err| {
						format!(
							"Failed to read monitor scale factor while creating overlay windows: {err:?}"
						)
					})?;

					(scale_factor * 1_000.0).round() as u32
				},
			};
			let attrs = winit::window::Window::default_attributes()
				.with_title("rsnap-overlay")
				.with_decorations(false)
				.with_resizable(false)
				.with_transparent(true)
				.with_window_level(WindowLevel::AlwaysOnTop)
				.with_inner_size(LogicalSize::new(
					monitor_rect.width as f64,
					monitor_rect.height as f64,
				))
				.with_position(LogicalPosition::new(
					monitor_rect.origin.x as f64,
					monitor_rect.origin.y as f64,
				));
			let window = event_loop
				.create_window(attrs)
				.map_err(|err| format!("Unable to create overlay window: {err}"))?;
			let window = Arc::new(window);
			let scale_factor = monitor_rect.scale_factor();
			let inner_size = window.inner_size();

			tracing::debug!(
				monitor_id = monitor_rect.id,
				origin = ?monitor_rect.origin,
				width_points = monitor_rect.width,
				height_points = monitor_rect.height,
				monitor_scale_factor = scale_factor,
				window_scale_factor = window.scale_factor(),
				window_inner_size_px = ?inner_size,
				"Overlay window created."
			);

			let _ = window.set_cursor_hittest(true);

			#[cfg(target_os = "macos")]
			macos_configure_overlay_window_mouse_moved_events(window.as_ref());

			window.request_redraw();
			window.focus_window();

			let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
			let renderer = WindowRenderer::new(
				gpu,
				Arc::clone(&window),
				Arc::clone(&self.egui_repaint_deadline),
			)
			.map_err(|err| format!("Failed to init renderer: {err:#}"))?;

			self.windows
				.insert(window.id(), OverlayWindow { monitor: monitor_rect, window, renderer });
		}

		Ok(())
	}

	fn create_hud_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		let attrs = winit::window::Window::default_attributes()
			.with_title("rsnap-hud")
			.with_decorations(false)
			.with_resizable(false)
			.with_transparent(true)
			.with_window_level(WindowLevel::AlwaysOnTop)
			.with_inner_size(LogicalSize::new(460.0, 52.0));
		let window = event_loop
			.create_window(attrs)
			.map_err(|err| format!("Unable to create HUD window: {err}"))?;
		let window = Arc::new(window);
		#[cfg(target_os = "macos")]
		let _ = window.set_cursor_hittest(true);
		#[cfg(not(target_os = "macos"))]
		let _ = window.set_cursor_hittest(false);

		window.set_transparent(true);

		#[cfg(target_os = "macos")]
		macos_configure_hud_window(
			window.as_ref(),
			self.macos_hud_window_blur_enabled(),
			self.config.hud_fog_amount,
			None,
		);

		#[cfg(not(target_os = "macos"))]
		window.set_blur(self.config.show_hud_blur);
		window.request_redraw();

		let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
		let renderer =
			WindowRenderer::new(gpu, Arc::clone(&window), Arc::clone(&self.egui_repaint_deadline))
				.map_err(|err| format!("Failed to init HUD renderer: {err:#}"))?;

		self.hud_window = Some(HudOverlayWindow { window, renderer });

		Ok(())
	}

	fn create_loupe_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		let attrs = winit::window::Window::default_attributes()
			.with_title("rsnap-loupe")
			.with_decorations(false)
			.with_resizable(false)
			.with_transparent(true)
			.with_visible(false)
			.with_window_level(WindowLevel::AlwaysOnTop)
			.with_inner_size(LogicalSize::new(260.0, 260.0));
		let window = event_loop
			.create_window(attrs)
			.map_err(|err| format!("Unable to create loupe window: {err}"))?;
		let window = Arc::new(window);
		#[cfg(target_os = "macos")]
		let _ = window.set_cursor_hittest(true);
		#[cfg(not(target_os = "macos"))]
		let _ = window.set_cursor_hittest(false);

		window.set_transparent(true);

		#[cfg(target_os = "macos")]
		macos_configure_hud_window(
			window.as_ref(),
			self.macos_hud_window_blur_enabled(),
			self.config.hud_fog_amount,
			Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
		);

		#[cfg(not(target_os = "macos"))]
		window.set_blur(self.config.show_hud_blur);

		let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
		let renderer =
			WindowRenderer::new(gpu, Arc::clone(&window), Arc::clone(&self.egui_repaint_deadline))
				.map_err(|err| format!("Failed to init loupe renderer: {err:#}"))?;

		self.loupe_window = Some(HudOverlayWindow { window, renderer });

		Ok(())
	}

	fn create_toolbar_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		let attrs = winit::window::Window::default_attributes()
			.with_title("rsnap-toolbar")
			.with_decorations(false)
			.with_resizable(false)
			.with_inner_size(LogicalSize::new(
				TOOLBAR_EXPANDED_WIDTH_PX as f64,
				TOOLBAR_EXPANDED_HEIGHT_PX as f64,
			))
			.with_transparent(true)
			.with_visible(false)
			.with_window_level(WindowLevel::AlwaysOnTop);
		let window = event_loop
			.create_window(attrs)
			.map_err(|err| format!("Unable to create toolbar window: {err}"))?;
		let window = Arc::new(window);
		#[cfg(target_os = "macos")]
		let _ = window.set_cursor_hittest(true);
		#[cfg(not(target_os = "macos"))]
		let _ = window.set_cursor_hittest(false);

		window.set_transparent(true);

		#[cfg(target_os = "macos")]
		macos_configure_hud_window(
			window.as_ref(),
			self.macos_hud_window_blur_enabled(),
			self.config.hud_fog_amount,
			Some(f64::from(HUD_PILL_CORNER_RADIUS_POINTS)),
		);

		#[cfg(not(target_os = "macos"))]
		window.set_blur(self.config.show_hud_blur);
		window.request_redraw();

		let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
		let renderer =
			WindowRenderer::new(gpu, Arc::clone(&window), Arc::clone(&self.egui_repaint_deadline))
				.map_err(|err| format!("Failed to init toolbar renderer: {err:#}"))?;

		self.toolbar_window = Some(HudOverlayWindow { window, renderer });

		Ok(())
	}

	pub fn request_redraw_all(&self) {
		for w in self.windows.values() {
			w.window.request_redraw();
		}

		if let Some(hud) = self.hud_window.as_ref() {
			hud.window.request_redraw();
		}
		if let Some(loupe) = self.loupe_window.as_ref() {
			loupe.window.request_redraw();
		}
		if let Some(toolbar) = self.toolbar_window.as_ref() {
			toolbar.window.request_redraw();
		}
	}

	pub fn request_redraw_for_monitor(&self, monitor: MonitorRect) {
		for w in self.windows.values() {
			if w.monitor == monitor {
				w.window.request_redraw();
			}
		}

		if let Some(hud) = self.hud_window.as_ref() {
			hud.window.request_redraw();
		}
		if let Some(loupe) = self.loupe_window.as_ref() {
			loupe.window.request_redraw();
		}

		// macOS uses a native toolbar popup window with compositor blur; keep shader-viewport
		// toolbar redraw on the fullscreen overlay path disabled for this platform.
		// Future direction: if toolbar styling moves off native blur, add a dedicated capture
		// pass feeding a toolbar-local shader-blur texture.
		if cfg!(target_os = "macos")
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.toolbar_state.visible
			&& self.state.monitor == Some(monitor)
			&& self.state.frozen_image.is_some()
			&& self.pending_freeze_capture != Some(monitor)
		{
			self.request_redraw_toolbar_window();
		}
	}

	fn request_redraw_toolbar_window(&self) {
		if let Some(toolbar) = self.toolbar_window.as_ref() {
			toolbar.window.request_redraw();
		}
	}

	pub fn about_to_wait(&mut self) -> OverlayControl {
		self.maybe_request_keepalive_redraw();

		if self.is_active() {
			self.sync_alt_held_from_global_keys();
		}

		self.maybe_keep_frozen_capture_redraw();
		self.maybe_tick_toolbar_window_warmup_redraw();
		self.maybe_tick_live_cursor_tracking();
		self.maybe_tick_live_sampling();
		self.maybe_tick_frozen_cursor_tracking();

		self.drain_worker_responses()
	}

	fn consume_egui_repaint_deadline(&self, now: Instant) -> Option<Instant> {
		let mut next_repaint =
			self.egui_repaint_deadline.lock().unwrap_or_else(|err| err.into_inner());

		if let Some(deadline) = *next_repaint {
			if deadline <= now {
				*next_repaint = None;

				drop(next_repaint);

				self.request_redraw_all();

				Some(deadline)
			} else {
				Some(deadline)
			}
		} else {
			None
		}
	}

	fn schedule_egui_repaint_after(&self, delay: Duration) {
		let deadline = Instant::now() + delay;
		let mut next_repaint =
			self.egui_repaint_deadline.lock().unwrap_or_else(|err| err.into_inner());

		if next_repaint.is_none_or(|next| deadline < next) {
			*next_repaint = Some(deadline);
		}
	}

	fn maybe_keep_frozen_capture_redraw(&self) {
		if !matches!(self.state.mode, OverlayMode::Frozen) {
			return;
		}
		if self.state.frozen_image.is_some() {
			return;
		}

		// Keep producing redraw events while the frozen background is being captured.
		// On some platforms the worker response won't wake the winit event loop, so we
		// must ensure `handle_overlay_window_redraw` + `drain_worker_responses` keep
		// running even with no input events.
		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		} else {
			self.request_redraw_all();
		}

		self.schedule_egui_repaint_after(FROZEN_CAPTURE_POLL_INTERVAL);
	}

	fn maybe_tick_toolbar_window_warmup_redraw(&mut self) {
		if self.toolbar_window_warmup_redraws_remaining == 0 {
			return;
		}

		#[cfg(not(target_os = "macos"))]
		{
			self.toolbar_window_warmup_redraws_remaining = 0;

			return;
		}

		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.toolbar_state.visible
			|| self.state.frozen_image.is_none()
			|| self.state.monitor.is_none()
		{
			self.toolbar_window_warmup_redraws_remaining = 0;

			return;
		}

		self.toolbar_window_warmup_redraws_remaining =
			self.toolbar_window_warmup_redraws_remaining.saturating_sub(1);

		self.request_redraw_toolbar_window();
		self.schedule_egui_repaint_after(FROZEN_CAPTURE_POLL_INTERVAL);
	}

	fn maybe_tick_frozen_cursor_tracking(&mut self) {
		if !self.is_active() || !matches!(self.state.mode, OverlayMode::Frozen) {
			return;
		}

		let mouse = self.cursor_device.get_mouse();
		let raw = GlobalPoint::new(mouse.coords.0, mouse.coords.1);
		let old_monitor = self.active_cursor_monitor();
		let Some((monitor, global, source)) = self.resolve_device_cursor_point(raw) else {
			return;
		};

		if tracing::enabled!(tracing::Level::TRACE) {
			tracing::trace!(
				mode = "frozen",
				source = source.as_str(),
				monitor_id = monitor.id,
				"Resolved device cursor for frozen tick."
			);
		}
		if self.state.cursor == Some(global) && old_monitor == Some(monitor) {
			return;
		}

		self.update_cursor_state(monitor, global);
		self.update_hud_window_position(monitor, global);

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		self.request_redraw_for_monitor(monitor);
	}

	fn maybe_tick_live_cursor_tracking(&mut self) {
		if !self.is_active() || !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		#[cfg(not(target_os = "macos"))]
		{
			// Keep this loop alive even if CursorMoved events are sparse on non-macOS.
			self.schedule_egui_repaint_after(Duration::from_millis(16));
		}

		let mouse = self.cursor_device.get_mouse();
		let raw = GlobalPoint::new(mouse.coords.0, mouse.coords.1);
		let old_monitor = self.active_cursor_monitor();
		let Some((monitor, global, source)) = self.resolve_live_cursor_point(raw) else {
			return;
		};

		if tracing::enabled!(tracing::Level::TRACE) {
			tracing::trace!(
				mode = "live",
				source = source.as_str(),
				monitor_id = monitor.id,
				"Resolved device cursor for live tick."
			);
		}
		if self.state.cursor == Some(global) && old_monitor == Some(monitor) {
			return;
		}

		self.update_cursor_state(monitor, global);
		self.update_hud_window_position(monitor, global);

		if self.use_fake_hud_blur() {
			if self.state.live_bg_monitor != Some(monitor) {
				self.state.live_bg_monitor = None;
				self.state.live_bg_image = None;
			}

			self.maybe_request_live_bg(monitor);
		}

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		self.request_redraw_for_monitor(monitor);
	}

	fn maybe_request_keepalive_redraw(&mut self) {
		// Avoid a tight present loop if the OS delivers spurious redraws.
		if self.is_active() && self.last_present_at.elapsed() > Duration::from_secs(30) {
			self.request_redraw_all();
		}
	}

	fn maybe_tick_live_sampling(&mut self) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		let Some(cursor) = self.state.cursor else {
			return;
		};
		let Some(monitor) = self.active_cursor_monitor() else {
			return;
		};

		self.record_live_sample_stall(cursor, monitor);

		if self.use_fake_hud_blur() {
			self.maybe_request_live_bg(monitor);
		}

		let Some(_worker) = &self.worker else {
			return;
		};

		self.request_live_samples_for_cursor(monitor, cursor);
	}

	fn drain_worker_responses(&mut self) -> OverlayControl {
		if self.worker.is_none() {
			return OverlayControl::Continue;
		}

		if let Some(image) = self.pending_encode_png.take()
			&& let Some(worker) = self.worker.as_ref()
			&& let Err(image) = worker.request_encode_png(image)
		{
			self.pending_encode_png = Some(image);
		}

		while let Some(resp) = self.worker.as_ref().and_then(|worker| worker.try_recv()) {
			let control = self.maybe_tick_worker_response_limiter(resp);

			if !matches!(control, OverlayControl::Continue) {
				return control;
			}
		}

		OverlayControl::Continue
	}

	fn request_live_samples_for_cursor(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		self.send_rgb_sample_request(monitor, cursor, true);

		if self.state.alt_held {
			self.send_loupe_sample_request(
				monitor,
				cursor,
				self.loupe_patch_width_px,
				self.loupe_patch_height_px,
				true,
			);
		}
	}

	fn record_live_sample_stall(&mut self, cursor: GlobalPoint, monitor: MonitorRect) {
		let now = Instant::now();

		match self.last_live_sample_cursor {
			Some(last_cursor) if last_cursor == cursor => {
				let stall_started_at = self.live_sample_stall_started_at;

				if self.live_sample_stall_started_at.is_none() {
					self.live_sample_stall_started_at = Some(now);
				} else if stall_started_at
					.is_some_and(|start| now.duration_since(start) >= Duration::from_millis(100))
					&& self.last_live_sample_stall_log_at.is_none_or(|last_log| {
						now.duration_since(last_log) >= Duration::from_millis(250)
					}) {
					let Some(stall_started_at) = self.live_sample_stall_started_at else {
						return;
					};

					tracing::debug!(
						cursor = ?cursor,
						monitor_id = monitor.id,
						stall_duration_ms = now.duration_since(stall_started_at).as_millis(),
						"Live sampling cursor unchanged while sampling ticks continue."
					);

					self.last_live_sample_stall_log_at = Some(now);
				}
			},
			Some(_) => {
				self.live_sample_stall_started_at = None;
				self.last_live_sample_stall_log_at = None;
			},
			None => {
				self.live_sample_stall_started_at = Some(now);
			},
		}

		self.last_live_sample_cursor = Some(cursor);
	}

	fn send_rgb_sample_request(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		respect_interval: bool,
	) {
		if respect_interval && self.last_rgb_request_at.elapsed() < self.rgb_request_interval {
			return;
		}

		let request_id = self.rgb_send_sequence + 1;
		let now = Instant::now();

		self.rgb_send_sequence = request_id;

		let Some(worker) = self.worker.as_ref() else {
			return;
		};

		match worker.try_sample_rgb(monitor, point) {
			Ok(()) => {
				self.pending_rgb_request_id = Some(request_id);
				self.pending_rgb_request_at = Some(now);

				if tracing::enabled!(tracing::Level::TRACE) {
					tracing::trace!(
						request_id,
						monitor_id = monitor.id,
						point = ?point,
						"RGB sample request sent."
					);
				}
			},
			Err(WorkerRequestSendError::Full) => {
				self.rgb_send_full_count = self.rgb_send_full_count.saturating_add(1);

				tracing::debug!(
					request_id,
					monitor_id = monitor.id,
					point = ?point,
					full_count = self.rgb_send_full_count,
					"RGB sample request dropped: worker queue full."
				);
			},
			Err(WorkerRequestSendError::Disconnected) => {
				self.rgb_send_disconnected_count =
					self.rgb_send_disconnected_count.saturating_add(1);

				tracing::debug!(
					request_id,
					monitor_id = monitor.id,
					point = ?point,
					disconnected_count = self.rgb_send_disconnected_count,
					"RGB sample request dropped: worker disconnected."
				);
			},
		}

		self.last_rgb_request_at = now;
	}

	fn send_loupe_sample_request(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		width_px: u32,
		height_px: u32,
		respect_interval: bool,
	) {
		if respect_interval && self.last_loupe_request_at.elapsed() < self.loupe_request_interval {
			return;
		}

		let request_id = self.loupe_send_sequence + 1;
		let now = Instant::now();

		self.loupe_send_sequence = request_id;

		let Some(worker) = self.worker.as_ref() else {
			return;
		};

		match worker.try_sample_loupe(monitor, point, width_px, height_px) {
			Ok(()) => {
				self.pending_loupe_request_id = Some(request_id);
				self.pending_loupe_request_at = Some(now);

				if tracing::enabled!(tracing::Level::TRACE) {
					tracing::trace!(
						request_id,
						monitor_id = monitor.id,
						point = ?point,
						"Loupe sample request sent."
					);
				}
			},
			Err(WorkerRequestSendError::Full) => {
				self.loupe_send_full_count = self.loupe_send_full_count.saturating_add(1);

				tracing::debug!(
					request_id,
					monitor_id = monitor.id,
					point = ?point,
					full_count = self.loupe_send_full_count,
					"Loupe sample request dropped: worker queue full."
				);
			},
			Err(WorkerRequestSendError::Disconnected) => {
				self.loupe_send_disconnected_count =
					self.loupe_send_disconnected_count.saturating_add(1);

				tracing::debug!(
					request_id,
					monitor_id = monitor.id,
					point = ?point,
					disconnected_count = self.loupe_send_disconnected_count,
					"Loupe sample request dropped: worker disconnected."
				);
			},
		}

		self.last_loupe_request_at = now;
	}

	fn maybe_tick_worker_response_limiter(&mut self, resp: WorkerResponse) -> OverlayControl {
		match resp {
			WorkerResponse::SampledLoupe { monitor, point, rgb, patch } => {
				self.handle_sampled_loupe_response(monitor, point, rgb, patch);

				OverlayControl::Continue
			},
			WorkerResponse::SampledRgb { monitor, point, rgb } => {
				let _ = point;

				self.handle_sampled_rgb_response(monitor, rgb);

				OverlayControl::Continue
			},
			WorkerResponse::CapturedFreeze { monitor, image } => {
				self.handle_captured_freeze_response(monitor, image);

				OverlayControl::Continue
			},
			WorkerResponse::Error(message) => {
				self.restore_capture_windows_visibility();
				self.state.set_error(message);
				self.request_redraw_all();

				OverlayControl::Continue
			},
			WorkerResponse::EncodedPng { png_bytes } => self.handle_encoded_png_response(png_bytes),
		}
	}

	fn handle_sampled_loupe_response(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		rgb: Option<crate::state::Rgb>,
		patch: Option<RgbaImage>,
	) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		self.state.rgb = rgb;
		self.state.loupe = patch.map(|patch| crate::state::LoupeSample { center: point, patch });

		if let Some(request_id) = self.pending_loupe_request_id.take()
			&& let Some(sent_at) = self.pending_loupe_request_at
		{
			let latency = sent_at.elapsed().as_millis();

			if tracing::enabled!(tracing::Level::TRACE) {
				tracing::trace!(
					request_id,
					monitor_id = monitor.id,
					lag_ms = latency,
					"Loupe sample response latency."
				);
			}

			self.pending_loupe_request_at = None;
		}

		self.redraw_for_monitor_and_current(monitor);
	}

	fn handle_sampled_rgb_response(
		&mut self,
		monitor: MonitorRect,
		rgb: Option<crate::state::Rgb>,
	) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		if let Some(request_id) = self.pending_rgb_request_id.take()
			&& let Some(sent_at) = self.pending_rgb_request_at
		{
			let latency = sent_at.elapsed().as_millis();

			if tracing::enabled!(tracing::Level::TRACE) {
				tracing::trace!(
					request_id,
					monitor_id = monitor.id,
					lag_ms = latency,
					"RGB sample response latency."
				);
			}

			self.pending_rgb_request_at = None;
		}

		self.state.rgb = rgb;

		self.redraw_for_monitor_and_current(monitor);
	}

	fn redraw_for_monitor_and_current(&mut self, monitor: MonitorRect) {
		let current_monitor = self.active_cursor_monitor();

		if let Some(current_monitor) = current_monitor {
			self.request_redraw_for_monitor(current_monitor);
		}

		if current_monitor != Some(monitor) {
			self.request_redraw_for_monitor(monitor);
		}
	}

	fn handle_captured_freeze_response(&mut self, monitor: MonitorRect, image: RgbaImage) {
		if matches!(self.state.mode, OverlayMode::Frozen) && self.state.monitor == Some(monitor) {
			self.state.finish_freeze(monitor, image);
			self.restore_capture_windows_visibility();

			self.toolbar_state.needs_redraw = true;

			#[cfg(target_os = "macos")]
			if self.toolbar_state.visible {
				self.toolbar_window_warmup_redraws_remaining =
					self.toolbar_window_warmup_redraws_remaining.max(TOOLBAR_WINDOW_WARMUP_REDRAWS);
			}

			if let Some(cursor) = self.state.cursor {
				self.state.rgb = frozen_rgb(&self.state.frozen_image, Some(monitor), cursor);
				self.state.loupe = frozen_loupe_patch(
					&self.state.frozen_image,
					Some(monitor),
					cursor,
					self.loupe_patch_width_px,
					self.loupe_patch_height_px,
				)
				.map(|patch| crate::state::LoupeSample { center: cursor, patch });
			}

			self.request_redraw_for_monitor(monitor);
			self.raise_hud_windows();

			return;
		}
		if matches!(self.state.mode, OverlayMode::Live)
			&& self.use_fake_hud_blur()
			&& self.active_cursor_monitor() == Some(monitor)
		{
			self.state.live_bg_monitor = Some(monitor);
			self.state.live_bg_image = Some(image);
			self.state.live_bg_generation = self.state.live_bg_generation.wrapping_add(1);

			self.request_redraw_for_monitor(monitor);
		}
	}

	fn handle_encoded_png_response(&mut self, png_bytes: Vec<u8>) -> OverlayControl {
		match write_png_bytes_to_clipboard(&png_bytes) {
			Ok(()) => self.exit(OverlayExit::PngBytes(png_bytes)),
			Err(err) => {
				self.state.set_error(format!("{err:#}"));
				self.request_redraw_all();

				OverlayControl::Continue
			},
		}
	}

	pub fn handle_window_event(
		&mut self,
		window_id: WindowId,
		event: &WindowEvent,
	) -> OverlayControl {
		let toolbar_window_id = self
			.toolbar_window
			.as_ref()
			.is_some_and(|toolbar_window| toolbar_window.window.id() == window_id);

		match event {
			WindowEvent::CloseRequested => self.exit(OverlayExit::Cancelled),
			WindowEvent::Resized(size) if toolbar_window_id => {
				self.handle_toolbar_window_resized(*size)
			},
			WindowEvent::Resized(size) => self.handle_resized(window_id, *size),
			WindowEvent::ScaleFactorChanged { .. } if toolbar_window_id => {
				self.handle_toolbar_window_scale_factor_changed(window_id)
			},
			WindowEvent::ScaleFactorChanged { .. } => self.handle_scale_factor_changed(window_id),
			WindowEvent::CursorMoved { position, .. } => {
				if toolbar_window_id {
					return self.handle_toolbar_cursor_moved(window_id, *position);
				}

				self.handle_cursor_moved(window_id, *position)
			},
			WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
				if toolbar_window_id {
					return self.handle_toolbar_mouse_input(*state);
				}

				self.handle_left_mouse_input(window_id, *state)
			},
			WindowEvent::RedrawRequested if toolbar_window_id => {
				self.handle_toolbar_window_redraw_requested()
			},
			WindowEvent::ThemeChanged(_) => {
				// Keep the HUD palette in sync with system changes when ThemeMode::System is active.
				if let Some(monitor) = self.windows.get(&window_id).map(|w| w.monitor) {
					self.request_redraw_for_monitor(monitor);
				} else {
					self.request_redraw_all();
				}

				OverlayControl::Continue
			},
			WindowEvent::KeyboardInput { event, .. } => self.handle_key_event(event),
			WindowEvent::ModifiersChanged(modifiers) => self.handle_modifiers_changed(modifiers),
			WindowEvent::RedrawRequested => self.handle_redraw_requested(window_id),
			_ => OverlayControl::Continue,
		}
	}

	fn handle_toolbar_mouse_input(&mut self, state: ElementState) -> OverlayControl {
		let toolbar_left_button_down = matches!(state, ElementState::Pressed);

		if toolbar_left_button_down == self.toolbar_left_button_down {
			return OverlayControl::Continue;
		}

		self.toolbar_left_button_down = toolbar_left_button_down;

		if !toolbar_left_button_down {
			self.toolbar_pointer_local = None;
			self.toolbar_state.dragging = false;
			self.toolbar_state.drag_offset = Vec2::ZERO;
			self.toolbar_state.drag_anchor = None;
		} else {
			self.toolbar_state.drag_offset = Vec2::ZERO;
			self.toolbar_state.dragging = false;
			self.toolbar_state.drag_anchor = None;
		}

		#[cfg(target_os = "macos")]
		{
			self.request_redraw_toolbar_window();
		}

		OverlayControl::Continue
	}

	fn reset_toolbar_pointer_state(&mut self) {
		self.toolbar_left_button_down = false;
		self.toolbar_left_button_down_prev = false;
		self.toolbar_pointer_local = None;
		self.toolbar_state.drag_anchor = None;
	}

	fn handle_toolbar_cursor_moved(
		&mut self,
		window_id: WindowId,
		position: PhysicalPosition<f64>,
	) -> OverlayControl {
		let Some(toolbar_window) = self.toolbar_window.as_ref() else {
			return OverlayControl::Continue;
		};

		if toolbar_window.window.id() != window_id
			|| !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.toolbar_state.visible
		{
			return OverlayControl::Continue;
		}

		let scale = toolbar_window.window.scale_factor().max(1.0);
		let cursor_local = Pos2::new((position.x / scale) as f32, (position.y / scale) as f32);

		self.toolbar_pointer_local = Some(cursor_local);

		let monitor = match self.state.monitor.or_else(|| self.active_cursor_monitor()) {
			Some(monitor) => monitor,
			None => return OverlayControl::Continue,
		};
		let global_cursor = self.toolbar_cursor_global_position(toolbar_window, cursor_local);
		let drag_monitor =
			global_cursor.and_then(|cursor| self.monitor_at(cursor)).unwrap_or(monitor);
		let mut mouse_drag = self.toolbar_left_button_down && self.toolbar_state.dragging;

		if self.toolbar_left_button_down && self.toolbar_state.drag_anchor.is_none() {
			self.toolbar_state.drag_anchor = Some(cursor_local);
		}
		if !mouse_drag && let Some(drag_anchor) = self.toolbar_state.drag_anchor {
			let dx = cursor_local.x - drag_anchor.x;
			let dy = cursor_local.y - drag_anchor.y;
			let threshold_sq = TOOLBAR_DRAG_START_THRESHOLD_PX * TOOLBAR_DRAG_START_THRESHOLD_PX;

			if dx * dx + dy * dy >= threshold_sq {
				let toolbar_outer_pos = self.toolbar_outer_pos.or_else(|| {
					self.toolbar_state.floating_position.map(|floating_position| {
						GlobalPoint::new(
							monitor.origin.x.saturating_add(floating_position.x.round() as i32),
							monitor.origin.y.saturating_add(floating_position.y.round() as i32),
						)
					})
				});

				if let (Some(global_cursor), Some(toolbar_outer_pos)) =
					(global_cursor, toolbar_outer_pos)
				{
					self.toolbar_state.drag_offset = Vec2::new(
						global_cursor.x as f32 - toolbar_outer_pos.x as f32,
						global_cursor.y as f32 - toolbar_outer_pos.y as f32,
					);
					self.toolbar_state.dragging = true;
					self.toolbar_state.drag_anchor = None;
					mouse_drag = true;
				}
			}
		}
		if mouse_drag && global_cursor.is_none() {
			mouse_drag = false;
		}
		if mouse_drag && let Some(global_cursor) = global_cursor {
			let desired_global = Pos2::new(
				global_cursor.x as f32 - self.toolbar_state.drag_offset.x,
				global_cursor.y as f32 - self.toolbar_state.drag_offset.y,
			);
			let desired_local = Pos2::new(
				desired_global.x - drag_monitor.origin.x as f32,
				desired_global.y - drag_monitor.origin.y as f32,
			);
			let _ = self.update_toolbar_outer_position(drag_monitor, desired_local);
		}

		self.request_redraw_toolbar_window();

		OverlayControl::Continue
	}

	fn toolbar_cursor_global_position(
		&self,
		toolbar_window: &HudOverlayWindow,
		cursor_local: Pos2,
	) -> Option<GlobalPoint> {
		let toolbar_scale = toolbar_window.window.scale_factor().max(1.0);
		let outer_position = toolbar_window.window.outer_position().ok()?;
		let global_cursor = Pos2::new(
			(outer_position.x as f64 / toolbar_scale) as f32 + cursor_local.x,
			(outer_position.y as f64 / toolbar_scale) as f32 + cursor_local.y,
		);

		Some(GlobalPoint::new(global_cursor.x.round() as i32, global_cursor.y.round() as i32))
	}

	fn handle_toolbar_window_resized(&mut self, size: PhysicalSize<u32>) -> OverlayControl {
		let Some(toolbar_window) = self.toolbar_window.as_mut() else {
			return OverlayControl::Continue;
		};

		match toolbar_window.renderer.resize(size) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	fn handle_toolbar_window_scale_factor_changed(
		&mut self,
		window_id: WindowId,
	) -> OverlayControl {
		let macos_hud_window_blur_enabled = self.macos_hud_window_blur_enabled();
		let Some(toolbar_window) = self
			.toolbar_window
			.as_mut()
			.filter(|toolbar_window| toolbar_window.window.id() == window_id)
		else {
			return OverlayControl::Continue;
		};
		let size = toolbar_window.window.inner_size();

		match toolbar_window.renderer.resize(size) {
			Ok(()) => {
				#[cfg(target_os = "macos")]
				macos_configure_hud_window(
					toolbar_window.window.as_ref(),
					macos_hud_window_blur_enabled,
					self.config.hud_fog_amount,
					Some(f64::from(HUD_PILL_CORNER_RADIUS_POINTS)),
				);

				OverlayControl::Continue
			},
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	fn handle_toolbar_window_redraw_requested(&mut self) -> OverlayControl {
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};
		let Some(monitor) = self.state.monitor else {
			return OverlayControl::Continue;
		};
		let toolbar_input = self.toolbar_pointer_state(monitor, self.toolbar_pointer_local);
		let Some(toolbar_window) = self.toolbar_window.as_mut() else {
			return OverlayControl::Continue;
		};

		#[cfg(not(target_os = "macos"))]
		{
			toolbar_window.window.set_visible(false);

			self.last_present_at = Instant::now();

			return OverlayControl::Continue;
		}

		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.toolbar_state.visible
			|| self.state.frozen_image.is_none()
			|| self.pending_freeze_capture == Some(monitor)
		{
			toolbar_window.window.set_visible(false);

			self.toolbar_window_visible = false;
			self.toolbar_window_warmup_redraws_remaining = 0;
			self.last_present_at = Instant::now();

			return OverlayControl::Continue;
		}

		toolbar_window.window.set_visible(true);

		if !self.toolbar_window_visible {
			self.toolbar_window_visible = true;
			self.toolbar_window_warmup_redraws_remaining = TOOLBAR_WINDOW_WARMUP_REDRAWS;
		}

		let previous_floating_position = self.toolbar_state.floating_position;

		self.toolbar_state.floating_position = Some(Pos2::ZERO);

		let draw_result = toolbar_window.renderer.draw(
			gpu,
			&self.state,
			monitor,
			false,
			Some(Pos2::ZERO),
			false,
			HudAnchor::Cursor,
			self.config.show_alt_hint_keycap,
			false,
			self.config.hud_opaque,
			self.config.hud_opacity,
			self.config.hud_fog_amount,
			self.config.hud_milk_amount,
			self.config.hud_tint_hue,
			self.config.theme_mode,
			false,
			Some(&mut self.toolbar_state),
			toolbar_input,
		);

		self.toolbar_state.floating_position = previous_floating_position;

		if let Err(err) = draw_result {
			return self.exit(OverlayExit::Error(format!("{err:#}")));
		}
		if let Some(hud_pill) = toolbar_window.renderer.hud_pill {
			let desired_w = hud_pill.rect.width().ceil().max(1.0) as u32;
			let desired_h = hud_pill.rect.height().ceil().max(1.0) as u32;
			let desired = (desired_w, desired_h);

			if self.toolbar_inner_size_points != Some(desired) {
				self.toolbar_inner_size_points = Some(desired);

				let _ = toolbar_window.window.request_inner_size(LogicalSize::new(
					f64::from(desired_w),
					f64::from(desired_h),
				));
			}

			if let Some(toolbar_pos) = self.toolbar_state.floating_position {
				let _ = self.update_toolbar_outer_position(monitor, toolbar_pos);
			}
		}

		self.last_present_at = Instant::now();
		self.toolbar_left_button_down_prev = self.toolbar_left_button_down;

		if self.toolbar_state.needs_redraw {
			self.toolbar_state.needs_redraw = false;

			self.request_redraw_toolbar_window();
		}

		OverlayControl::Continue
	}

	fn handle_modifiers_changed(&mut self, modifiers: &winit::event::Modifiers) -> OverlayControl {
		let alt = self.resolve_alt_modifier_state(modifiers.state().alt_key());

		match self.config.alt_activation {
			AltActivationMode::Hold => self.set_alt_held(alt),
			AltActivationMode::Toggle => {
				if alt && !self.alt_modifier_down {
					self.set_alt_held(!self.state.alt_held);
				}
			},
		}

		self.alt_modifier_down = alt;

		if let Some(monitor) = self.active_cursor_monitor() {
			self.request_redraw_for_monitor(monitor);
		} else {
			self.request_redraw_all();
		}

		OverlayControl::Continue
	}

	fn resolve_alt_modifier_state(&mut self, alt: bool) -> bool {
		let transient_alt_release = !alt
			&& self.state.alt_held
			&& self
				.last_alt_press_at
				.is_some_and(|press| press.elapsed() <= Duration::from_millis(120))
			&& self.is_option_key_down();

		if transient_alt_release { true } else { alt }
	}

	fn is_option_key_down(&self) -> bool {
		let keys = self.cursor_device.get_keys();

		keys.contains(&Keycode::LOption)
			|| keys.contains(&Keycode::ROption)
			|| keys.contains(&Keycode::LAlt)
			|| keys.contains(&Keycode::RAlt)
	}

	fn sync_alt_held_from_global_keys(&mut self) {
		if matches!(self.config.alt_activation, AltActivationMode::Hold)
			&& self.state.alt_held
			&& !self.is_option_key_down()
		{
			self.set_alt_held(false);
		}
		if !self.is_option_key_down() {
			self.alt_modifier_down = false;
		}
	}

	fn set_alt_held(&mut self, alt: bool) {
		if self.state.alt_held == alt {
			return;
		}

		self.state.alt_held = alt;

		if !alt {
			self.handle_alt_release();

			return;
		}

		let Some(cursor) = self.state.cursor else {
			return;
		};
		let Some(monitor) = self.active_cursor_monitor() else {
			return;
		};

		self.last_alt_press_at = Some(Instant::now());

		self.set_alt_loupe_window_visible(Some(monitor), true);

		if self.use_fake_hud_blur() {
			self.maybe_request_live_bg(monitor);
		}

		match self.state.mode {
			OverlayMode::Live => self.request_live_alt_samples(monitor, cursor),
			OverlayMode::Frozen => self.request_frozen_alt_samples(cursor),
		}
	}

	fn handle_alt_release(&mut self) {
		self.last_alt_press_at = None;
		self.state.loupe = None;
		self.loupe_outer_pos = None;

		self.set_alt_loupe_window_visible(None, false);

		if let Some(monitor) = self.active_cursor_monitor() {
			self.request_redraw_for_monitor(monitor);
		}
	}

	fn set_alt_loupe_window_visible(&mut self, monitor: Option<MonitorRect>, visible: bool) {
		if visible {
			let Some(monitor) = monitor else {
				return;
			};
			let visible = self.update_loupe_window_position(monitor);

			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(visible);
				loupe_window.window.request_redraw();
			}

			return;
		}

		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(false);
			loupe_window.window.request_redraw();
		}
	}

	fn request_live_alt_samples(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		self.send_rgb_sample_request(monitor, cursor, false);
		self.send_loupe_sample_request(
			monitor,
			cursor,
			self.loupe_patch_width_px,
			self.loupe_patch_height_px,
			false,
		);
	}

	fn request_frozen_alt_samples(&mut self, cursor: GlobalPoint) {
		if let (Some(frozen_monitor), Some(_)) =
			(self.state.monitor, self.state.frozen_image.as_ref())
		{
			self.state.loupe = frozen_loupe_patch(
				&self.state.frozen_image,
				Some(frozen_monitor),
				cursor,
				self.loupe_patch_width_px,
				self.loupe_patch_height_px,
			)
			.map(|patch| crate::state::LoupeSample { center: cursor, patch });

			self.request_redraw_for_monitor(frozen_monitor);
		}
	}

	fn handle_resized(&mut self, window_id: WindowId, size: PhysicalSize<u32>) -> OverlayControl {
		let macos_hud_window_blur_enabled = self.macos_hud_window_blur_enabled();
		let window_scale_factor = self
			.windows
			.get(&window_id)
			.map(|w| w.window.scale_factor())
			.or_else(|| self.hud_window.as_ref().map(|w| w.window.scale_factor()))
			.or_else(|| self.loupe_window.as_ref().map(|w| w.window.scale_factor()));

		tracing::trace!(?window_id, ?size, ?window_scale_factor, "WindowEvent::Resized");

		if let Some(hud_window) = self.hud_window.as_mut()
			&& hud_window.window.id() == window_id
		{
			match hud_window.renderer.resize(size) {
				Ok(()) => {
					#[cfg(target_os = "macos")]
					macos_configure_hud_window(
						hud_window.window.as_ref(),
						macos_hud_window_blur_enabled,
						self.config.hud_fog_amount,
						None,
					);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}
		if let Some(loupe_window) = self.loupe_window.as_mut()
			&& loupe_window.window.id() == window_id
		{
			match loupe_window.renderer.resize(size) {
				Ok(()) => {
					#[cfg(target_os = "macos")]
					macos_configure_hud_window(
						loupe_window.window.as_ref(),
						macos_hud_window_blur_enabled,
						self.config.hud_fog_amount,
						Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
					);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}

		let Some(overlay_window) = self.windows.get_mut(&window_id) else {
			return OverlayControl::Continue;
		};

		match overlay_window.renderer.resize(size) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	fn handle_scale_factor_changed(&mut self, window_id: WindowId) -> OverlayControl {
		let macos_hud_window_blur_enabled = self.macos_hud_window_blur_enabled();
		let window_scale_factor = self
			.windows
			.get(&window_id)
			.map(|w| w.window.scale_factor())
			.or_else(|| self.hud_window.as_ref().map(|w| w.window.scale_factor()))
			.or_else(|| self.loupe_window.as_ref().map(|w| w.window.scale_factor()));

		tracing::trace!(?window_id, ?window_scale_factor, "WindowEvent::ScaleFactorChanged");

		if let Some(hud_window) = self.hud_window.as_mut()
			&& hud_window.window.id() == window_id
		{
			let size = hud_window.window.inner_size();

			match hud_window.renderer.resize(size) {
				Ok(()) => {
					#[cfg(target_os = "macos")]
					macos_configure_hud_window(
						hud_window.window.as_ref(),
						macos_hud_window_blur_enabled,
						self.config.hud_fog_amount,
						None,
					);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}
		if let Some(loupe_window) = self.loupe_window.as_mut()
			&& loupe_window.window.id() == window_id
		{
			let size = loupe_window.window.inner_size();

			match loupe_window.renderer.resize(size) {
				Ok(()) => {
					#[cfg(target_os = "macos")]
					macos_configure_hud_window(
						loupe_window.window.as_ref(),
						macos_hud_window_blur_enabled,
						self.config.hud_fog_amount,
						Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
					);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}

		let Some(overlay_window) = self.windows.get_mut(&window_id) else {
			return OverlayControl::Continue;
		};
		let size = overlay_window.window.inner_size();

		match overlay_window.renderer.resize(size) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	fn handle_cursor_moved(
		&mut self,
		window_id: WindowId,
		position: PhysicalPosition<f64>,
	) -> OverlayControl {
		let old_monitor = self.active_cursor_monitor();
		let now = Instant::now();
		let Some((window_monitor, scale_factor)) =
			self.windows.get(&window_id).map(|w| (w.monitor, w.window.scale_factor()))
		else {
			return self.handle_cursor_moved_without_overlay_window(window_id, old_monitor);
		};
		// Prefer global OS coordinates and fall back to the event cursor when needed.
		let local_x = (position.x / scale_factor).round() as i32;
		let local_y = (position.y / scale_factor).round() as i32;
		let event_global =
			GlobalPoint::new(window_monitor.origin.x + local_x, window_monitor.origin.y + local_y);
		let event_monitor = self.monitor_at(event_global);

		if let Some(event_monitor) = event_monitor {
			self.last_event_cursor = Some((event_monitor, event_global));
			self.last_event_cursor_at = Some(now);
		}

		let device_cursor = self.current_device_cursor();
		let Some((monitor, global, source)) = self.resolve_live_cursor_point(device_cursor) else {
			return OverlayControl::Continue;
		};
		let old_cursor = self.state.cursor;

		self.trace_cursor_moved_with_mapping(
			window_id,
			position,
			device_cursor,
			event_global,
			old_cursor,
			monitor,
			global,
			source,
		);
		self.update_cursor_for_live_move(monitor, global);
		self.request_cursor_move_samples(monitor, global);

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		self.request_redraw_for_monitor(monitor);

		OverlayControl::Continue
	}

	fn handle_cursor_moved_without_overlay_window(
		&mut self,
		window_id: WindowId,
		old_monitor: Option<MonitorRect>,
	) -> OverlayControl {
		let mouse = self.cursor_device.get_mouse();
		let raw = GlobalPoint::new(mouse.coords.0, mouse.coords.1);
		let Some((monitor, global, source)) = self.resolve_device_cursor_point(raw) else {
			return OverlayControl::Continue;
		};
		let old_cursor = self.state.cursor;

		if tracing::enabled!(tracing::Level::TRACE) {
			tracing::trace!(
				window_id = ?window_id,
				window_known = false,
				old_cursor = ?old_cursor,
				device_cursor = ?global,
				event_cursor = ?global,
				source = source.as_str(),
				"CursorMoved (no overlay window mapping)."
			);
		}

		self.update_cursor_state(monitor, global);
		self.update_hud_window_position(monitor, global);

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		self.request_redraw_for_monitor(monitor);

		OverlayControl::Continue
	}

	fn current_device_cursor(&self) -> GlobalPoint {
		let mouse = self.cursor_device.get_mouse();

		GlobalPoint::new(mouse.coords.0, mouse.coords.1)
	}

	fn trace_cursor_moved_with_mapping(
		&self,
		window_id: WindowId,
		position: PhysicalPosition<f64>,
		device_cursor: GlobalPoint,
		event_global: GlobalPoint,
		old_cursor: Option<GlobalPoint>,
		monitor: MonitorRect,
		global: GlobalPoint,
		source: DeviceCursorPointSource,
	) {
		if !tracing::enabled!(tracing::Level::TRACE) {
			return;
		}

		let delta_x = global.x.abs_diff(old_cursor.map_or(global.x, |point| point.x));
		let delta_y = global.y.abs_diff(old_cursor.map_or(global.y, |point| point.y));

		tracing::trace!(
			window_id = ?window_id,
			window_known = true,
			window_position = ?position,
			old_cursor = ?old_cursor,
			device_cursor = ?device_cursor,
			event_cursor = ?event_global,
			source = source.as_str(),
			monitor_id = monitor.id,
			cursor_delta_x = delta_x,
			cursor_delta_y = delta_y,
			"CursorMoved coordinate source: {}.",
			source.as_str()
		);
	}

	fn update_cursor_for_live_move(&mut self, monitor: MonitorRect, global: GlobalPoint) {
		self.update_cursor_state(monitor, global);
		self.update_hud_window_position(monitor, global);

		if matches!(self.state.mode, OverlayMode::Live) && self.use_fake_hud_blur() {
			if self.state.live_bg_monitor != Some(monitor) {
				self.state.live_bg_monitor = None;
				self.state.live_bg_image = None;
			}

			self.maybe_request_live_bg(monitor);
		}
	}

	fn request_cursor_move_samples(&mut self, monitor: MonitorRect, global: GlobalPoint) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		self.request_live_samples_for_cursor(monitor, global);
	}

	fn handle_left_mouse_input(
		&mut self,
		window_id: WindowId,
		state: ElementState,
	) -> OverlayControl {
		let monitor = self
			.windows
			.get(&window_id)
			.map(|w| w.monitor)
			.or_else(|| self.active_cursor_monitor())
			.or(self.state.monitor);
		let Some(monitor) = monitor else {
			return OverlayControl::Continue;
		};

		if matches!(self.state.mode, OverlayMode::Frozen) {
			self.reset_toolbar_pointer_state();
			self.request_redraw_for_monitor(monitor);

			return OverlayControl::Continue;
		}
		if state != ElementState::Pressed || !matches!(self.state.mode, OverlayMode::Live) {
			return OverlayControl::Continue;
		}

		self.reset_toolbar_pointer_state();

		let frozen_rgb = self.state.rgb;
		let frozen_loupe = self.state.loupe.as_ref().map(|loupe| crate::state::LoupeSample {
			center: loupe.center,
			patch: loupe.patch.clone(),
		});

		self.state.clear_error();
		self.state.begin_freeze(monitor);

		tracing::debug!(
			monitor_id = monitor.id,
			origin = ?monitor.origin,
			width_points = monitor.width,
			height_points = monitor.height,
			monitor_scale_factor = monitor.scale_factor(),
			cursor = ?self.state.cursor,
			"Freeze begin."
		);

		self.toolbar_state.floating_position = None;
		self.toolbar_state.dragging = false;
		self.toolbar_state.needs_redraw = true;
		self.toolbar_state.pill_height_points = None;
		self.toolbar_state.layout_last_screen_size_points = None;
		self.toolbar_state.layout_stable_frames = 0;
		// Spawn the toolbar immediately at the default position (fullscreen capture = bottom
		// centered with margin). This avoids any dependency on egui viewport stabilization or
		// additional input events (mouse move) to "finish" the initial layout.
		{
			let screen_rect = Rect::from_min_size(
				Pos2::ZERO,
				Vec2::new(monitor.width as f32, monitor.height as f32),
			);
			let capture_rect = screen_rect;
			let toolbar_size = Vec2::new(TOOLBAR_EXPANDED_WIDTH_PX, TOOLBAR_EXPANDED_HEIGHT_PX);
			let default_pos =
				WindowRenderer::frozen_toolbar_default_pos(screen_rect, capture_rect, toolbar_size);

			self.toolbar_state.floating_position = Some(default_pos);

			let _ = self.update_toolbar_outer_position(monitor, default_pos);

			tracing::debug!(
				monitor_id = monitor.id,
				frozen_generation = self.state.frozen_generation,
				toolbar_size_points = ?toolbar_size,
				default_pos = ?default_pos,
				"Frozen toolbar default position preseeded."
			);
		}

		self.request_redraw_toolbar_window();

		self.state.rgb = frozen_rgb;
		self.state.loupe = frozen_loupe;
		self.pending_freeze_capture = Some(monitor);
		self.pending_freeze_capture_armed = false;
		self.capture_windows_hidden = false;

		self.schedule_egui_repaint_after(FROZEN_CAPTURE_POLL_INTERVAL);
		self.request_redraw_for_monitor(monitor);

		if self.use_fake_hud_blur()
			&& self.state.live_bg_monitor == Some(monitor)
			&& let Some(image) = self.state.live_bg_image.take()
		{
			self.state.live_bg_monitor = None;

			self.state.finish_freeze(monitor, image);

			self.pending_freeze_capture = None;
			self.pending_freeze_capture_armed = false;

			if let Some(cursor) = self.state.cursor {
				self.update_cursor_state(monitor, cursor);
			}
		} else {
			self.state.live_bg_monitor = None;
			self.state.live_bg_image = None;
			#[cfg(not(target_os = "macos"))]
			{
				self.capture_windows_hidden = true;

				self.hide_capture_windows();
			}
		}

		self.request_redraw_for_monitor(monitor);

		OverlayControl::Continue
	}

	fn toolbar_pointer_state(
		&self,
		monitor: MonitorRect,
		toolbar_cursor_local_override: Option<Pos2>,
	) -> Option<FrozenToolbarPointerState> {
		if !matches!(self.state.mode, OverlayMode::Frozen) {
			return None;
		}
		if !self.toolbar_state.visible {
			return None;
		}
		if self.state.monitor != Some(monitor) {
			return None;
		}
		if self.active_cursor_monitor() != Some(monitor) {
			return None;
		}

		let cursor_local = toolbar_cursor_local_override
			.or_else(|| self.state.cursor.and_then(|cursor| global_to_local(cursor, monitor)))?;
		let left_button_down = self.toolbar_left_button_down;
		let left_button_went_down = left_button_down && !self.toolbar_left_button_down_prev;
		let left_button_went_up = !left_button_down && self.toolbar_left_button_down_prev;

		Some(FrozenToolbarPointerState {
			cursor_local,
			left_button_down,
			left_button_went_down,
			left_button_went_up,
		})
	}

	fn handle_key_event(&mut self, event: &KeyEvent) -> OverlayControl {
		if event.state != ElementState::Pressed {
			return OverlayControl::Continue;
		}
		if event.repeat {
			return OverlayControl::Continue;
		}

		match &event.logical_key {
			Key::Named(NamedKey::Escape) => self.exit(OverlayExit::Cancelled),
			Key::Named(NamedKey::Tab) => {
				let Some(rgb) = self.state.rgb else {
					return OverlayControl::Continue;
				};
				let hex = rgb.hex_upper();

				match write_text_to_clipboard(&hex) {
					Ok(()) => {},
					Err(err) => {
						self.state.set_error(format!("{err:#}"));
						self.request_redraw_all();
					},
				}

				OverlayControl::Continue
			},
			Key::Character(key_text) if key_text == "h" || key_text == "H" => {
				self.toolbar_state.visible = !self.toolbar_state.visible;

				self.request_redraw_all();

				OverlayControl::Continue
			},
			Key::Named(NamedKey::Space) => {
				if matches!(self.state.mode, OverlayMode::Frozen)
					&& self.state.frozen_image.is_some()
				{
					self.state.set_error("Copying...");

					self.pending_encode_png = self.state.frozen_image.take();

					self.request_redraw_all();
				}

				OverlayControl::Continue
			},
			_ => OverlayControl::Continue,
		}
	}

	fn handle_redraw_requested(&mut self, window_id: WindowId) -> OverlayControl {
		if self.hud_window.as_ref().is_some_and(|hud_window| hud_window.window.id() == window_id) {
			return self.handle_hud_redraw_requested();
		}
		if self
			.loupe_window
			.as_ref()
			.is_some_and(|loupe_window| loupe_window.window.id() == window_id)
		{
			return self.handle_loupe_redraw_requested();
		}

		self.handle_overlay_window_redraw(window_id)
	}

	fn handle_hud_redraw_requested(&mut self) -> OverlayControl {
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};

		if self.capture_windows_hidden {
			#[cfg(not(target_os = "macos"))]
			if let Some(hud_window) = self.hud_window.as_ref() {
				hud_window.window.set_visible(false);
			}

			self.last_present_at = Instant::now();

			#[cfg(not(target_os = "macos"))]
			return OverlayControl::Continue;
		}

		let monitor =
			self.monitor_for_mode().or_else(|| self.windows.values().next().map(|w| w.monitor));
		let macos_hud_window_blur_enabled = self.macos_hud_window_blur_enabled();
		let mut request_toolbar_redraw = None;

		if let (Some(monitor), Some(hud_window)) = (monitor, self.hud_window.as_mut()) {
			#[cfg(target_os = "macos")]
			macos_configure_hud_window(
				hud_window.window.as_ref(),
				macos_hud_window_blur_enabled,
				self.config.hud_fog_amount,
				None,
			);

			#[cfg(not(target_os = "macos"))]
			hud_window.window.set_visible(true);

			if let Err(err) = hud_window.renderer.draw(
				gpu,
				&self.state,
				monitor,
				true,
				Some(Pos2::new(-14.0, -14.0)),
				true,
				HudAnchor::Cursor,
				self.config.show_alt_hint_keycap,
				self.config.show_hud_blur,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_fog_amount,
				self.config.hud_milk_amount,
				self.config.hud_tint_hue,
				self.config.theme_mode,
				true,
				None,
				None,
			) {
				return self.exit(OverlayExit::Error(format!("{err:#}")));
			}
			if let Some(hud_pill) = hud_window.renderer.hud_pill {
				let height_points = hud_pill.rect.height();
				let height_changed = self
					.toolbar_state
					.pill_height_points
					.is_none_or(|prev| (prev - height_points).abs() > 0.1);

				self.toolbar_state.pill_height_points = Some(height_points);

				// The toolbar uses the HUD pill height as its own height. The HUD window might be
				// redrawn after the overlay has already rendered the toolbar (especially right
				// after entering Frozen mode), which would otherwise leave the toolbar in a
				// partially-updated position until the next input event.
				if height_changed
					&& matches!(self.state.mode, OverlayMode::Frozen)
					&& self.toolbar_state.visible
					&& self.state.monitor == Some(monitor)
				{
					self.toolbar_state.needs_redraw = true;
					request_toolbar_redraw = Some(monitor);
				}

				let desired_w = hud_pill.rect.width().ceil().max(1.0) as u32;
				let desired_h = hud_pill.rect.height().ceil().max(1.0) as u32;
				let desired = (desired_w, desired_h);

				if self.hud_inner_size_points != Some(desired) {
					self.hud_inner_size_points = Some(desired);

					let _ = hud_window.window.request_inner_size(LogicalSize::new(
						f64::from(desired_w),
						f64::from(desired_h),
					));

					if let Some(cursor) = self.state.cursor {
						self.update_hud_window_position(monitor, cursor);
					}
				}
			}
		}
		if let Some(monitor) = request_toolbar_redraw {
			self.request_redraw_for_monitor(monitor);
		}

		self.last_present_at = Instant::now();

		OverlayControl::Continue
	}

	fn handle_loupe_redraw_requested(&mut self) -> OverlayControl {
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};

		if self.capture_windows_hidden {
			#[cfg(not(target_os = "macos"))]
			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(false);
			}

			self.last_present_at = Instant::now();

			#[cfg(not(target_os = "macos"))]
			return OverlayControl::Continue;
		}
		if !self.state.alt_held {
			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(false);
			}

			self.last_present_at = Instant::now();

			return OverlayControl::Continue;
		}

		let monitor =
			self.monitor_for_mode().or_else(|| self.windows.values().next().map(|w| w.monitor));
		let Some(monitor) = monitor else {
			self.last_present_at = Instant::now();

			return OverlayControl::Continue;
		};
		let Some(cursor) = self.state.cursor else {
			self.last_present_at = Instant::now();

			return OverlayControl::Continue;
		};
		let macos_hud_window_blur_enabled = self.macos_hud_window_blur_enabled();
		let mut needs_reposition = false;

		if let Some(loupe_window) = self.loupe_window.as_mut() {
			#[cfg(not(target_os = "macos"))]
			loupe_window.window.set_visible(true);

			#[cfg(target_os = "macos")]
			macos_configure_hud_window(
				loupe_window.window.as_ref(),
				macos_hud_window_blur_enabled,
				self.config.hud_fog_amount,
				Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
			);

			if let Err(err) = loupe_window.renderer.draw_loupe_tile_window(
				gpu,
				&self.state,
				monitor,
				cursor,
				self.config.show_hud_blur,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_fog_amount,
				self.config.hud_milk_amount,
				self.config.hud_tint_hue,
				self.config.theme_mode,
			) {
				return self.exit(OverlayExit::Error(format!("{err:#}")));
			}
			if let Some(tile_rect) = loupe_window.renderer.loupe_tile {
				let desired_w = tile_rect.max.x.ceil().max(1.0) as u32;
				let desired_h = tile_rect.max.y.ceil().max(1.0) as u32;
				let desired = (desired_w, desired_h);

				if self.loupe_inner_size_points != Some(desired) {
					self.loupe_inner_size_points = Some(desired);

					let _ = loupe_window.window.request_inner_size(LogicalSize::new(
						f64::from(desired_w),
						f64::from(desired_h),
					));

					needs_reposition = true;
				}
			}
		}

		if needs_reposition {
			let _ = self.update_loupe_window_position(monitor);
		}

		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(true);
		}

		self.last_present_at = Instant::now();

		OverlayControl::Continue
	}

	fn handle_overlay_window_redraw(&mut self, window_id: WindowId) -> OverlayControl {
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};
		let Some(overlay_monitor) = self.windows.get(&window_id).map(|overlay| overlay.monitor)
		else {
			return OverlayControl::Continue;
		};
		let toolbar_input = self.toolbar_pointer_state(overlay_monitor, None);
		// On macOS the frozen toolbar is now rendered in its own native HUD window; keep this
		// fullscreen overlay free of toolbar UI so shader-backed blur and monitor-aligned offsets
		// do not conflict with native-window positioning.
		let draw_toolbar = !cfg!(target_os = "macos")
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.toolbar_state.visible
			&& self.state.monitor == Some(overlay_monitor)
			&& self.state.frozen_image.is_some()
			&& self.pending_freeze_capture != Some(overlay_monitor);

		if matches!(self.state.mode, OverlayMode::Frozen)
			&& self.state.monitor == Some(overlay_monitor)
		{
			tracing::trace!(
				window_id = ?window_id,
				monitor_id = overlay_monitor.id,
				frozen_generation = self.state.frozen_generation,
				frozen_image_ready = self.state.frozen_image.is_some(),
				pending_freeze_capture = self.pending_freeze_capture.map(|m| m.id),
				draw_toolbar,
				toolbar_visible = self.toolbar_state.visible,
				toolbar_floating_position = ?self.toolbar_state.floating_position,
				toolbar_stable_frames = self.toolbar_state.layout_stable_frames,
				toolbar_last_screen_size_points = ?self.toolbar_state.layout_last_screen_size_points,
				"Overlay redraw (Frozen)."
			);
		}

		let toolbar_state = if draw_toolbar { Some(&mut self.toolbar_state) } else { None };

		{
			let Some(overlay_window) = self.windows.get_mut(&window_id) else {
				return OverlayControl::Continue;
			};

			if let Err(err) = overlay_window.renderer.draw(
				gpu,
				&self.state,
				overlay_monitor,
				false,
				None,
				false,
				self.config.hud_anchor,
				self.config.show_alt_hint_keycap,
				self.config.show_hud_blur,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_fog_amount,
				self.config.hud_milk_amount,
				self.config.hud_tint_hue,
				self.config.theme_mode,
				true,
				toolbar_state,
				toolbar_input,
			) {
				return self.exit(OverlayExit::Error(format!("{err:#}")));
			}
		}
		self.last_present_at = Instant::now();
		self.toolbar_left_button_down_prev = self.toolbar_left_button_down;

		if self.pending_freeze_capture == Some(overlay_monitor)
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.state.monitor == Some(overlay_monitor)
			&& self.state.frozen_image.is_none()
			&& let Some(worker) = &self.worker
		{
			#[cfg(target_os = "macos")]
			{
				if worker.request_freeze_capture(overlay_monitor) {
					self.pending_freeze_capture = None;
					self.pending_freeze_capture_armed = false;
				} else {
					self.request_redraw_for_monitor(overlay_monitor);
				}
			}
			#[cfg(not(target_os = "macos"))]
			{
				// Capture must happen on a post-hide redraw so the HUD/loupe are not included.
				if self.pending_freeze_capture_armed {
					if worker.request_freeze_capture(overlay_monitor) {
						self.pending_freeze_capture = None;
						self.pending_freeze_capture_armed = false;
					} else {
						self.request_redraw_for_monitor(overlay_monitor);
					}
				} else {
					self.pending_freeze_capture_armed = true;

					self.hide_capture_windows();
					self.request_redraw_for_monitor(overlay_monitor);
				}
			}
		}
		if draw_toolbar && self.toolbar_state.needs_redraw {
			self.toolbar_state.needs_redraw = false;

			self.request_redraw_for_monitor(overlay_monitor);
		}

		OverlayControl::Continue
	}

	fn exit(&mut self, exit: OverlayExit) -> OverlayControl {
		self.windows.clear();

		self.hud_window = None;
		self.hud_inner_size_points = None;
		self.hud_outer_pos = None;
		self.loupe_window = None;
		self.loupe_inner_size_points = None;
		self.loupe_outer_pos = None;
		self.toolbar_window = None;
		self.toolbar_inner_size_points = None;
		self.toolbar_outer_pos = None;
		self.toolbar_window_visible = false;
		self.toolbar_window_warmup_redraws_remaining = 0;
		self.cursor_monitor = None;
		self.gpu = None;
		self.worker = None;
		self.toolbar_left_button_down = false;
		self.toolbar_left_button_down_prev = false;
		self.toolbar_pointer_local = None;

		OverlayControl::Exit(exit)
	}

	fn initialize_cursor_state(&mut self) {
		let mouse = self.cursor_device.get_mouse();
		let cursor = GlobalPoint::new(mouse.coords.0, mouse.coords.1);
		let Some(monitor) = self.monitor_at(cursor) else {
			self.state.cursor = Some(cursor);
			self.state.rgb = None;
			self.cursor_monitor = None;

			return;
		};

		self.update_cursor_state(monitor, cursor);
		self.update_hud_window_position(monitor, cursor);

		if matches!(self.state.mode, OverlayMode::Live) {
			if self.use_fake_hud_blur() {
				self.maybe_request_live_bg(monitor);
			}

			if let Some(worker) = &self.worker {
				let request_id = self.rgb_send_sequence + 1;
				let now = Instant::now();

				self.rgb_send_sequence = request_id;

				match worker.try_sample_rgb(monitor, cursor) {
					Ok(()) => {
						self.pending_rgb_request_id = Some(request_id);
						self.pending_rgb_request_at = Some(now);

						if tracing::enabled!(tracing::Level::TRACE) {
							tracing::trace!(
								request_id,
								monitor_id = monitor.id,
								point = ?cursor,
								"RGB sample request sent."
							);
						}
					},
					Err(WorkerRequestSendError::Full) => {
						self.rgb_send_full_count = self.rgb_send_full_count.saturating_add(1);

						tracing::debug!(
							request_id,
							monitor_id = monitor.id,
							point = ?cursor,
							full_count = self.rgb_send_full_count,
							"RGB sample request dropped: worker queue full."
						);
					},
					Err(WorkerRequestSendError::Disconnected) => {
						self.rgb_send_disconnected_count =
							self.rgb_send_disconnected_count.saturating_add(1);

						tracing::debug!(
							request_id,
							monitor_id = monitor.id,
							point = ?cursor,
							disconnected_count = self.rgb_send_disconnected_count,
							"RGB sample request dropped: worker disconnected."
						);
					},
				}

				self.last_rgb_request_at = Instant::now();
			}
		}
	}

	fn maybe_request_live_bg(&mut self, monitor: MonitorRect) {
		if !matches!(self.state.mode, OverlayMode::Live) || !self.use_fake_hud_blur() {
			return;
		}
		if self.state.live_bg_monitor == Some(monitor) && self.state.live_bg_image.is_some() {
			return;
		}

		let force = self.state.alt_held && self.state.live_bg_image.is_none();

		if !force && self.last_live_bg_request_at.elapsed() < self.live_bg_request_interval {
			return;
		}

		let Some(worker) = &self.worker else {
			return;
		};

		if worker.request_freeze_capture(monitor) {
			self.last_live_bg_request_at = Instant::now();
		}
	}

	fn monitor_at(&self, cursor: GlobalPoint) -> Option<MonitorRect> {
		self.windows
			.values()
			.find(|window| window.monitor.contains(cursor))
			.map(|window| window.monitor)
	}

	fn resolve_device_cursor_point(
		&self,
		raw: GlobalPoint,
	) -> Option<(MonitorRect, GlobalPoint, DeviceCursorPointSource)> {
		if let Some(monitor) = self.monitor_at(raw) {
			return Some((monitor, raw, DeviceCursorPointSource::DevicePoints));
		}

		for monitor in self.windows.values().map(|window| window.monitor) {
			let sf = f64::from(monitor.scale_factor()).max(1.0);
			let origin_px_x = (monitor.origin.x as f64 * sf).round() as i64;
			let origin_px_y = (monitor.origin.y as f64 * sf).round() as i64;
			let size_px_x = (monitor.width as f64 * sf).round() as i64;
			let size_px_y = (monitor.height as f64 * sf).round() as i64;
			let local_px_x = (raw.x as i64).saturating_sub(origin_px_x);
			let local_px_y = (raw.y as i64).saturating_sub(origin_px_y);

			if local_px_x < 0
				|| local_px_y < 0
				|| local_px_x >= size_px_x
				|| local_px_y >= size_px_y
			{
				continue;
			}

			let local_points_x = (local_px_x as f64 / sf).round() as i64;
			let local_points_y = (local_px_y as f64 / sf).round() as i64;
			let local_points_x = match i32::try_from(local_points_x) {
				Ok(value) => value,
				Err(_) => continue,
			};
			let local_points_y = match i32::try_from(local_points_y) {
				Ok(value) => value,
				Err(_) => continue,
			};
			let candidate = GlobalPoint::new(
				monitor.origin.x.saturating_add(local_points_x),
				monitor.origin.y.saturating_add(local_points_y),
			);

			if monitor.contains(candidate) {
				return Some((monitor, candidate, DeviceCursorPointSource::DevicePixelsFallback));
			}
		}

		None
	}

	fn resolve_live_cursor_point(
		&self,
		raw_device: GlobalPoint,
	) -> Option<(MonitorRect, GlobalPoint, DeviceCursorPointSource)> {
		let Some((device_monitor, device_global, device_source)) =
			self.resolve_device_cursor_point(raw_device)
		else {
			let Some((monitor, global)) = self.last_event_cursor else {
				return None;
			};
			let Some(event_cursor_at) = self.last_event_cursor_at else {
				return None;
			};

			if event_cursor_at.elapsed() > LIVE_EVENT_CURSOR_CACHE_TTL {
				return None;
			}

			return Some((monitor, global, DeviceCursorPointSource::EventRecentFallback));
		};

		if let (Some(event_cursor_at), Some((event_monitor, event_global))) =
			(self.last_event_cursor_at, self.last_event_cursor)
			&& self.state.cursor == Some(device_global)
			&& event_global != device_global
			&& event_cursor_at.elapsed() <= LIVE_EVENT_CURSOR_CACHE_TTL
		{
			return Some((
				event_monitor,
				event_global,
				DeviceCursorPointSource::EventRecentFallback,
			));
		}

		Some((device_monitor, device_global, device_source))
	}

	fn active_cursor_monitor(&self) -> Option<MonitorRect> {
		self.cursor_monitor.or_else(|| self.state.cursor.and_then(|cursor| self.monitor_at(cursor)))
	}

	fn monitor_for_mode(&self) -> Option<MonitorRect> {
		match self.state.mode {
			OverlayMode::Frozen => self.active_cursor_monitor().or(self.state.monitor),
			OverlayMode::Live => self.active_cursor_monitor(),
		}
	}

	fn update_hud_window_position(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		let Some(hud_window) = self.hud_window.as_ref() else {
			return;
		};
		let scale = hud_window.window.scale_factor().max(1.0);
		let size = hud_window.window.inner_size();
		let hud_w_points = ((size.width as f64) / scale).ceil().max(1.0) as i32;
		let hud_h_points = ((size.height as f64) / scale).ceil().max(1.0) as i32;
		let monitor_right = monitor.origin.x.saturating_add_unsigned(monitor.width);
		let monitor_bottom = monitor.origin.y.saturating_add_unsigned(monitor.height);
		// Keep the HUD far enough from the cursor that even if the OS lags window moves during
		// rapid drags, the cursor is unlikely to "catch up" and overlap the HUD window.
		let offset_x = 48;
		let offset_y = 24;
		let mut x = cursor.x.saturating_add(offset_x);
		let mut y = cursor.y.saturating_add(offset_y);

		if x.saturating_add(hud_w_points) > monitor_right {
			x = cursor.x.saturating_sub(offset_x.saturating_add(hud_w_points));
		}
		if y.saturating_add(hud_h_points) > monitor_bottom {
			y = cursor.y.saturating_sub(offset_y.saturating_add(hud_h_points));
		}

		x = x.clamp(
			monitor.origin.x,
			monitor_right.saturating_sub(hud_w_points).max(monitor.origin.x),
		);
		y = y.clamp(
			monitor.origin.y,
			monitor_bottom.saturating_sub(hud_h_points).max(monitor.origin.y),
		);

		let desired = GlobalPoint::new(x, y);

		if self.hud_outer_pos == Some(desired) {
			return;
		}

		self.hud_outer_pos = Some(desired);

		hud_window.window.set_outer_position(LogicalPosition::new(x as f64, y as f64));
		hud_window.window.request_redraw();

		if self.state.alt_held {
			let visible = self.update_loupe_window_position(monitor);

			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(visible);
				loupe_window.window.request_redraw();
			}
		}
	}

	fn update_loupe_window_position(&mut self, monitor: MonitorRect) -> bool {
		if !self.state.alt_held {
			return false;
		}

		let Some(loupe_window) = self.loupe_window.as_ref() else {
			return false;
		};
		let Some(hud_window) = self.hud_window.as_ref() else {
			return false;
		};
		let Some(hud_outer) = self.hud_outer_pos else {
			return false;
		};
		let hud_scale = hud_window.window.scale_factor().max(1.0);
		let hud_size = hud_window.window.inner_size();
		let hud_h_points = ((hud_size.height as f64) / hud_scale).ceil().max(1.0) as i32;
		let loupe_scale = loupe_window.window.scale_factor().max(1.0);
		let loupe_size = loupe_window.window.inner_size();
		let loupe_w_points = ((loupe_size.width as f64) / loupe_scale).ceil().max(1.0) as i32;
		let loupe_h_points = ((loupe_size.height as f64) / loupe_scale).ceil().max(1.0) as i32;
		let gap = 10;
		let monitor_right = monitor.origin.x.saturating_add_unsigned(monitor.width);
		let monitor_bottom = monitor.origin.y.saturating_add_unsigned(monitor.height);
		let below_y = hud_outer.y.saturating_add(hud_h_points + gap);
		let above_y = hud_outer.y.saturating_sub(gap.saturating_add(loupe_h_points));
		let max_x = monitor_right.saturating_sub(loupe_w_points).max(monitor.origin.x);
		let max_y = monitor_bottom.saturating_sub(loupe_h_points).max(monitor.origin.y);
		let mut x = hud_outer.x;
		let mut y = if below_y.saturating_add(loupe_h_points) <= monitor_bottom {
			below_y
		} else {
			above_y
		};

		x = x.clamp(monitor.origin.x, max_x);
		y = y.clamp(monitor.origin.y, max_y);

		let desired = GlobalPoint::new(x, y);

		if self.loupe_outer_pos == Some(desired) {
			return true;
		}

		self.loupe_outer_pos = Some(desired);

		loupe_window.window.set_outer_position(LogicalPosition::new(x as f64, y as f64));
		loupe_window.window.request_redraw();

		true
	}

	fn update_toolbar_outer_position(&mut self, monitor: MonitorRect, local_pos: Pos2) -> bool {
		let Some(toolbar_window) = self.toolbar_window.as_ref() else {
			return false;
		};
		let toolbar_scale = toolbar_window.window.scale_factor().max(1.0);
		let toolbar_size = if let Some((width, height)) = self.toolbar_inner_size_points {
			Vec2::new(width as f32, height as f32)
		} else {
			let size = toolbar_window.window.inner_size();
			let toolbar_w = ((size.width as f64) / toolbar_scale).ceil().max(1.0) as f32;
			let toolbar_h = ((size.height as f64) / toolbar_scale).ceil().max(1.0) as f32;

			Vec2::new(toolbar_w, toolbar_h)
		};
		let screen_rect =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
		let clamped_local_pos = WindowRenderer::clamp_toolbar_position(
			screen_rect,
			toolbar_size,
			local_pos,
			TOOLBAR_SCREEN_MARGIN_PX,
			TOOLBAR_SCREEN_MARGIN_PX,
		);
		let desired = GlobalPoint::new(
			monitor.origin.x.saturating_add(clamped_local_pos.x.round() as i32),
			monitor.origin.y.saturating_add(clamped_local_pos.y.round() as i32),
		);

		if self.toolbar_outer_pos == Some(desired) {
			return false;
		}

		self.toolbar_outer_pos = Some(desired);
		self.toolbar_state.floating_position = Some(clamped_local_pos);

		toolbar_window
			.window
			.set_outer_position(LogicalPosition::new(desired.x as f64, desired.y as f64));
		toolbar_window.window.request_redraw();

		true
	}

	fn update_cursor_state(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		self.cursor_monitor = Some(monitor);
		self.state.cursor = Some(cursor);

		match self.state.mode {
			OverlayMode::Live => {},
			OverlayMode::Frozen => {
				if self.state.frozen_image.is_none() {
					return;
				}

				let frozen_monitor = self.state.monitor;

				self.state.rgb = frozen_rgb(&self.state.frozen_image, frozen_monitor, cursor);
				self.state.loupe = if self.state.alt_held {
					frozen_loupe_patch(
						&self.state.frozen_image,
						frozen_monitor,
						cursor,
						self.loupe_patch_width_px,
						self.loupe_patch_height_px,
					)
					.map(|patch| crate::state::LoupeSample { center: cursor, patch })
				} else {
					None
				};
			},
		}
	}

	#[cfg(not(target_os = "macos"))]
	fn hide_capture_windows(&mut self) {
		self.capture_windows_hidden = true;

		if let Some(hud_window) = &self.hud_window {
			hud_window.window.set_visible(false);
		}
		if let Some(loupe_window) = &self.loupe_window {
			loupe_window.window.set_visible(false);
		}
	}

	fn restore_capture_windows_visibility(&mut self) {
		if !self.capture_windows_hidden {
			return;
		}

		self.capture_windows_hidden = false;

		#[cfg(not(target_os = "macos"))]
		if let Some(hud_window) = &self.hud_window {
			hud_window.window.set_visible(true);
		}
		#[cfg(not(target_os = "macos"))]
		if let Some(loupe_window) = &self.loupe_window {
			loupe_window.window.set_visible(self.state.alt_held);
		}
	}

	#[cfg(target_os = "macos")]
	fn raise_hud_windows(&self) {}

	#[cfg(not(target_os = "macos"))]
	fn raise_hud_windows(&self) {
		if let Some(hud_window) = self.hud_window.as_ref() {
			hud_window.window.focus_window();
		}

		if self.state.alt_held
			&& let Some(loupe_window) = self.loupe_window.as_ref()
		{
			loupe_window.window.focus_window();
		}
	}
}

impl Default for OverlaySession {
	fn default() -> Self {
		Self::new()
	}
}

#[derive(Clone, Copy, Debug)]
struct HudDrawConfig {
	can_draw_hud: bool,
	needs_frozen_surface_bg: bool,
	needs_shader_blur_bg: bool,
	hud_glass_active: bool,
}

#[derive(Debug)]
struct FrozenToolbarState {
	visible: bool,
	dragging: bool,
	selected_tool: FrozenToolbarTool,
	needs_redraw: bool,
	pill_height_points: Option<f32>,
	floating_position: Option<Pos2>,
	layout_last_screen_size_points: Option<Vec2>,
	layout_stable_frames: u8,
	drag_offset: Vec2,
	drag_anchor: Option<Pos2>,
}
impl Default for FrozenToolbarState {
	fn default() -> Self {
		Self {
			visible: true,
			dragging: false,
			selected_tool: FrozenToolbarTool::Pointer,
			needs_redraw: false,
			pill_height_points: None,
			floating_position: None,
			layout_last_screen_size_points: None,
			layout_stable_frames: 0,
			drag_offset: Vec2::ZERO,
			drag_anchor: None,
		}
	}
}

#[derive(Clone, Copy, Debug)]
struct FrozenToolbarPointerState {
	cursor_local: Pos2,
	left_button_down: bool,
	left_button_went_down: bool,
	left_button_went_up: bool,
}

struct HudOverlayWindow {
	window: Arc<winit::window::Window>,
	renderer: WindowRenderer,
}

struct OverlayWindow {
	monitor: MonitorRect,
	window: Arc<winit::window::Window>,
	renderer: WindowRenderer,
}

struct GpuContext {
	instance: wgpu::Instance,
	adapter: Adapter,
	device: Device,
	queue: Queue,
}
impl GpuContext {
	fn new() -> Result<Self> {
		let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
		let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::LowPower,
			compatible_surface: None,
			force_fallback_adapter: false,
		}))
		.map_err(|err| eyre::eyre!("Failed to request GPU adapter: {err}"))?;
		let adapter_limits = adapter.limits();
		let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
			label: Some("rsnap-overlay device"),
			required_features: wgpu::Features::empty(),
			// Use the adapter's actual limits. Using `downlevel_defaults()` caps max texture
			// size to 2048, which breaks on common HiDPI displays.
			required_limits: adapter_limits,
			experimental_features: wgpu::ExperimentalFeatures::default(),
			memory_hints: wgpu::MemoryHints::Performance,
			trace: wgpu::Trace::Off,
		}))
		.wrap_err("Failed to create wgpu device")?;

		Ok(Self { instance, adapter, device, queue })
	}
}

struct WindowRenderer {
	window: Arc<winit::window::Window>,
	surface: Surface<'static>,
	surface_config: wgpu::SurfaceConfiguration,
	needs_reconfigure: bool,
	egui_ctx: egui::Context,
	egui_renderer: Renderer,
	bg_sampler: wgpu::Sampler,
	mipgen_pipeline: RenderPipeline,
	mipgen_surface_pipeline: RenderPipeline,
	mipgen_bind_group_layout: BindGroupLayout,
	hud_blur_pipeline: RenderPipeline,
	hud_blur_bind_group_layout: BindGroupLayout,
	hud_blur_uniform: wgpu::Buffer,
	hud_bg: Option<HudBg>,
	hud_bg_generation: u64,
	hud_pill: Option<HudPillGeometry>,
	loupe_tile: Option<Rect>,
	hud_theme: Option<HudTheme>,
	egui_start_time: Instant,
	egui_last_frame_time: Instant,
}
impl WindowRenderer {
	fn mip_level_count(width: u32, height: u32) -> u32 {
		let max_dim = width.max(height).max(1);

		(32_u32.saturating_sub(max_dim.leading_zeros())).max(1)
	}

	fn create_mipgen_pipeline(
		gpu: &GpuContext,
		format: wgpu::TextureFormat,
	) -> (RenderPipeline, BindGroupLayout) {
		let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("rsnap-mipgen shader"),
			source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
				"mipgen.wgsl"
			))),
		});
		let bind_group_layout =
			gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("rsnap-mipgen bgl"),
				entries: &[
					wgpu::BindGroupLayoutEntry {
						binding: 0,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Texture {
							multisampled: false,
							view_dimension: wgpu::TextureViewDimension::D2,
							sample_type: wgpu::TextureSampleType::Float { filterable: true },
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 1,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
						count: None,
					},
				],
			});
		let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("rsnap-mipgen pipeline layout"),
			bind_group_layouts: &[&bind_group_layout],
			push_constant_ranges: &[],
		});
		let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rsnap-mipgen pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				buffers: &[],
			},
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleList,
				strip_index_format: None,
				front_face: wgpu::FrontFace::Ccw,
				cull_mode: None,
				polygon_mode: wgpu::PolygonMode::Fill,
				unclipped_depth: false,
				conservative: false,
			},
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format,
					blend: None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			multiview: None,
			cache: None,
		});

		(pipeline, bind_group_layout)
	}

	fn create_mipgen_surface_pipeline(
		gpu: &GpuContext,
		format: wgpu::TextureFormat,
		bind_group_layout: &BindGroupLayout,
	) -> RenderPipeline {
		let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("rsnap-mipgen fullscreen shader"),
			source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
				"mipgen.wgsl"
			))),
		});
		let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("rsnap-mipgen fullscreen pipeline layout"),
			bind_group_layouts: &[bind_group_layout],
			push_constant_ranges: &[],
		});

		gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rsnap-mipgen fullscreen pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				buffers: &[],
			},
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleList,
				strip_index_format: None,
				front_face: wgpu::FrontFace::Ccw,
				cull_mode: None,
				polygon_mode: wgpu::PolygonMode::Fill,
				unclipped_depth: false,
				conservative: false,
			},
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format,
					blend: None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			multiview: None,
			cache: None,
		})
	}

	fn generate_mipmaps(&self, gpu: &GpuContext, texture: &wgpu::Texture, mip_level_count: u32) {
		if mip_level_count <= 1 {
			return;
		}

		let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
			label: Some("rsnap-mipgen encoder"),
		});

		for level in 1..mip_level_count {
			let src_view = texture.create_view(&wgpu::TextureViewDescriptor {
				label: Some("rsnap-mipgen src view"),
				format: None,
				dimension: None,
				usage: None,
				aspect: wgpu::TextureAspect::All,
				base_mip_level: level - 1,
				mip_level_count: Some(1),
				base_array_layer: 0,
				array_layer_count: Some(1),
			});
			let dst_view = texture.create_view(&wgpu::TextureViewDescriptor {
				label: Some("rsnap-mipgen dst view"),
				format: None,
				dimension: None,
				usage: None,
				aspect: wgpu::TextureAspect::All,
				base_mip_level: level,
				mip_level_count: Some(1),
				base_array_layer: 0,
				array_layer_count: Some(1),
			});
			let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
				label: Some("rsnap-mipgen bind group"),
				layout: &self.mipgen_bind_group_layout,
				entries: &[
					wgpu::BindGroupEntry {
						binding: 0,
						resource: wgpu::BindingResource::TextureView(&src_view),
					},
					wgpu::BindGroupEntry {
						binding: 1,
						resource: wgpu::BindingResource::Sampler(&self.bg_sampler),
					},
				],
			});
			let rpass_desc = wgpu::RenderPassDescriptor {
				label: Some("rsnap-mipgen pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &dst_view,
					depth_slice: None,
					resolve_target: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
			};
			let mut rpass = encoder.begin_render_pass(&rpass_desc).forget_lifetime();

			rpass.set_pipeline(&self.mipgen_pipeline);
			rpass.set_bind_group(0, &bind_group, &[]);
			rpass.draw(0..3, 0..1);
		}

		gpu.queue.submit(Some(encoder.finish()));
	}
	fn pick_surface_format(caps: &SurfaceCapabilities) -> wgpu::TextureFormat {
		caps.formats
			.iter()
			.copied()
			.find(|f| {
				matches!(
					f,
					wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Rgba8UnormSrgb
				)
			})
			.or_else(|| caps.formats.iter().copied().find(wgpu::TextureFormat::is_srgb))
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

	fn make_surface_config(
		window: &winit::window::Window,
		format: wgpu::TextureFormat,
		alpha_mode: CompositeAlphaMode,
	) -> wgpu::SurfaceConfiguration {
		let size = window.inner_size();

		wgpu::SurfaceConfiguration {
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			format,
			width: size.width.max(1),
			height: size.height.max(1),
			present_mode: wgpu::PresentMode::Fifo,
			alpha_mode,
			view_formats: vec![],
			desired_maximum_frame_latency: 2,
		}
	}

	fn create_bg_sampler(gpu: &GpuContext) -> wgpu::Sampler {
		gpu.device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("rsnap-frozen-bg sampler"),
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			address_mode_w: wgpu::AddressMode::ClampToEdge,
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			mipmap_filter: wgpu::FilterMode::Linear,
			..Default::default()
		})
	}

	fn create_hud_blur_pipeline(
		gpu: &GpuContext,
		surface_format: wgpu::TextureFormat,
	) -> (RenderPipeline, BindGroupLayout) {
		let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("rsnap-hud-blur shader"),
			source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
				"hud_blur.wgsl"
			))),
		});
		let bind_group_layout =
			gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("rsnap-hud-blur bgl"),
				entries: &[
					wgpu::BindGroupLayoutEntry {
						binding: 0,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Texture {
							multisampled: false,
							view_dimension: wgpu::TextureViewDimension::D2,
							sample_type: wgpu::TextureSampleType::Float { filterable: true },
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 1,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 2,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Uniform,
							has_dynamic_offset: false,
							min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<
								HudBlurUniformRaw,
							>() as u64),
						},
						count: None,
					},
				],
			});
		let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("rsnap-hud-blur pipeline layout"),
			bind_group_layouts: &[&bind_group_layout],
			push_constant_ranges: &[],
		});
		let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rsnap-hud-blur pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				buffers: &[],
			},
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleList,
				strip_index_format: None,
				front_face: wgpu::FrontFace::Ccw,
				cull_mode: None,
				polygon_mode: wgpu::PolygonMode::Fill,
				unclipped_depth: false,
				conservative: false,
			},
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format: surface_format,
					blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			multiview: None,
			cache: None,
		});

		(pipeline, bind_group_layout)
	}

	fn apply_pending_reconfigure(&mut self, gpu: &GpuContext) {
		if self.needs_reconfigure {
			self.reconfigure(gpu);

			self.needs_reconfigure = false;
		}
	}

	fn prepare_egui_input(
		&mut self,
		gpu: &GpuContext,
		pointer_state: Option<FrozenToolbarPointerState>,
		pixels_per_point_override: Option<f32>,
	) -> (PhysicalSize<u32>, f32, egui::RawInput) {
		// egui animations depend on a monotonic time base. Without this, animation state can appear
		// to "snap" only after an input event (e.g. CursorMoved) triggers a new frame.
		let now = Instant::now();
		let elapsed = now.duration_since(self.egui_start_time).as_secs_f64().max(0.0);
		let predicted_dt =
			now.duration_since(self.egui_last_frame_time).as_secs_f32().clamp(0.0, 0.5);

		self.egui_last_frame_time = now;

		// Keep the wgpu surface configuration in sync with the OS-reported window size.
		//
		// On macOS we can observe transient mismatches where `surface_config` is smaller than the
		// actual window size (e.g. right after entering Frozen mode), which causes egui to build
		// a smaller `screen_rect` and results in UI elements appearing clipped/offset until a
		// later redraw or input event triggers a resize/reconfigure.
		let actual_size = self.window.inner_size();
		let desired_w = actual_size.width.max(1);
		let desired_h = actual_size.height.max(1);

		if self.surface_config.width != desired_w || self.surface_config.height != desired_h {
			tracing::debug!(
				window_id = ?self.window.id(),
				actual_size_px = ?actual_size,
				old_surface_px = ?(self.surface_config.width, self.surface_config.height),
				new_surface_px = ?(desired_w, desired_h),
				window_scale_factor = self.window.scale_factor(),
				pixels_per_point_override,
				"Reconfiguring wgpu surface to match window."
			);

			self.surface_config.width = desired_w;
			self.surface_config.height = desired_h;
			self.needs_reconfigure = false;

			self.reconfigure(gpu);
		}

		let size = PhysicalSize::new(self.surface_config.width, self.surface_config.height);
		let pixels_per_point = pixels_per_point_override
			.filter(|v| *v > 0.0)
			.unwrap_or_else(|| self.window.scale_factor() as f32);
		let screen_size_points =
			Vec2::new(size.width as f32 / pixels_per_point, size.height as f32 / pixels_per_point);
		let max_texture_side = gpu.device.limits().max_texture_dimension_2d as usize;

		self.egui_ctx.input_mut(|i| i.max_texture_side = max_texture_side);

		let mut raw_input = egui::RawInput {
			screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen_size_points)),
			focused: true,
			time: Some(elapsed),
			predicted_dt,
			..Default::default()
		};
		let mut events = Vec::new();

		raw_input.max_texture_side = Some(max_texture_side);

		if let Some(pointer) = pointer_state {
			events.push(Event::PointerMoved(pointer.cursor_local));

			if pointer.left_button_went_down {
				events.push(Event::PointerButton {
					pos: pointer.cursor_local,
					button: PointerButton::Primary,
					pressed: true,
					modifiers: egui::Modifiers::default(),
				});
			}
			if pointer.left_button_went_up {
				events.push(Event::PointerButton {
					pos: pointer.cursor_local,
					button: PointerButton::Primary,
					pressed: false,
					modifiers: egui::Modifiers::default(),
				});
			}
		}

		if !events.is_empty() {
			raw_input.events = events;
		}

		if let Some(viewport) = raw_input.viewports.get_mut(&ViewportId::ROOT) {
			viewport.native_pixels_per_point = Some(pixels_per_point);
			viewport.inner_rect = raw_input.screen_rect;
			viewport.focused = Some(true);
		}

		(size, pixels_per_point, raw_input)
	}

	#[allow(clippy::too_many_arguments)]
	fn run_egui(
		&mut self,
		raw_input: egui::RawInput,
		state: &OverlayState,
		monitor: MonitorRect,
		can_draw_hud: bool,
		hud_local_cursor_override: Option<Pos2>,
		hud_compact: bool,
		show_hud_blur: bool,
		hud_anchor: HudAnchor,
		show_alt_hint_keycap: bool,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		theme: HudTheme,
		mut toolbar_state: Option<&mut FrozenToolbarState>,
		toolbar_pointer: Option<FrozenToolbarPointerState>,
	) -> (FullOutput, Option<HudPillGeometry>) {
		let hud_data = if can_draw_hud {
			state.cursor.and_then(|cursor| {
				let local_cursor =
					hud_local_cursor_override.or_else(|| global_to_local(cursor, monitor))?;

				Some((cursor, local_cursor))
			})
		} else {
			None
		};
		let mut hud_pill = None;
		let full_output = self.egui_ctx.run(raw_input, |ctx| {
			Self::render_frozen_toolbar_ui(
				ctx,
				state,
				monitor,
				theme,
				hud_blur_active,
				hud_opaque,
				hud_opacity,
				hud_milk_amount,
				hud_tint_hue,
				toolbar_state.as_deref_mut(),
				toolbar_pointer,
				&mut hud_pill,
			);

			if let Some((cursor, local_cursor)) = hud_data {
				let _ = show_hud_blur;

				Self::render_hud(
					ctx,
					state,
					monitor,
					cursor,
					local_cursor,
					hud_compact,
					hud_anchor,
					show_alt_hint_keycap,
					hud_blur_active,
					hud_opaque,
					hud_opacity,
					hud_milk_amount,
					hud_tint_hue,
					theme,
					&mut hud_pill,
				);
			}
		});

		(full_output, hud_pill)
	}

	#[allow(clippy::too_many_arguments)]
	fn render_frozen_toolbar_ui(
		ctx: &egui::Context,
		state: &OverlayState,
		monitor: MonitorRect,
		theme: HudTheme,
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

		if !matches!(state.mode, OverlayMode::Frozen) || !toolbar_state.visible {
			return;
		}
		if state.monitor != Some(monitor) {
			return;
		}

		let (cursor, left_button_down) = if let Some(pointer_state) = pointer_state {
			(pointer_state.cursor_local, pointer_state.left_button_down)
		} else {
			toolbar_state.dragging = false;

			(Pos2::new(-1.0, -1.0), false)
		};
		let toolbar_size = Vec2::new(
			TOOLBAR_EXPANDED_WIDTH_PX,
			toolbar_state.pill_height_points.unwrap_or(TOOLBAR_EXPANDED_HEIGHT_PX),
		);
		let screen_rect = ctx.input(|i| i.viewport_rect());
		let capture_rect = Self::frozen_toolbar_capture_rect(screen_rect);
		let Some(toolbar_pos) = Self::resolve_frozen_toolbar_birth(
			ctx,
			state,
			monitor,
			toolbar_state,
			screen_rect,
			capture_rect,
			toolbar_size,
		) else {
			return;
		};

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
			cursor,
			left_button_down,
			hud_pill_out,
		);
	}

	fn resolve_frozen_toolbar_birth(
		ctx: &egui::Context,
		state: &OverlayState,
		monitor: MonitorRect,
		toolbar_state: &mut FrozenToolbarState,
		screen_rect: Rect,
		capture_rect: Rect,
		toolbar_size: Vec2,
	) -> Option<Pos2> {
		if let Some(pos) = toolbar_state.floating_position {
			return Some(pos);
		}

		let screen_size_points = screen_rect.size();

		tracing::trace!(
			monitor_id = monitor.id,
			frozen_generation = state.frozen_generation,
			screen_rect = ?screen_rect,
			screen_size_points = ?screen_size_points,
			pixels_per_point = ctx.pixels_per_point(),
			last_screen_size_points = ?toolbar_state.layout_last_screen_size_points,
			stable_frames = toolbar_state.layout_stable_frames,
			"Frozen toolbar birth attempt."
		);

		let needs_new_sample = match toolbar_state.layout_last_screen_size_points {
			None => true,
			Some(last) => {
				let dx = (last.x - screen_size_points.x).abs();
				let dy = (last.y - screen_size_points.y).abs();

				dx > 0.5 || dy > 0.5
			},
		};

		if needs_new_sample {
			toolbar_state.layout_last_screen_size_points = Some(screen_size_points);
			toolbar_state.layout_stable_frames = 0;
			toolbar_state.needs_redraw = true;

			tracing::debug!(
				monitor_id = monitor.id,
				frozen_generation = state.frozen_generation,
				new_screen_size_points = ?screen_size_points,
				"Frozen toolbar waiting for stable screen rect (new sample)."
			);

			ctx.request_repaint();

			return None;
		}
		if toolbar_state.layout_stable_frames < 1 {
			toolbar_state.layout_stable_frames =
				toolbar_state.layout_stable_frames.saturating_add(1);
			toolbar_state.needs_redraw = true;

			tracing::debug!(
				monitor_id = monitor.id,
				frozen_generation = state.frozen_generation,
				screen_size_points = ?screen_size_points,
				stable_frames = toolbar_state.layout_stable_frames,
				"Frozen toolbar waiting for stable screen rect (confirm)."
			);

			ctx.request_repaint();

			return None;
		}

		let default_pos = Self::frozen_toolbar_default_pos(screen_rect, capture_rect, toolbar_size);

		tracing::debug!(
			monitor_id = monitor.id,
			frozen_generation = state.frozen_generation,
			toolbar_size_points = ?toolbar_size,
			default_pos = ?default_pos,
			"Frozen toolbar birth resolved."
		);

		toolbar_state.floating_position = Some(default_pos);

		Some(default_pos)
	}

	fn frozen_toolbar_capture_rect(screen_rect: Rect) -> Rect {
		screen_rect
	}

	fn frozen_toolbar_default_pos(
		screen_rect: Rect,
		capture_rect: Rect,
		toolbar_size: Vec2,
	) -> Pos2 {
		let below_y = capture_rect.max.y + TOOLBAR_CAPTURE_GAP_PX;
		let within_screen =
			below_y + toolbar_size.y + TOOLBAR_SCREEN_MARGIN_PX <= screen_rect.max.y;
		let y = if within_screen {
			below_y
		} else {
			capture_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - toolbar_size.y
		};
		let min_x = screen_rect.min.x + TOOLBAR_SCREEN_MARGIN_PX;
		let min_y = screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;
		let max_x = (screen_rect.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX).max(min_x);
		let max_y = (screen_rect.max.y - toolbar_size.y - TOOLBAR_SCREEN_MARGIN_PX).max(min_y);
		let x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(min_x, max_x);
		let y = y.max(min_y).min(max_y);

		Pos2::new(x, y)
	}

	#[allow(clippy::too_many_arguments)]
	fn draw_frozen_toolbar(
		ctx: &egui::Context,
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
		cursor: Pos2,
		left_button_down: bool,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		egui::Area::new(Id::new(format!("frozen-toolbar-{}", monitor.id)))
			.order(egui::Order::Foreground)
			.fixed_pos(toolbar_pos)
			.show(ctx, |ui| {
				let (rect, response) =
					ui.allocate_exact_size(toolbar_size, Sense::click_and_drag());
				let body_fill = Self::tinted_hud_body_fill(
					theme,
					hud_blur_active,
					hud_opaque,
					hud_opacity,
					hud_milk_amount,
					hud_tint_hue,
				);
				let toolbar_frame =
					Self::hud_pill_frame(theme, hud_opaque, hud_opacity, body_fill, false);

				if response.drag_started() {
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
				} else if toolbar_state.dragging {
					toolbar_state.dragging = false;
				}

				// Draw the capsule ourselves at the exact allocated rect. This keeps the visible pill
				// and the blur rect perfectly aligned (no shrink-to-content surprises on first frame).
				ui.painter().rect_filled(
					rect,
					f32::from(HUD_PILL_CORNER_RADIUS_POINTS),
					toolbar_frame.fill,
				);
				ui.painter().rect_stroke(
					rect.shrink(0.5),
					CornerRadius::same(HUD_PILL_CORNER_RADIUS_POINTS),
					toolbar_frame.stroke,
					egui::StrokeKind::Inside,
				);

				let inner_stroke_color = match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
					HudTheme::Light => Color32::from_rgba_unmultiplied(255, 255, 255, 140),
				};
				let inner_stroke = egui::Stroke::new(1.0, inner_stroke_color);
				let inner_rect = rect.shrink(1.0);

				ui.painter().rect_stroke(
					inner_rect,
					CornerRadius::same(HUD_PILL_CORNER_RADIUS_POINTS.saturating_sub(1)),
					inner_stroke,
					egui::StrokeKind::Inside,
				);

				let inner_rect = rect.shrink2(egui::vec2(
					HUD_PILL_INNER_MARGIN_X_POINTS,
					HUD_PILL_INNER_MARGIN_Y_POINTS,
				));
				let _ = ui.scope_builder(egui::UiBuilder::new().max_rect(inner_rect), |ui| {
					ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
						ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);

						Self::render_frozen_toolbar_controls(ui, toolbar_state, theme);
					});
				});

				*hud_pill_out = Some(HudPillGeometry {
					rect,
					radius_points: f32::from(HUD_PILL_CORNER_RADIUS_POINTS),
				});
			});
	}

	#[allow(clippy::too_many_arguments)]
	fn render_frozen_toolbar_controls(
		ui: &mut Ui,
		toolbar_state: &mut FrozenToolbarState,
		theme: HudTheme,
	) {
		const TOOLS: [FrozenToolbarTool; 9] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
			FrozenToolbarTool::Done,
		];

		let tools: &[FrozenToolbarTool] = &TOOLS;
		let button_size = FROZEN_TOOLBAR_BUTTON_SIZE_POINTS;
		let button_font_size = 18.0;
		let item_spacing = FROZEN_TOOLBAR_ITEM_SPACING_POINTS;
		let hit_area_inset = 5.0;
		let (normal_color, hover_color, selected_color, hover_bg, selected_bg, selected_border) =
			Self::frozen_toolbar_colors(theme);

		ui.horizontal_centered(|ui| {
			ui.spacing_mut().item_spacing.x = item_spacing;

			for tool in tools {
				let selected = *tool == toolbar_state.selected_tool;
				let response =
					ui.allocate_response(Vec2::new(button_size, button_size), Sense::click());
				let hovered = response.hovered();
				let response = response.on_hover_text(tool.label());
				let hover_anim: f32 = if hovered { 1.0 } else { 0.0 };
				let selected_anim: f32 = if selected { 1.0 } else { 0.0 };
				let glow = hover_anim.max(selected_anim);
				let mut icon_color = normal_color;
				let mut bg_color = Color32::from_rgba_unmultiplied(255, 255, 255, 0);
				let mut border_alpha = 0.0;

				if selected_anim > 0.0 {
					icon_color = Self::blend_color(icon_color, selected_color, selected_anim);
					bg_color = Self::blend_color(bg_color, selected_bg, selected_anim);
					border_alpha = selected_anim;
				}
				if hover_anim > 0.0 {
					icon_color = Self::blend_color(icon_color, hover_color, hover_anim);
					bg_color =
						Self::blend_color(bg_color, hover_bg, hover_anim * (1.0 - selected_anim));
				}
				if glow > 0.0 {
					let bg_rect = response.rect.shrink(hit_area_inset);

					ui.painter().rect_filled(bg_rect, 8.0, bg_color);
				}
				if border_alpha > 0.0 {
					let selected_border = Color32::from_rgba_unmultiplied(
						selected_border.r(),
						selected_border.g(),
						selected_border.b(),
						(selected_border.a() as f32 * border_alpha).round() as u8,
					);

					ui.painter().rect_stroke(
						response.rect.shrink(hit_area_inset),
						8.0,
						egui::Stroke::new(1.0, selected_border),
						egui::StrokeKind::Inside,
					);
				}

				ui.painter().text(
					response.rect.center(),
					Align2::CENTER_CENTER,
					tool.icon(),
					FontId::proportional(button_font_size),
					icon_color,
				);

				if response.clicked() {
					let tool = *tool;

					toolbar_state.selected_tool = tool;
					toolbar_state.needs_redraw = true;
				}
			}
		});
	}

	fn frozen_toolbar_colors(
		theme: HudTheme,
	) -> (Color32, Color32, Color32, Color32, Color32, Color32) {
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
		let selected_border = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 82),
			HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 72),
		};

		(normal_color, hover_color, selected_color, hover_bg, selected_bg, selected_border)
	}

	fn blend_color(a: Color32, b: Color32, t: f32) -> Color32 {
		let t = t.clamp(0.0, 1.0);
		let u = 1.0 - t;

		Color32::from_rgba_unmultiplied(
			((f32::from(a.r()) * u + f32::from(b.r()) * t).round().clamp(0.0, 255.0)) as u8,
			((f32::from(a.g()) * u + f32::from(b.g()) * t).round().clamp(0.0, 255.0)) as u8,
			((f32::from(a.b()) * u + f32::from(b.b()) * t).round().clamp(0.0, 255.0)) as u8,
			((f32::from(a.a()) * u + f32::from(b.a()) * t).round().clamp(0.0, 255.0)) as u8,
		)
	}

	fn clamp_toolbar_position(
		screen_rect: Rect,
		toolbar_size: Vec2,
		cursor: Pos2,
		side_margin: f32,
		top_margin: f32,
	) -> Pos2 {
		let min_x = screen_rect.min.x + side_margin;
		let min_y = screen_rect.min.y + top_margin;
		let max_x = (screen_rect.max.x - toolbar_size.x - side_margin).max(min_x);
		let max_y = (screen_rect.max.y - toolbar_size.y - top_margin * 0.5).max(min_y);

		Pos2::new(cursor.x.clamp(min_x, max_x.max(min_x)), cursor.y.clamp(min_y, max_y.max(min_y)))
	}

	fn should_draw_hud(state: &OverlayState, monitor: MonitorRect) -> bool {
		if cfg!(target_os = "macos") && matches!(state.mode, OverlayMode::Frozen) {
			return true;
		}

		!matches!(state.mode, OverlayMode::Frozen)
			|| state.monitor != Some(monitor)
			|| state.frozen_image.is_some()
			|| state.error_message.is_some()
	}

	#[allow(clippy::too_many_arguments)]
	fn render_hud(
		ctx: &egui::Context,
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

		egui::Area::new("hud".into())
			.order(egui::Order::Foreground)
			.fixed_pos(Pos2::new(hud_x, hud_y))
			.show(ctx, |ui| {
				Self::render_hud_frame(
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
			});
	}

	#[allow(clippy::too_many_arguments)]
	fn render_hud_frame(
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

				ui.label(egui::RichText::new(err).color(err_color).monospace());
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
		let inner_stroke = egui::Stroke::new(1.0, inner_stroke_color);
		let inner_rect = pill_rect.shrink(1.0);

		ui.painter().rect_stroke(
			inner_rect,
			CornerRadius::same(HUD_PILL_CORNER_RADIUS_POINTS.saturating_sub(1)),
			inner_stroke,
			egui::StrokeKind::Inside,
		);

		if !hud_compact {
			Self::render_loupe_tile(
				ui,
				state,
				monitor,
				cursor,
				pill_rect,
				hud_blur_active,
				hud_opaque,
				body_fill,
				theme,
			);
		}
	}

	fn hud_pill_frame(
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
			egui::epaint::Shadow::NONE
		};

		Frame {
			fill: body_fill,
			stroke: egui::Stroke::new(1.0, outer_stroke_color),
			shadow: pill_shadow,
			corner_radius: CornerRadius::same(HUD_PILL_CORNER_RADIUS_POINTS),
			inner_margin: Margin::symmetric(12, 8),
			..Frame::default()
		}
	}

	fn render_hud_content(
		ui: &mut Ui,
		state: &OverlayState,
		_monitor: MonitorRect,
		cursor: GlobalPoint,
		show_alt_hint_keycap: bool,
		theme: HudTheme,
	) {
		let (label_color, secondary_color) = match theme {
			HudTheme::Dark => (
				Color32::from_rgba_unmultiplied(235, 235, 245, 235),
				Color32::from_rgba_unmultiplied(235, 235, 245, 150),
			),
			HudTheme::Light => (
				Color32::from_rgba_unmultiplied(28, 28, 32, 235),
				Color32::from_rgba_unmultiplied(28, 28, 32, 160),
			),
		};
		let pos_text = format!("x={}, y={}", cursor.x, cursor.y);
		let (hex_text, rgb_text) = match state.rgb {
			Some(rgb) => (rgb.hex_upper(), format!("RGB({}, {}, {})", rgb.r, rgb.g, rgb.b)),
			None => (String::from("?"), String::from("RGB(?)")),
		};
		let swatch_size = egui::vec2(10.0, 10.0);

		ui.vertical(|ui| {
			ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
				ui.label(egui::RichText::new(pos_text).color(label_color).monospace());
				ui.label(egui::RichText::new("•").color(secondary_color).monospace());

				let (rect, _) = ui.allocate_exact_size(swatch_size, egui::Sense::hover());
				let swatch_color = match state.rgb {
					Some(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
					None => Color32::from_rgba_unmultiplied(255, 255, 255, 26),
				};

				ui.painter().rect_filled(rect, 3.0, swatch_color);
				ui.painter().rect_stroke(
					rect,
					3.0,
					egui::Stroke::new(
						1.0,
						match theme {
							HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 36),
							HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
						},
					),
					egui::StrokeKind::Inside,
				);
				ui.label(egui::RichText::new(hex_text).color(label_color).monospace());
				ui.label(egui::RichText::new(rgb_text).color(secondary_color).monospace());

				if show_alt_hint_keycap {
					let alt_active = state.alt_held;
					let (keycap_fill, keycap_stroke, keycap_text) = match theme {
						HudTheme::Dark if alt_active => (
							Color32::from_rgba_unmultiplied(255, 255, 255, 40),
							egui::Stroke::new(
								1.0,
								Color32::from_rgba_unmultiplied(255, 255, 255, 70),
							),
							label_color,
						),
						HudTheme::Dark => (
							Color32::from_rgba_unmultiplied(255, 255, 255, 18),
							egui::Stroke::new(
								1.0,
								Color32::from_rgba_unmultiplied(255, 255, 255, 30),
							),
							secondary_color,
						),
						HudTheme::Light if alt_active => (
							Color32::from_rgba_unmultiplied(0, 0, 0, 22),
							egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 64)),
							label_color,
						),
						HudTheme::Light => (
							Color32::from_rgba_unmultiplied(0, 0, 0, 12),
							egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 32)),
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
						ui.label(egui::RichText::new("Alt").color(keycap_text).monospace());
					});
				}
			});
		});
	}

	#[allow(clippy::too_many_arguments)]
	fn render_loupe_tile(
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		pill_rect: Rect,
		hud_blur_active: bool,
		hud_opaque: bool,
		body_fill: Color32,
		theme: HudTheme,
	) {
		let ctx = ui.ctx().clone();

		if !state.alt_held {
			return;
		}

		const CELL: f32 = 10.0;

		let fallback_side_px = state.loupe_patch_side_px.max(1);
		let (w, h) = state
			.loupe
			.as_ref()
			.map(|loupe| loupe.patch.dimensions())
			.unwrap_or((fallback_side_px, fallback_side_px));
		let side = (w.max(h) as f32) * CELL;
		let tile_padding = Margin::same(10);
		let tile_w = side + (tile_padding.left as f32) + (tile_padding.right as f32);
		let tile_h = side + (tile_padding.top as f32) + (tile_padding.bottom as f32);
		let screen = ctx.content_rect();
		let gap = 10.0;
		let mut x = pill_rect.min.x;

		x = x.clamp(screen.min.x + 6.0, (screen.max.x - tile_w - 6.0).max(screen.min.x + 6.0));

		let below_y = pill_rect.max.y + gap;
		let above_y = pill_rect.min.y - gap - tile_h;
		let mut y = if below_y + tile_h <= screen.max.y { below_y } else { above_y };

		y = y.clamp(screen.min.y + 6.0, (screen.max.y - tile_h - 6.0).max(screen.min.y + 6.0));

		let pos = Pos2::new(x, y);

		egui::Area::new(egui::Id::new("rsnap-loupe-tile"))
			.order(egui::Order::Foreground)
			.fixed_pos(pos)
			.show(&ctx, |ui| {
				let _ = hud_blur_active;
				let fill = body_fill;
				let outer_stroke_color = match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 40),
					HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
				};
				let outer_stroke = egui::Stroke::new(1.0, outer_stroke_color);
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

					Self::render_loupe(
						ui,
						state,
						monitor,
						cursor,
						hud_blur_active,
						hud_opaque,
						theme,
					);
				});
			});
	}

	fn render_loupe(
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		hud_blur_active: bool,
		hud_opaque: bool,
		theme: HudTheme,
	) {
		const CELL: f32 = 10.0;

		let mode = state.mode;

		if matches!(mode, OverlayMode::Live) {
			Self::render_live_loupe(ui, state, CELL, hud_blur_active, hud_opaque, theme);
		} else if matches!(mode, OverlayMode::Frozen)
			&& state.monitor == Some(monitor)
			&& (state.frozen_image.is_some() || state.loupe.is_some())
			&& state.cursor.is_some()
		{
			Self::render_frozen_loupe(
				ui,
				state,
				monitor,
				cursor,
				CELL,
				hud_blur_active,
				hud_opaque,
				theme,
			);
		}
	}

	fn render_live_loupe(
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
		let side = (w.max(h) as f32) * cell;
		let (rect, _) = ui.allocate_exact_size(Vec2::new(side, side), egui::Sense::hover());
		let body_fill = hud_body_fill_srgba8(theme, hud_opaque);
		let stroke = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 140));
		let grid_stroke =
			egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 26));

		if let Some(loupe) = state.loupe.as_ref() {
			let _ = loupe.center;

			for y in 0..h {
				for x in 0..w {
					let cell_min =
						Pos2::new(rect.min.x + (x as f32) * cell, rect.min.y + (y as f32) * cell);
					let cell_rect = Rect::from_min_size(cell_min, Vec2::splat(cell));
					let pixel = loupe.patch.get_pixel_checked(x, y).expect("pixel bounds checked");
					let fill = Color32::from_rgb(pixel.0[0], pixel.0[1], pixel.0[2]);

					ui.painter().rect_filled(cell_rect, 0.0, fill);
				}
			}

			let n = w.max(h);

			for i in 0..=n {
				let x = rect.min.x + (i as f32) * cell;
				let y = rect.min.y + (i as f32) * cell;

				ui.painter().line_segment(
					[Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)],
					grid_stroke,
				);
				ui.painter().line_segment(
					[Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
					grid_stroke,
				);
			}
		} else {
			let placeholder_fill =
				Color32::from_rgba_unmultiplied(body_fill[0], body_fill[1], body_fill[2], 255);

			ui.painter().rect_filled(rect, 3.0, placeholder_fill);
		}

		ui.painter().rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Outside);

		let center_x = (w / 2) as f32;
		let center_y = (h / 2) as f32;
		let center_min = Pos2::new(rect.min.x + center_x * cell, rect.min.y + center_y * cell);
		let center_rect = Rect::from_min_size(center_min, Vec2::splat(cell));

		ui.painter().rect_stroke(
			center_rect,
			0.0,
			egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 180)),
			egui::StrokeKind::Inside,
		);
	}

	#[allow(clippy::too_many_arguments)]
	fn render_frozen_loupe(
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		cell: f32,
		hud_blur_active: bool,
		hud_opaque: bool,
		theme: HudTheme,
	) {
		if state.loupe.is_some() {
			Self::render_live_loupe(ui, state, cell, hud_blur_active, hud_opaque, theme);

			return;
		}

		const LOUPE_RADIUS_PX: i32 = 5;
		const LOUPE_SIDE_PX: i32 = (LOUPE_RADIUS_PX * 2) + 1;

		let side = (LOUPE_SIDE_PX as f32) * cell;
		let (rect, _) = ui.allocate_exact_size(Vec2::new(side, side), egui::Sense::hover());
		let Some(image) = state.frozen_image.as_ref() else {
			return;
		};
		let Some((center_x, center_y)) = monitor.local_u32_pixels(cursor) else {
			return;
		};
		let (width, height) = image.dimensions();
		let width = width as i32;
		let height = height as i32;
		let center_x = center_x as i32;
		let center_y = center_y as i32;
		let stroke = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 140));
		let grid_stroke =
			egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 26));

		for dy in -LOUPE_RADIUS_PX..=LOUPE_RADIUS_PX {
			for dx in -LOUPE_RADIUS_PX..=LOUPE_RADIUS_PX {
				let x = center_x + dx;
				let y = center_y + dy;
				let cell_x = dx + LOUPE_RADIUS_PX;
				let cell_y = dy + LOUPE_RADIUS_PX;
				let cell_min = Pos2::new(
					rect.min.x + (cell_x as f32) * cell,
					rect.min.y + (cell_y as f32) * cell,
				);
				let cell_rect = Rect::from_min_size(cell_min, Vec2::splat(cell));
				let fill = if x < 0 || y < 0 || x >= width || y >= height {
					Color32::from_rgba_unmultiplied(0, 0, 0, 0)
				} else {
					let pixel =
						image.get_pixel_checked(x as u32, y as u32).expect("pixel bounds checked");

					Color32::from_rgb(pixel.0[0], pixel.0[1], pixel.0[2])
				};

				ui.painter().rect_filled(cell_rect, 0.0, fill);
			}
		}
		for i in 0..=LOUPE_SIDE_PX {
			let x = rect.min.x + (i as f32) * cell;
			let y = rect.min.y + (i as f32) * cell;

			ui.painter()
				.line_segment([Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)], grid_stroke);
			ui.painter()
				.line_segment([Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)], grid_stroke);
		}

		ui.painter().rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Outside);

		let center_min = Pos2::new(
			rect.min.x + (LOUPE_RADIUS_PX as f32) * cell,
			rect.min.y + (LOUPE_RADIUS_PX as f32) * cell,
		);
		let center_rect = Rect::from_min_size(center_min, Vec2::splat(cell));

		ui.painter().rect_stroke(
			center_rect,
			0.0,
			egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 180)),
			egui::StrokeKind::Inside,
		);
	}

	fn sync_egui_textures(&mut self, gpu: &GpuContext, full_output: &FullOutput) {
		for (id, image_delta) in &full_output.textures_delta.set {
			self.egui_renderer.update_texture(&gpu.device, &gpu.queue, *id, image_delta);
		}
		for id in &full_output.textures_delta.free {
			self.egui_renderer.free_texture(id);
		}
	}

	fn acquire_frame(&mut self, gpu: &GpuContext) -> Result<SurfaceTexture> {
		match self.surface.get_current_texture() {
			Ok(frame) => Ok(frame),
			Err(SurfaceError::Outdated | SurfaceError::Lost) => {
				self.reconfigure(gpu);

				self.needs_reconfigure = false;

				self.surface
					.get_current_texture()
					.wrap_err("Surface was lost and could not be reacquired")
			},
			Err(err) => Err(err).wrap_err("Failed to acquire surface texture"),
		}
	}

	#[allow(clippy::too_many_arguments)]
	fn render_frame(
		&mut self,
		gpu: &GpuContext,
		draw_frozen_bg: bool,
		hud_blur_active: bool,
		frame: SurfaceTexture,
		paint_jobs: &[ClippedPrimitive],
		screen_descriptor: &ScreenDescriptor,
	) -> Result<()> {
		let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
		let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
			label: Some("rsnap-overlay encoder"),
		});
		let _user_cmds = self.egui_renderer.update_buffers(
			&gpu.device,
			&gpu.queue,
			&mut encoder,
			paint_jobs,
			screen_descriptor,
		);

		{
			let rpass_desc = wgpu::RenderPassDescriptor {
				label: Some("rsnap-overlay renderpass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					depth_slice: None,
					resolve_target: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
			};
			let mut rpass = encoder.begin_render_pass(&rpass_desc).forget_lifetime();

			if draw_frozen_bg && let Some(bg) = &self.hud_bg {
				rpass.set_pipeline(&self.mipgen_surface_pipeline);
				rpass.set_bind_group(0, &bg.mipgen_bind_group, &[]);
				rpass.draw(0..3, 0..1);
			}
			if hud_blur_active
				&& self.hud_pill.is_some()
				&& let Some(bg) = &self.hud_bg
			{
				if let Some(pill) = self.hud_pill {
					let ppp = screen_descriptor.pixels_per_point;
					let pad_px = (24.0 * ppp).ceil() as i32;
					let surface_w = screen_descriptor.size_in_pixels[0].max(1) as i32;
					let surface_h = screen_descriptor.size_in_pixels[1].max(1) as i32;
					let min_x_bound = (surface_w - 1).max(0);
					let min_y_bound = (surface_h - 1).max(0);
					let min_x =
						((pill.rect.min.x * ppp).floor() as i32 - pad_px).clamp(0, min_x_bound);
					let min_y =
						((pill.rect.min.y * ppp).floor() as i32 - pad_px).clamp(0, min_y_bound);
					let max_x =
						((pill.rect.max.x * ppp).ceil() as i32 + pad_px).clamp(0, surface_w);
					let max_y =
						((pill.rect.max.y * ppp).ceil() as i32 + pad_px).clamp(0, surface_h);
					let w = (max_x - min_x).max(1) as u32;
					let h = (max_y - min_y).max(1) as u32;

					rpass.set_scissor_rect(min_x as u32, min_y as u32, w, h);
				}

				rpass.set_pipeline(&self.hud_blur_pipeline);
				rpass.set_bind_group(0, &bg.hud_blur_bind_group, &[]);
				rpass.draw(0..3, 0..1);
				rpass.set_scissor_rect(
					0,
					0,
					screen_descriptor.size_in_pixels[0].max(1),
					screen_descriptor.size_in_pixels[1].max(1),
				);
			}

			self.egui_renderer.render(&mut rpass, paint_jobs, screen_descriptor);
		}

		gpu.queue.submit(Some(encoder.finish()));
		frame.present();

		Ok(())
	}

	fn new(
		gpu: &GpuContext,
		window: Arc<winit::window::Window>,
		egui_repaint_deadline: Arc<Mutex<Option<Instant>>>,
	) -> Result<Self> {
		let surface = gpu
			.instance
			.create_surface(Arc::clone(&window))
			.wrap_err("wgpu create_surface failed")?;
		let caps = surface.get_capabilities(&gpu.adapter);
		let surface_format = Self::pick_surface_format(&caps);
		let surface_alpha = Self::pick_surface_alpha(&caps);
		let surface_config =
			Self::make_surface_config(window.as_ref(), surface_format, surface_alpha);

		surface.configure(&gpu.device, &surface_config);

		let egui_ctx = egui::Context::default();
		let mut fonts = FontDefinitions::default();

		egui_phosphor::add_to_fonts(&mut fonts, Variant::Regular);

		egui_ctx.set_fonts(fonts);

		let egui_renderer = Renderer::new(
			&gpu.device,
			surface_format,
			egui_wgpu::RendererOptions {
				msaa_samples: 1,
				depth_stencil_format: None,
				dithering: false,
				predictable_texture_filtering: false,
			},
		);
		let repaint_deadline = Arc::clone(&egui_repaint_deadline);

		egui_ctx.set_request_repaint_callback(move |info| {
			let deadline = Instant::now() + info.delay;
			let mut next_repaint = repaint_deadline.lock().unwrap_or_else(|err| err.into_inner());
			let needs_update = next_repaint.is_none_or(|previous| deadline < previous);

			if needs_update {
				*next_repaint = Some(deadline);
			}
		});

		let bg_sampler = Self::create_bg_sampler(gpu);
		let (mipgen_pipeline, mipgen_bind_group_layout) =
			Self::create_mipgen_pipeline(gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
		let mipgen_surface_pipeline =
			Self::create_mipgen_surface_pipeline(gpu, surface_format, &mipgen_bind_group_layout);
		let (hud_blur_pipeline, hud_blur_bind_group_layout) =
			Self::create_hud_blur_pipeline(gpu, surface_format);
		let hud_blur_uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("rsnap-hud-blur uniform"),
			size: std::mem::size_of::<HudBlurUniformRaw>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let now = Instant::now();

		Ok(Self {
			window,
			surface,
			surface_config,
			needs_reconfigure: false,
			egui_ctx,
			egui_renderer,
			bg_sampler,
			mipgen_pipeline,
			mipgen_surface_pipeline,
			mipgen_bind_group_layout,
			hud_blur_pipeline,
			hud_blur_bind_group_layout,
			hud_blur_uniform,
			hud_bg: None,
			hud_bg_generation: 0,
			hud_pill: None,
			loupe_tile: None,
			hud_theme: None,
			egui_start_time: now,
			egui_last_frame_time: now,
		})
	}

	fn resize(&mut self, size: PhysicalSize<u32>) -> Result<()> {
		self.surface_config.width = size.width.max(1);
		self.surface_config.height = size.height.max(1);
		self.needs_reconfigure = true;

		Ok(())
	}

	fn reconfigure(&mut self, gpu: &GpuContext) {
		self.surface.configure(&gpu.device, &self.surface_config);
	}

	fn sync_egui_theme(&mut self, theme: HudTheme) {
		if self.hud_theme == Some(theme) {
			return;
		}

		match theme {
			HudTheme::Dark => self.egui_ctx.set_visuals(egui::Visuals::dark()),
			HudTheme::Light => self.egui_ctx.set_visuals(egui::Visuals::light()),
		}

		self.hud_theme = Some(theme);
	}

	#[allow(clippy::too_many_arguments)]
	fn draw(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
		draw_hud: bool,
		hud_local_cursor_override: Option<Pos2>,
		hud_compact: bool,
		hud_anchor: HudAnchor,
		show_alt_hint_keycap: bool,
		show_hud_blur: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_fog_amount: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		theme_mode: ThemeMode,
		allow_frozen_surface_bg: bool,
		toolbar_state: Option<&mut FrozenToolbarState>,
		toolbar_pointer: Option<FrozenToolbarPointerState>,
	) -> Result<()> {
		self.apply_pending_reconfigure(gpu);

		let theme = effective_hud_theme(theme_mode, self.window.theme());

		self.sync_egui_theme(theme);

		let (size, pixels_per_point, raw_input) =
			self.prepare_egui_input(gpu, toolbar_pointer, Some(monitor.scale_factor()));
		let toolbar_active = toolbar_state.is_some();

		self.trace_frozen_frame_metrics(state, monitor, size, pixels_per_point, toolbar_active);

		let hud_cfg = Self::resolve_hud_draw_config(
			state,
			monitor,
			draw_hud,
			allow_frozen_surface_bg,
			toolbar_active,
			show_hud_blur,
			hud_opaque,
		);

		self.sync_or_clear_hud_bg(gpu, state, monitor, hud_cfg)?;

		let hud_shader_blur_active = self.hud_shader_blur_active(state, monitor, hud_cfg);
		let (full_output, hud_pill) = self.run_egui(
			raw_input,
			state,
			monitor,
			hud_cfg.can_draw_hud,
			hud_local_cursor_override,
			hud_compact,
			show_hud_blur,
			hud_anchor,
			show_alt_hint_keycap,
			hud_cfg.hud_glass_active,
			hud_opaque,
			hud_opacity,
			hud_milk_amount,
			hud_tint_hue,
			theme,
			toolbar_state,
			toolbar_pointer,
		);

		self.hud_pill = hud_pill;

		if hud_shader_blur_active {
			self.update_hud_blur_uniform(
				gpu,
				size,
				pixels_per_point,
				theme,
				hud_fog_amount,
				hud_milk_amount,
				hud_tint_hue,
			);
		}

		self.sync_egui_textures(gpu, &full_output);

		let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
		let screen_descriptor =
			ScreenDescriptor { size_in_pixels: [size.width, size.height], pixels_per_point };
		let frame = self.acquire_frame(gpu)?;
		let draw_frozen_bg = hud_cfg.needs_frozen_surface_bg
			&& state.monitor == Some(monitor)
			&& state.frozen_image.is_some();

		self.render_frame(
			gpu,
			draw_frozen_bg,
			hud_shader_blur_active,
			frame,
			&paint_jobs,
			&screen_descriptor,
		)?;

		Ok(())
	}

	fn trace_frozen_frame_metrics(
		&self,
		state: &OverlayState,
		monitor: MonitorRect,
		size: PhysicalSize<u32>,
		pixels_per_point: f32,
		toolbar_active: bool,
	) {
		if !matches!(state.mode, OverlayMode::Frozen) || state.monitor != Some(monitor) {
			return;
		}

		let screen_size_points =
			Vec2::new(size.width as f32 / pixels_per_point, size.height as f32 / pixels_per_point);

		tracing::trace!(
					window_id = ?self.window.id(),
					monitor_id = monitor.id,
					window_scale_factor = self.window.scale_factor(),
		monitor_scale_factor = monitor.scale_factor(),
					size_in_pixels = ?size,
					pixels_per_point,
					screen_size_points = ?screen_size_points,
					flip_y = false,
					frozen_generation = state.frozen_generation,
					frozen_image_ready = state.frozen_image.is_some(),
					toolbar_active,
					"Frozen frame metrics."
				);
	}

	fn resolve_hud_draw_config(
		state: &OverlayState,
		monitor: MonitorRect,
		draw_hud: bool,
		allow_frozen_surface_bg: bool,
		toolbar_active: bool,
		show_hud_blur: bool,
		hud_opaque: bool,
	) -> HudDrawConfig {
		let can_draw_hud = draw_hud && Self::should_draw_hud(state, monitor);
		let needs_frozen_surface_bg =
			allow_frozen_surface_bg && !draw_hud && matches!(state.mode, OverlayMode::Frozen);
		// `show_hud_blur` is a UX toggle for "glass mode".
		// - On macOS: HUD uses native compositor blur; toolbar uses native HUD windowing, so shader
		//   blur stays tied to monitor-aligned overlay windows.
		// - On non-macOS: HUD and toolbar remain in overlay windows with shader blur paths.
		let hud_glass_active = can_draw_hud && show_hud_blur && !hud_opaque;
		let toolbar_glass_active = toolbar_active && show_hud_blur && !hud_opaque;
		let use_shader_blur_for_hud = !cfg!(target_os = "macos");
		let needs_shader_blur_bg =
			toolbar_glass_active || (hud_glass_active && use_shader_blur_for_hud);

		HudDrawConfig {
			can_draw_hud,
			needs_frozen_surface_bg,
			needs_shader_blur_bg,
			hud_glass_active,
		}
	}

	fn sync_or_clear_hud_bg(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
		hud_cfg: HudDrawConfig,
	) -> Result<()> {
		if hud_cfg.needs_frozen_surface_bg || hud_cfg.needs_shader_blur_bg {
			return self.sync_hud_bg(gpu, state, monitor);
		}

		self.hud_bg = None;
		self.hud_bg_generation = match state.mode {
			OverlayMode::Live => state.live_bg_generation,
			OverlayMode::Frozen => state.frozen_generation,
		};

		Ok(())
	}

	fn hud_shader_blur_active(
		&self,
		state: &OverlayState,
		monitor: MonitorRect,
		hud_cfg: HudDrawConfig,
	) -> bool {
		hud_cfg.needs_shader_blur_bg
			&& self.hud_bg.is_some()
			&& match state.mode {
				OverlayMode::Live => state.live_bg_monitor == Some(monitor),
				OverlayMode::Frozen => state.monitor == Some(monitor),
			}
	}

	#[allow(clippy::too_many_arguments)]
	fn draw_loupe_tile_window(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		show_hud_blur: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_fog_amount: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		theme_mode: ThemeMode,
	) -> Result<()> {
		self.apply_pending_reconfigure(gpu);

		let theme = effective_hud_theme(theme_mode, self.window.theme());

		self.sync_egui_theme(theme);

		let (size, pixels_per_point, raw_input) =
			self.prepare_egui_input(gpu, None, Some(monitor.scale_factor()));

		self.loupe_tile = None;

		let shader_blur_active = !cfg!(target_os = "macos")
			&& matches!(state.mode, OverlayMode::Frozen)
			&& show_hud_blur
			&& !hud_opaque;
		let hud_cfg = HudDrawConfig {
			can_draw_hud: false,
			needs_frozen_surface_bg: false,
			needs_shader_blur_bg: shader_blur_active,
			hud_glass_active: shader_blur_active,
		};

		self.sync_or_clear_hud_bg(gpu, state, monitor, hud_cfg)?;

		let hud_shader_blur_active = self.hud_shader_blur_active(state, monitor, hud_cfg);
		let hud_blur_active = show_hud_blur && !hud_opaque;
		let body_fill = Self::tinted_hud_body_fill(
			theme,
			hud_blur_active,
			hud_opaque,
			hud_opacity,
			hud_milk_amount,
			hud_tint_hue,
		);
		let (full_output, loupe_tile_rect) = self.run_loupe_tile_egui(
			raw_input,
			state,
			monitor,
			cursor,
			theme,
			hud_blur_active,
			hud_opaque,
			body_fill,
		);

		self.loupe_tile = loupe_tile_rect;

		if hud_shader_blur_active {
			self.hud_pill = loupe_tile_rect.map(|rect| HudPillGeometry {
				rect,
				radius_points: LOUPE_TILE_CORNER_RADIUS_POINTS as f32,
			});

			if self.hud_pill.is_some() {
				self.update_hud_blur_uniform(
					gpu,
					size,
					pixels_per_point,
					theme,
					hud_fog_amount,
					hud_milk_amount,
					hud_tint_hue,
				);
			}
		} else {
			self.hud_pill = None;
		}

		self.sync_egui_textures(gpu, &full_output);

		let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
		let screen_descriptor =
			ScreenDescriptor { size_in_pixels: [size.width, size.height], pixels_per_point };
		let frame = self.acquire_frame(gpu)?;

		self.render_frame(
			gpu,
			false,
			hud_shader_blur_active,
			frame,
			&paint_jobs,
			&screen_descriptor,
		)?;

		Ok(())
	}

	fn tinted_hud_body_fill(
		theme: HudTheme,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
	) -> Color32 {
		let mut opacity = if hud_opaque { 1.0 } else { hud_opacity.clamp(0.0, 1.0) };

		if hud_blur_active {
			opacity = opacity.max(hud_blur_tint_alpha(theme));
		}

		let tint = hud_milk_amount.clamp(0.0, 1.0);
		let mut fill = hud_body_fill_srgba8(theme, false);
		let tint_hue = hud_tint_hue.clamp(0.0, 1.0);
		let tint_saturation = 1.0;
		let (_, _, base_lightness) = rgb_to_hsl(crate::state::Rgb::new(fill[0], fill[1], fill[2]));
		let tinted_target = hsl_to_rgb(tint_hue, tint_saturation, base_lightness);

		fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
			((f32::from(a) + ((f32::from(b) - f32::from(a)) * t)).round().clamp(0.0, 255.0)) as u8
		}

		fill[0] = lerp_u8(fill[0], tinted_target.r, tint);
		fill[1] = lerp_u8(fill[1], tinted_target.g, tint);
		fill[2] = lerp_u8(fill[2], tinted_target.b, tint);
		fill[3] = (opacity * 255.0).round().clamp(0.0, 255.0) as u8;

		Color32::from_rgba_unmultiplied(fill[0], fill[1], fill[2], fill[3])
	}

	#[allow(clippy::too_many_arguments)]
	fn run_loupe_tile_egui(
		&mut self,
		raw_input: egui::RawInput,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		theme: HudTheme,
		hud_blur_active: bool,
		hud_opaque: bool,
		body_fill: Color32,
	) -> (FullOutput, Option<Rect>) {
		let mut loupe_tile_rect = None;
		let full_output = self.egui_ctx.run(raw_input, |ctx| {
			if !state.alt_held {
				return;
			}

			const CELL: f32 = 10.0;

			let fallback_side_px = state.loupe_patch_side_px.max(1);
			let (w, h) = state
				.loupe
				.as_ref()
				.map(|loupe| loupe.patch.dimensions())
				.unwrap_or((fallback_side_px, fallback_side_px));
			let side = (w.max(h) as f32) * CELL;
			let tile_padding = Margin::same(10);
			let outer_stroke_color = match theme {
				HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 40),
				HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
			};
			let outer_stroke = egui::Stroke::new(1.0, outer_stroke_color);
			let shadow = egui::epaint::Shadow {
				offset: [0, 0],
				blur: 10,
				spread: 0,
				color: match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 28),
					HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 18),
				},
			};
			let tile_radius = LOUPE_TILE_CORNER_RADIUS_POINTS as u8;
			let frame = Frame {
				fill: body_fill,
				stroke: outer_stroke,
				shadow,
				corner_radius: CornerRadius::same(tile_radius),
				inner_margin: tile_padding,
				..Frame::default()
			};
			let pad = 6.0;

			egui::Area::new(egui::Id::new("rsnap-loupe-window"))
				.order(egui::Order::Foreground)
				.fixed_pos(Pos2::new(pad, pad))
				.show(ctx, |ui| {
					let inner = frame.show(ui, |ui| {
						ui.set_min_size(Vec2::new(side, side));

						Self::render_loupe(
							ui,
							state,
							monitor,
							cursor,
							hud_blur_active,
							hud_opaque,
							theme,
						);
					});
					let tile_rect = inner.response.rect;

					loupe_tile_rect = Some(tile_rect);

					let inner_stroke_color = match theme {
						HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
						HudTheme::Light => Color32::from_rgba_unmultiplied(255, 255, 255, 140),
					};
					let inner_stroke = egui::Stroke::new(1.0, inner_stroke_color);
					let inner_rect = tile_rect.shrink(1.0);

					ui.painter().rect_stroke(
						inner_rect,
						CornerRadius::same(tile_radius.saturating_sub(1)),
						inner_stroke,
						egui::StrokeKind::Inside,
					);
				});
		});

		(full_output, loupe_tile_rect)
	}

	#[allow(clippy::too_many_arguments)]
	fn update_hud_blur_uniform(
		&mut self,
		gpu: &GpuContext,
		size: PhysicalSize<u32>,
		pixels_per_point: f32,
		theme: HudTheme,
		hud_fog_amount: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
	) {
		if self.hud_bg.is_none() {
			return;
		}

		let Some(hud_pill) = self.hud_pill else {
			return;
		};
		let surface_w = size.width as f32;
		let surface_h = size.height as f32;

		if surface_w <= 0.0 || surface_h <= 0.0 {
			return;
		}

		let rect_min_px =
			[hud_pill.rect.min.x * pixels_per_point, hud_pill.rect.min.y * pixels_per_point];
		let rect_size_px =
			[hud_pill.rect.width() * pixels_per_point, hud_pill.rect.height() * pixels_per_point];
		let rect_min_size = [rect_min_px[0], rect_min_px[1], rect_size_px[0], rect_size_px[1]];
		let max_lod = self.hud_bg.as_ref().map(|bg| bg.max_lod).unwrap_or(0.0);
		let tint =
			Self::tinted_hud_body_fill(theme, false, false, 1.0, hud_milk_amount, hud_tint_hue);
		let tint_rgba = [
			srgb8_to_linear_f32(tint[0]),
			srgb8_to_linear_f32(tint[1]),
			srgb8_to_linear_f32(tint[2]),
			hud_blur_tint_alpha(theme),
		];
		let effects =
			[hud_fog_amount.clamp(0.0, 1.0), hud_milk_amount.clamp(0.0, 1.0), max_lod, 0.0];
		let u = HudBlurUniformRaw {
			rect_min_size,
			radius_blur_soft: [
				hud_pill.radius_points * pixels_per_point,
				(0.9 + (hud_fog_amount.clamp(0.0, 1.0) * 3.2)) * pixels_per_point,
				1.0 * pixels_per_point,
				0.0,
			],
			surface_size_px: [surface_w, surface_h, 0.0, 0.0],
			tint_rgba,
			effects,
		};

		gpu.queue.write_buffer(&self.hud_blur_uniform, 0, u.as_bytes());
	}

	fn sync_hud_bg(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
	) -> Result<()> {
		let (target_generation, target_image) = match state.mode {
			OverlayMode::Live if state.live_bg_monitor == Some(monitor) => {
				(state.live_bg_generation, state.live_bg_image.as_ref())
			},
			OverlayMode::Frozen if state.monitor == Some(monitor) => {
				(state.frozen_generation, state.frozen_image.as_ref())
			},
			OverlayMode::Live => {
				self.hud_bg = None;
				self.hud_bg_generation = state.live_bg_generation;

				return Ok(());
			},
			OverlayMode::Frozen => {
				self.hud_bg = None;
				self.hud_bg_generation = state.frozen_generation;

				return Ok(());
			},
		};

		if self.hud_bg.is_some() && self.hud_bg_generation == target_generation {
			if target_image.is_none() {
				// Keep displaying the already-uploaded background even if image bytes moved.
				return Ok(());
			}

			return Ok(());
		}

		let Some(image) = target_image else {
			// Capture is in progress and no image is available yet.
			self.hud_bg = None;
			self.hud_bg_generation = target_generation;

			return Ok(());
		};

		self.render_frozen_bg_to_texture(gpu, image, target_generation)
	}

	fn render_frozen_bg_to_texture(
		&mut self,
		gpu: &GpuContext,
		image: &RgbaImage,
		target_generation: u64,
	) -> Result<()> {
		let upload_image =
			downscale_for_gpu_upload(image, gpu.device.limits().max_texture_dimension_2d);
		let (width, height) = upload_image.dimensions();
		let max_side = gpu.device.limits().max_texture_dimension_2d;
		let mip_level_count = Self::mip_level_count(width, height).min(10);

		debug_assert!(width <= max_side && height <= max_side);

		let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("rsnap-frozen-bg texture"),
			size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
			mip_level_count,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8UnormSrgb,
			usage: wgpu::TextureUsages::TEXTURE_BINDING
				| wgpu::TextureUsages::COPY_DST
				| wgpu::TextureUsages::RENDER_ATTACHMENT,
			view_formats: &[],
		});
		let upload_bytes = upload_image.as_raw();
		let bytes_per_pixel = 4_usize;
		let unpadded_bytes_per_row = (width as usize) * bytes_per_pixel;
		let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
		let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
		let rgba_padded;
		let rgba_bytes: &[u8] = if padded_bytes_per_row == unpadded_bytes_per_row {
			upload_bytes
		} else {
			let src = upload_bytes;

			rgba_padded =
				pad_rows(src, unpadded_bytes_per_row, padded_bytes_per_row, height as usize);

			&rgba_padded
		};

		gpu.queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &texture,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			rgba_bytes,
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(padded_bytes_per_row as u32),
				rows_per_image: Some(height),
			},
			wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
		);
		self.generate_mipmaps(gpu, &texture, mip_level_count);

		let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
		let hud_blur_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("rsnap-hud-blur bind group"),
			layout: &self.hud_blur_bind_group_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&self.bg_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: self.hud_blur_uniform.as_entire_binding(),
				},
			],
		});
		let mipgen_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("rsnap-mipgen fullscreen bind group"),
			layout: &self.mipgen_bind_group_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&self.bg_sampler),
				},
			],
		});
		let max_lod = (mip_level_count.saturating_sub(1)) as f32;

		self.hud_bg = Some(HudBg {
			_texture: texture,
			_view: view,
			hud_blur_bind_group,
			mipgen_bind_group,
			max_lod,
		});
		self.hud_bg_generation = target_generation;

		Ok(())
	}
}

struct HudBg {
	_texture: wgpu::Texture,
	_view: wgpu::TextureView,
	hud_blur_bind_group: BindGroup,
	mipgen_bind_group: BindGroup,
	max_lod: f32,
}

#[derive(Clone, Copy, Debug)]
struct HudPillGeometry {
	rect: Rect,
	radius_points: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct HudBlurUniformRaw {
	rect_min_size: [f32; 4],
	radius_blur_soft: [f32; 4],
	surface_size_px: [f32; 4],
	tint_rgba: [f32; 4],
	effects: [f32; 4],
}
impl HudBlurUniformRaw {
	fn as_bytes(&self) -> &[u8] {
		unsafe {
			std::slice::from_raw_parts(
				std::ptr::from_ref(self).cast::<u8>(),
				std::mem::size_of::<Self>(),
			)
		}
	}
}

fn srgb8_to_linear_f32(x: u8) -> f32 {
	let c = (x as f32) / 255.0;

	if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn rgb_to_hsl(rgb: crate::state::Rgb) -> (f32, f32, f32) {
	let red = f32::from(rgb.r) / 255.0;
	let green = f32::from(rgb.g) / 255.0;
	let blue = f32::from(rgb.b) / 255.0;
	let max_channel = red.max(green).max(blue);
	let min_channel = red.min(green).min(blue);
	let delta = max_channel - min_channel;
	let lightness = (max_channel + min_channel) / 2.0;

	if delta <= f32::EPSILON {
		return (0.0, 0.0, lightness);
	}

	let saturation = if lightness > 0.5 {
		delta / (2.0 - max_channel - min_channel)
	} else {
		delta / (max_channel + min_channel)
	};
	let mut hue = if (max_channel - red).abs() <= f32::EPSILON {
		(green - blue) / delta + if green < blue { 6.0 } else { 0.0 }
	} else if (max_channel - green).abs() <= f32::EPSILON {
		(blue - red) / delta + 2.0
	} else {
		(red - green) / delta + 4.0
	};

	hue /= 6.0;

	(hue, saturation, lightness)
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> crate::state::Rgb {
	let hue = hue.clamp(0.0, 1.0);
	let saturation = saturation.clamp(0.0, 1.0);
	let lightness = lightness.clamp(0.0, 1.0);

	if saturation <= 0.0 {
		let gray = (lightness * 255.0).round().clamp(0.0, 255.0) as u8;

		return crate::state::Rgb::new(gray, gray, gray);
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

	crate::state::Rgb::new(
		(red * 255.0).round().clamp(0.0, 255.0) as u8,
		(green * 255.0).round().clamp(0.0, 255.0) as u8,
		(blue * 255.0).round().clamp(0.0, 255.0) as u8,
	)
}

fn hue_to_rgb(p: f32, q: f32, hue: f32) -> f32 {
	let mut normalized_hue = hue;

	normalized_hue = normalized_hue.rem_euclid(1.0);

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

fn effective_hud_theme(mode: ThemeMode, window_theme: Option<Theme>) -> HudTheme {
	match mode {
		ThemeMode::System => match window_theme.unwrap_or(Theme::Dark) {
			Theme::Dark => HudTheme::Dark,
			Theme::Light => HudTheme::Light,
		},
		ThemeMode::Dark => HudTheme::Dark,
		ThemeMode::Light => HudTheme::Light,
	}
}

fn hud_body_fill_srgba8(theme: HudTheme, opaque: bool) -> [u8; 4] {
	let mut c = if matches!(theme, HudTheme::Light) {
		HUD_PILL_BODY_FILL_LIGHT_SRGBA8
	} else {
		HUD_PILL_BODY_FILL_DARK_SRGBA8
	};

	if opaque {
		c[3] = 255;
	}

	c
}

fn hud_blur_tint_alpha(theme: HudTheme) -> f32 {
	if matches!(theme, HudTheme::Light) {
		HUD_PILL_BLUR_TINT_ALPHA_LIGHT
	} else {
		HUD_PILL_BLUR_TINT_ALPHA_DARK
	}
}

fn frozen_rgb(
	image: &Option<RgbaImage>,
	monitor: Option<MonitorRect>,
	point: GlobalPoint,
) -> Option<crate::state::Rgb> {
	let Some(image) = image else {
		return None;
	};
	let monitor = monitor?;
	let (x, y) = monitor.local_u32_pixels(point)?;
	let pixel = image.get_pixel_checked(x, y)?;

	Some(crate::state::Rgb::new(pixel.0[0], pixel.0[1], pixel.0[2]))
}

fn frozen_loupe_patch(
	image: &Option<RgbaImage>,
	monitor: Option<MonitorRect>,
	point: GlobalPoint,
	width_px: u32,
	height_px: u32,
) -> Option<RgbaImage> {
	let Some(image) = image else {
		return None;
	};
	let monitor = monitor?;
	let (center_x, center_y) = monitor.local_u32_pixels(point)?;
	let mut out = RgbaImage::new(width_px.max(1), height_px.max(1));
	let out_width = out.width() as i32;
	let out_height = out.height() as i32;
	let half_width = out_width / 2;
	let half_height = out_height / 2;
	let center_x = center_x as i32;
	let center_y = center_y as i32;
	let image_width = image.width() as i32;
	let image_height = image.height() as i32;

	for out_y in 0..out.height() {
		for out_x in 0..out.width() {
			let image_x = center_x + (out_x as i32) - half_width;
			let image_y = center_y + (out_y as i32) - half_height;
			let color = if image_x >= 0
				&& image_y >= 0
				&& image_x < image_width
				&& image_y < image_height
			{
				*image.get_pixel(image_x as u32, image_y as u32)
			} else {
				image::Rgba([0, 0, 0, 0])
			};

			out.put_pixel(out_x, out_y, color);
		}
	}

	Some(out)
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn write_png_bytes_to_clipboard(png_bytes: &[u8]) -> Result<()> {
	use std::ffi::CString;

	use objc::runtime::{BOOL, Object, YES};

	use objc::{class, msg_send, sel, sel_impl};

	let pasteboard_type = CString::new("public.png").wrap_err("Invalid NSPasteboard type")?;

	unsafe {
		let data: *mut Object =
			msg_send![class!(NSData), dataWithBytes: png_bytes.as_ptr() length: png_bytes.len()];
		let pasteboard: *mut Object = msg_send![class!(NSPasteboard), generalPasteboard];
		let _: i64 = msg_send![pasteboard, clearContents];
		let ty: *mut Object =
			msg_send![class!(NSString), stringWithUTF8String: pasteboard_type.as_ptr()];
		let ok: BOOL = msg_send![pasteboard, setData: data forType: ty];

		if ok != YES {
			return Err(eyre::eyre!("NSPasteboard setData:forType failed"));
		}
	}

	Ok(())
}

#[cfg(not(target_os = "macos"))]
fn write_png_bytes_to_clipboard(png_bytes: &[u8]) -> Result<()> {
	use arboard::{Clipboard, ImageData};

	let image = image::load_from_memory(png_bytes).wrap_err("Failed to decode PNG bytes")?;
	let rgba = image.to_rgba8();
	let (width, height) = rgba.dimensions();
	let mut clipboard = Clipboard::new().wrap_err("Failed to initialize clipboard")?;

	clipboard
		.set_image(ImageData {
			width: width as usize,
			height: height as usize,
			bytes: std::borrow::Cow::Owned(rgba.into_raw()),
		})
		.wrap_err("Failed to write image to clipboard")?;

	Ok(())
}

fn write_text_to_clipboard(text: &str) -> Result<()> {
	use arboard::Clipboard;

	let mut clipboard = Clipboard::new().wrap_err("Failed to initialize clipboard")?;

	clipboard.set_text(text.to_string()).wrap_err("Failed to write text to clipboard")?;

	Ok(())
}

fn pad_rows(src: &[u8], src_row_bytes: usize, dst_row_bytes: usize, rows: usize) -> Vec<u8> {
	debug_assert!(dst_row_bytes >= src_row_bytes);

	let mut out = vec![0_u8; dst_row_bytes * rows];

	for y in 0..rows {
		let src_i = y * src_row_bytes;
		let dst_i = y * dst_row_bytes;

		out[dst_i..dst_i + src_row_bytes].copy_from_slice(&src[src_i..src_i + src_row_bytes]);
	}

	out
}

fn downscale_for_gpu_upload(image: &RgbaImage, max_side: u32) -> std::borrow::Cow<'_, RgbaImage> {
	if image.width() <= max_side && image.height() <= max_side {
		return std::borrow::Cow::Borrowed(image);
	}

	let longest_side = image.width().max(image.height()) as f32;
	let scale = (max_side as f32) / longest_side;
	let width = ((image.width() as f32) * scale).round().max(1.0) as u32;
	let height = ((image.height() as f32) * scale).round().max(1.0) as u32;

	std::borrow::Cow::Owned(image::imageops::resize(
		image,
		width.min(max_side),
		height.min(max_side),
		FilterType::Triangle,
	))
}

fn global_to_local(cursor: GlobalPoint, monitor: MonitorRect) -> Option<Pos2> {
	let (x, y) = monitor.local_u32(cursor)?;

	Some(Pos2::new(x as f32, y as f32))
}

#[cfg(target_os = "macos")]
fn macos_configure_overlay_window_mouse_moved_events(window: &winit::window::Window) {
	use objc::runtime::{Object, YES};

	use objc::{msg_send, sel, sel_impl};

	let Ok(handle) = window.window_handle() else {
		return;
	};
	let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
		return;
	};
	let ns_view = appkit.ns_view.as_ptr().cast::<Object>();

	unsafe {
		let ns_window: *mut Object = msg_send![ns_view, window];

		if ns_window.is_null() {
			return;
		}

		let _: () = msg_send![ns_window, setAcceptsMouseMovedEvents: YES];
	}
}

#[cfg(target_os = "macos")]
fn macos_configure_hud_window(
	window: &winit::window::Window,
	blur_enabled: bool,
	blur_amount: f32,
	corner_radius_points: Option<f64>,
) {
	use objc::runtime::{Object, YES};

	use objc::{class, msg_send, sel, sel_impl};

	let Ok(handle) = window.window_handle() else {
		return;
	};
	let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
		return;
	};
	let ns_view = appkit.ns_view.as_ptr().cast::<Object>();

	unsafe {
		let ns_window: *mut Object = msg_send![ns_view, window];

		if ns_window.is_null() {
			return;
		}

		// winit exposes blur as a boolean. We also set an explicit radius so we can drive it from
		// settings (this uses the same private CGS API that winit uses internally).
		{
			use std::ffi::c_void;

			#[link(name = "CoreGraphics", kind = "framework")]
			unsafe extern "C" {
				fn CGSMainConnectionID() -> *mut c_void;

				fn CGSSetWindowBackgroundBlurRadius(
					connection_id: *mut c_void,
					window_id: isize,
					radius: i64,
				) -> i32;
			}

			let amount = blur_amount.clamp(0.0, 1.0);
			let radius = if blur_enabled {
				// Map the slider linearly (0..=1) to the native blur radius.
				// Keep the upper bound conservative; CGS blur radius gets strong quickly.
				let max_radius = 12.0;

				(amount * max_radius).round().clamp(0.0, 200.0) as i64
			} else {
				0
			};
			let window_number: isize = msg_send![ns_window, windowNumber];
			let _ = CGSSetWindowBackgroundBlurRadius(CGSMainConnectionID(), window_number, radius);
		}

		let _: () = msg_send![ns_window, setOpaque: false];
		let _: () = msg_send![ns_window, setHasShadow: false];
		let _: () = msg_send![ns_window, setAcceptsMouseMovedEvents: YES];
		let _: () = msg_send![ns_window, setLevel: MACOS_HUD_WINDOW_LEVEL];
		let sharing_type_none = 0_u64;
		let _: () = msg_send![ns_window, setSharingType: sharing_type_none];
		let clear: *mut Object = msg_send![class!(NSColor), clearColor];
		let _: () = msg_send![ns_window, setBackgroundColor: clear];
		let content_view: *mut Object = msg_send![ns_window, contentView];

		if content_view.is_null() {
			return;
		}

		let _: () = msg_send![content_view, setWantsLayer: YES];
		let layer: *mut Object = msg_send![content_view, layer];

		if layer.is_null() {
			return;
		}

		// Round the window itself so native blur doesn't show a rectangular boundary.
		let scale = window.scale_factor().max(1.0);
		let size = window.inner_size();
		let height_points = (size.height as f64) / scale;
		let radius = corner_radius_points.unwrap_or(height_points * 0.5);
		let _: () = msg_send![layer, setCornerRadius: radius];
		let _: () = msg_send![layer, setMasksToBounds: YES];
	}
}

#[cfg(test)]
mod tests {
	use crate::overlay::{
		HudTheme, Pos2, Rect, TOOLBAR_CAPTURE_GAP_PX, TOOLBAR_SCREEN_MARGIN_PX, Vec2,
		WindowRenderer, hud_blur_tint_alpha, hud_body_fill_srgba8,
	};

	#[test]
	fn frozen_toolbar_default_position_fits_below_capture_rect() {
		let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
		let capture_rect = Rect::from_min_size(Pos2::new(50.0, 100.0), Vec2::new(300.0, 200.0));
		let toolbar_size = Vec2::new(460.0, 54.0);
		let pos = WindowRenderer::frozen_toolbar_default_pos(monitor, capture_rect, toolbar_size);
		let expected_x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(
			TOOLBAR_SCREEN_MARGIN_PX,
			(monitor.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX)
				.max(TOOLBAR_SCREEN_MARGIN_PX),
		);

		assert!((pos.x - expected_x).abs() < f32::EPSILON);
		assert_eq!(pos.y, capture_rect.max.y + TOOLBAR_CAPTURE_GAP_PX);
	}

	#[test]
	fn frozen_toolbar_default_position_falls_inside_when_no_space_below_capture_rect() {
		let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 600.0));
		let toolbar_size = Vec2::new(460.0, 54.0);
		let capture_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 560.0));
		let pos = WindowRenderer::frozen_toolbar_default_pos(monitor, capture_rect, toolbar_size);
		let expected_x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(
			TOOLBAR_SCREEN_MARGIN_PX,
			(monitor.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX)
				.max(TOOLBAR_SCREEN_MARGIN_PX),
		);
		let expected_y = capture_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - toolbar_size.y;

		assert_eq!(pos.x, expected_x);
		assert_eq!(pos.y, capture_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - toolbar_size.y);
		assert_eq!(pos.y, expected_y);
	}

	#[test]
	fn tinted_hud_body_fill_amount_zero_keeps_base_fill() {
		for theme in [HudTheme::Dark, HudTheme::Light] {
			let base_fill = hud_body_fill_srgba8(theme, false);
			let no_tint =
				WindowRenderer::tinted_hud_body_fill(theme, false, false, 1.0, 0.0, 0.585);

			assert_eq!(no_tint.r(), base_fill[0]);
			assert_eq!(no_tint.g(), base_fill[1]);
			assert_eq!(no_tint.b(), base_fill[2]);
			assert_eq!(no_tint.a(), 255);
		}
	}

	#[test]
	fn tinted_hud_body_fill_100pct_tint_is_visibly_blue() {
		let dark_min_delta: u16 = 57;
		let light_min_delta: u16 = 24;
		let sky_tint = 0.585;

		for theme in [HudTheme::Dark, HudTheme::Light] {
			let base_fill =
				WindowRenderer::tinted_hud_body_fill(theme, false, false, 1.0, 0.0, sky_tint);
			let tinted_fill =
				WindowRenderer::tinted_hud_body_fill(theme, false, false, 1.0, 1.0, sky_tint);
			let rgb_delta = u16::from(base_fill.r()).abs_diff(u16::from(tinted_fill.r()))
				+ u16::from(base_fill.g()).abs_diff(u16::from(tinted_fill.g()))
				+ u16::from(base_fill.b()).abs_diff(u16::from(tinted_fill.b()));
			let min_delta =
				if matches!(theme, HudTheme::Dark) { dark_min_delta } else { light_min_delta };

			assert!(
				rgb_delta >= min_delta,
				"expected minimum tint delta >= {min_delta}, got {rgb_delta}"
			);
		}
	}

	#[test]
	fn tinted_hud_body_fill_preserves_alpha() {
		for theme in [HudTheme::Dark, HudTheme::Light] {
			let tint_hue = 0.585;
			let opaque =
				WindowRenderer::tinted_hud_body_fill(theme, false, true, 0.25, 1.0, tint_hue);
			let translucent =
				WindowRenderer::tinted_hud_body_fill(theme, false, false, 0.33, 1.0, tint_hue);

			assert_eq!(opaque.a(), 255);
			assert_eq!(translucent.a(), (0.33_f32 * 255.0).round().clamp(0.0, 255.0) as u8);
		}
	}

	#[test]
	fn tinted_hud_body_fill_blur_active_enforces_min_opacity() {
		for theme in [HudTheme::Dark, HudTheme::Light] {
			let tint_hue = 0.585;
			let fill = WindowRenderer::tinted_hud_body_fill(theme, true, false, 0.0, 0.0, tint_hue);
			let expected = (hud_blur_tint_alpha(theme) * 255.0).round().clamp(0.0, 255.0) as u8;

			assert_eq!(fill.a(), expected);
		}
	}

	#[test]
	fn frozen_toolbar_clamps_floating_position() {
		let monitor = Rect::from_min_size(Pos2::new(-200.0, -100.0), Vec2::new(500.0, 400.0));
		let toolbar_size = Vec2::new(220.0, 42.0);
		let clamped = WindowRenderer::clamp_toolbar_position(
			monitor,
			toolbar_size,
			Pos2::new(-400.0, -240.0),
			TOOLBAR_SCREEN_MARGIN_PX,
			TOOLBAR_SCREEN_MARGIN_PX,
		);

		assert_eq!(clamped.x, monitor.min.x + TOOLBAR_SCREEN_MARGIN_PX);
		assert_eq!(clamped.y, monitor.min.y + TOOLBAR_SCREEN_MARGIN_PX);
	}
}
