use std::{
	io::{self, Write as _},
	num::NonZeroU32,
};

use color_eyre::eyre::{Context as _, Result, eyre};
use raw_window_handle::{
	DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
	RawWindowHandle,
};
use rsnap_overlay_protocol::{OverlayOutput, Point, Rect};
use softbuffer::{Context, Surface};
use winit::{
	application::ApplicationHandler,
	dpi::{PhysicalPosition, PhysicalSize},
	event::{ElementState, MouseButton, WindowEvent},
	event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
	keyboard::{Key, NamedKey},
	window::{Fullscreen, Window, WindowId, WindowLevel},
};

const DRAG_THRESHOLD_PX: f64 = 4.0;
const OVERLAY_ALPHA_U8: u8 = 80;

fn main() {
	if let Err(err) = run() {
		finish_with(OverlayOutput::Error { message: format!("{err:#}") }, 1);
	}
}

fn run() -> Result<()> {
	color_eyre::install()?;

	let monitor_pos =
		primary_monitor_origin().wrap_err("Failed to resolve primary monitor origin")?;
	let mut app = App::new(monitor_pos);

	let event_loop = EventLoop::new().wrap_err("Failed to create event loop")?;
	event_loop.run_app(&mut app).wrap_err("Event loop failed")?;

	Ok(())
}

struct App {
	state: OverlayState,
	window_id: Option<WindowId>,
	renderer: Option<Renderer>,
}

impl App {
	fn new(monitor_pos: PhysicalPosition<i32>) -> Self {
		Self { state: OverlayState::new(monitor_pos), window_id: None, renderer: None }
	}

	fn ensure_renderer(&mut self, event_loop: &ActiveEventLoop) {
		if self.renderer.is_some() {
			return;
		}

		let attrs = Window::default_attributes()
			.with_title("rsnap-overlay")
			.with_decorations(false)
			.with_resizable(false)
			.with_transparent(true)
			.with_window_level(WindowLevel::AlwaysOnTop)
			.with_fullscreen(Some(Fullscreen::Borderless(None)));

		let window = match event_loop.create_window(attrs) {
			Ok(window) => window,
			Err(err) => finish_with(OverlayOutput::Error { message: err.to_string() }, 1),
		};
		set_overlay_opacity(&window, OVERLAY_ALPHA_U8);

		let window_id = window.id();
		let display = match Display::from_window(&window) {
			Ok(display) => display,
			Err(err) => finish_with(OverlayOutput::Error { message: format!("{err:#}") }, 1),
		};
		let renderer = match Renderer::new(display, window) {
			Ok(renderer) => renderer,
			Err(err) => finish_with(OverlayOutput::Error { message: format!("{err:#}") }, 1),
		};

		renderer.request_redraw();

		self.window_id = Some(window_id);
		self.renderer = Some(renderer);
	}
}

impl ApplicationHandler for App {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		self.ensure_renderer(event_loop);
	}

	fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
		event_loop.set_control_flow(ControlFlow::Wait);
	}

	fn window_event(
		&mut self,
		_event_loop: &ActiveEventLoop,
		window_id: WindowId,
		event: WindowEvent,
	) {
		if self.window_id != Some(window_id) {
			return;
		}
		let Some(renderer) = self.renderer.as_mut() else {
			return;
		};

		match event {
			WindowEvent::CloseRequested => finish(OverlayOutput::Cancel),
			WindowEvent::KeyboardInput { event, .. }
				if event.state == ElementState::Pressed
					&& matches!(event.logical_key, Key::Named(NamedKey::Escape)) =>
				finish(OverlayOutput::Cancel),
			WindowEvent::CursorMoved { position, .. } => {
				self.state.cursor = position;
				if let Some(down_at) = self.state.mouse_down_at {
					let dx = (self.state.cursor.x - down_at.x).abs();
					let dy = (self.state.cursor.y - down_at.y).abs();
					if dx >= DRAG_THRESHOLD_PX || dy >= DRAG_THRESHOLD_PX {
						self.state.dragging = true;
					}
				}

				if !self.state.dragging {
					let point = self.state.to_global_point(self.state.cursor);
					match hit_test_window(point) {
						Ok(Some((window_id, rect))) => {
							self.state.hover_window_id = Some(window_id);
							self.state.hover_rect = Some(rect);
						},
						Ok(None) => {
							self.state.hover_window_id = None;
							self.state.hover_rect = None;
						},
						Err(err) =>
							finish_with(OverlayOutput::Error { message: format!("{err:#}") }, 1),
					}
				} else {
					self.state.hover_window_id = None;
					self.state.hover_rect = None;
				}
				renderer.request_redraw();
			},
			WindowEvent::MouseInput { state: button_state, button: MouseButton::Left, .. } =>
				match button_state {
					ElementState::Pressed => {
						self.state.mouse_down_at = Some(self.state.cursor);
						self.state.dragging = false;
						renderer.request_redraw();
					},
					ElementState::Released => {
						let Some(down_at) = self.state.mouse_down_at.take() else {
							return;
						};

						if self.state.dragging {
							let a = self.state.to_global_point(down_at);
							let b = self.state.to_global_point(self.state.cursor);
							let rect = Rect::from_points(a, b);
							if rect.width == 0 || rect.height == 0 {
								finish(OverlayOutput::Cancel);
							}
							finish(OverlayOutput::Region { rect });
						}

						if let Some(window_id) = self.state.hover_window_id {
							finish(OverlayOutput::Window { window_id });
						}

						let click = self.state.to_global_point(down_at);
						match hit_test_window(click) {
							Ok(Some((window_id, _))) => finish(OverlayOutput::Window { window_id }),
							Ok(None) => finish(OverlayOutput::Cancel),
							Err(err) =>
								finish_with(OverlayOutput::Error { message: format!("{err:#}") }, 1),
						}
					},
				},
			WindowEvent::Resized(size) => {
				if let Err(err) = renderer.resize(size) {
					finish_with(OverlayOutput::Error { message: format!("{err:#}") }, 1);
				}
				renderer.request_redraw();
			},
			WindowEvent::ScaleFactorChanged { .. } => {
				let size = renderer.window().inner_size();
				if let Err(err) = renderer.resize(size) {
					finish_with(OverlayOutput::Error { message: format!("{err:#}") }, 1);
				}
				renderer.request_redraw();
			},
			WindowEvent::RedrawRequested =>
				if let Err(err) = renderer.draw(&self.state) {
					finish_with(OverlayOutput::Error { message: format!("{err:#}") }, 1);
				},
			_ => {},
		}
	}
}

fn finish(output: OverlayOutput) -> ! {
	finish_with(output, 0)
}

fn finish_with(output: OverlayOutput, exit_code: i32) -> ! {
	match serde_json::to_string(&output) {
		Ok(line) => {
			let mut stdout = io::stdout().lock();
			if stdout.write_all(line.as_bytes()).is_err() {
				std::process::exit(1);
			}
			if stdout.write_all(b"\n").is_err() {
				std::process::exit(1);
			}
			let _ = stdout.flush();
			std::process::exit(exit_code);
		},
		Err(err) => {
			eprintln!("{err:?}");
			std::process::exit(1);
		},
	}
}

fn primary_monitor_origin() -> Result<PhysicalPosition<i32>> {
	let monitors = xcap::Monitor::all().wrap_err("Failed to enumerate monitors")?;
	let monitor = monitors
		.iter()
		.find(|monitor| monitor.is_primary())
		.or_else(|| monitors.first())
		.ok_or_else(|| eyre!("No monitor detected"))?;

	Ok(PhysicalPosition { x: monitor.x(), y: monitor.y() })
}

fn hit_test_window(point: Point) -> Result<Option<(u32, Rect)>> {
	let windows = xcap::Window::all().wrap_err("Failed to enumerate windows")?;
	let self_pid = std::process::id();

	let mut best: Option<(u32, Rect, i32)> = None;
	for window in windows {
		if window.pid() == self_pid
			|| window.is_minimized()
			|| window.width() == 0
			|| window.height() == 0
		{
			continue;
		}

		let contains = point.x >= window.x()
			&& point.y >= window.y()
			&& point.x < window.x().saturating_add_unsigned(window.width())
			&& point.y < window.y().saturating_add_unsigned(window.height());
		if !contains {
			continue;
		}

		let replace = match best.as_ref() {
			None => true,
			Some((_, _, best_z)) => window.z() >= *best_z,
		};
		if replace {
			best = Some((
				window.id(),
				Rect {
					x: window.x(),
					y: window.y(),
					width: window.width(),
					height: window.height(),
				},
				window.z(),
			));
		}
	}

	Ok(best.map(|(id, rect, _z)| (id, rect)))
}

#[derive(Debug, Clone, Copy)]
struct Display {
	raw: RawDisplayHandle,
}

impl Display {
	fn from_window(window: &Window) -> Result<Self> {
		let raw = window
			.display_handle()
			.map(|handle| handle.as_raw())
			.map_err(|err| eyre!("Failed to get display handle: {err}"))?;
		Ok(Self { raw })
	}
}

impl HasDisplayHandle for Display {
	fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
		Ok(unsafe { DisplayHandle::borrow_raw(self.raw) })
	}
}

struct OverlayState {
	monitor_pos: PhysicalPosition<i32>,
	cursor: PhysicalPosition<f64>,
	mouse_down_at: Option<PhysicalPosition<f64>>,
	dragging: bool,
	hover_window_id: Option<u32>,
	hover_rect: Option<Rect>,
}

impl OverlayState {
	fn new(monitor_pos: PhysicalPosition<i32>) -> Self {
		Self {
			monitor_pos,
			cursor: PhysicalPosition { x: 0.0, y: 0.0 },
			mouse_down_at: None,
			dragging: false,
			hover_window_id: None,
			hover_rect: None,
		}
	}

	fn to_global_point(&self, local: PhysicalPosition<f64>) -> Point {
		let x = self.monitor_pos.x.saturating_add(local.x.round() as i32);
		let y = self.monitor_pos.y.saturating_add(local.y.round() as i32);
		Point { x, y }
	}
}

struct Renderer {
	_context: Context<Display>,
	surface: Surface<Display, Window>,
	size: PhysicalSize<u32>,
}

impl Renderer {
	fn new(display: Display, window: Window) -> Result<Self> {
		let context = Context::new(display)
			.map_err(|err| eyre!("Failed to create softbuffer context: {err}"))?;
		let size = window.inner_size();
		let mut surface = Surface::new(&context, window)
			.map_err(|err| eyre!("Failed to create surface: {err}"))?;

		let size = PhysicalSize { width: size.width.max(1), height: size.height.max(1) };
		let width = NonZeroU32::new(size.width).ok_or_else(|| eyre!("Invalid width"))?;
		let height = NonZeroU32::new(size.height).ok_or_else(|| eyre!("Invalid height"))?;
		surface.resize(width, height).map_err(|err| eyre!("Failed to resize surface: {err}"))?;

		Ok(Self { _context: context, surface, size })
	}

	fn window(&self) -> &Window {
		self.surface.window()
	}

	fn request_redraw(&self) {
		self.window().request_redraw();
	}

	fn resize(&mut self, size: PhysicalSize<u32>) -> Result<()> {
		let size = PhysicalSize { width: size.width.max(1), height: size.height.max(1) };
		let width = NonZeroU32::new(size.width).ok_or_else(|| eyre!("Invalid width"))?;
		let height = NonZeroU32::new(size.height).ok_or_else(|| eyre!("Invalid height"))?;
		self.surface
			.resize(width, height)
			.map_err(|err| eyre!("Failed to resize surface: {err}"))?;
		self.size = size;
		Ok(())
	}

	fn draw(&mut self, state: &OverlayState) -> Result<()> {
		let mut buffer = self
			.surface
			.buffer_mut()
			.map_err(|err| eyre!("Failed to acquire draw buffer: {err}"))?;

		let bg = 0x00101010;
		for pixel in buffer.iter_mut() {
			*pixel = bg;
		}

		if !state.dragging
			&& let Some(hover) = state.hover_rect
		{
			let rect = Rect {
				x: hover.x - state.monitor_pos.x,
				y: hover.y - state.monitor_pos.y,
				width: hover.width,
				height: hover.height,
			};
			draw_rect_outline(&mut buffer, self.size, rect, 0x0000ff00);
		}

		if let Some(down_at) = state.mouse_down_at
			&& state.dragging
		{
			let a = Point { x: down_at.x.round() as i32, y: down_at.y.round() as i32 };
			let b = Point { x: state.cursor.x.round() as i32, y: state.cursor.y.round() as i32 };
			let rect = rsnap_overlay_protocol::Rect::from_points(a, b);
			draw_rect_outline(&mut buffer, self.size, rect, 0x00ffffff);
		}

		buffer.present().map_err(|err| eyre!("Failed to present frame: {err}"))?;
		Ok(())
	}
}

fn draw_rect_outline(
	buffer: &mut [u32],
	size: PhysicalSize<u32>,
	rect: rsnap_overlay_protocol::Rect,
	color: u32,
) {
	let width = size.width as i32;
	let height = size.height as i32;
	if width <= 0 || height <= 0 {
		return;
	}

	let x0 = rect.x.clamp(0, width - 1);
	let y0 = rect.y.clamp(0, height - 1);
	let x1 = rect.x.saturating_add_unsigned(rect.width).clamp(0, width);
	let y1 = rect.y.saturating_add_unsigned(rect.height).clamp(0, height);

	if x1 <= x0 || y1 <= y0 {
		return;
	}

	for x in x0..x1 {
		set_pixel(buffer, size, x, y0, color);
		set_pixel(buffer, size, x, y1.saturating_sub(1), color);
	}
	for y in y0..y1 {
		set_pixel(buffer, size, x0, y, color);
		set_pixel(buffer, size, x1.saturating_sub(1), y, color);
	}
}

fn set_pixel(buffer: &mut [u32], size: PhysicalSize<u32>, x: i32, y: i32, color: u32) {
	if x < 0 || y < 0 {
		return;
	}
	let x = x as u32;
	let y = y as u32;
	if x >= size.width || y >= size.height {
		return;
	}
	let idx = (y as usize * size.width as usize).saturating_add(x as usize);
	if idx < buffer.len() {
		buffer[idx] = color;
	}
}

fn set_overlay_opacity(window: &Window, alpha: u8) {
	#[cfg(windows)]
	set_overlay_opacity_windows(window, alpha);
	#[cfg(target_os = "macos")]
	set_overlay_opacity_macos(window, alpha);
}

#[cfg(windows)]
fn set_overlay_opacity_windows(window: &Window, alpha: u8) {
	use windows_sys::Win32::UI::WindowsAndMessaging::{
		GWL_EXSTYLE, GetWindowLongW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongW,
		WS_EX_LAYERED,
	};

	let handle = match window.window_handle() {
		Ok(handle) => handle,
		Err(_) => return,
	};
	let RawWindowHandle::Win32(handle) = handle.as_raw() else {
		return;
	};
	let hwnd = handle.hwnd.get();

	unsafe {
		let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
		let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED as i32);
		let _ = SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
	}
}

#[cfg(target_os = "macos")]
fn set_overlay_opacity_macos(window: &Window, alpha: u8) {
	use objc::{msg_send, sel, sel_impl};

	let handle = match window.window_handle() {
		Ok(handle) => handle,
		Err(_) => return,
	};
	let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
		return;
	};

	let ns_view = handle.ns_view.as_ptr() as *mut objc::runtime::Object;
	let ns_window: *mut objc::runtime::Object = unsafe { msg_send![ns_view, window] };
	if ns_window.is_null() {
		return;
	}

	let value = (alpha as f64) / 255.0;
	unsafe {
		let _: () = msg_send![ns_window, setOpaque: false];
		let _: () = msg_send![ns_window, setAlphaValue: value];
	}
}
