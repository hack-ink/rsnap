use std::{
	collections::HashMap,
	io::{self, Write as _},
	num::NonZeroU32,
};

use color_eyre::eyre::{self, Context as _, Result};
use raw_window_handle::{
	DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
	RawWindowHandle,
};
use softbuffer::{Context, Surface};
#[cfg(not(target_os = "macos"))] use winit::window::Fullscreen;
use winit::{
	application::ApplicationHandler,
	dpi::{LogicalPosition, PhysicalPosition, PhysicalSize},
	event::{ElementState, MouseButton, WindowEvent},
	event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
	keyboard::{Key, NamedKey},
	monitor::MonitorHandle,
	window::{WindowId, WindowLevel},
};

use rsnap_overlay_protocol::{OverlayOutput, Point, Rect};

const DRAG_THRESHOLD_PX: i32 = 4;
const OVERLAY_ALPHA_U8: u8 = 80;

struct App {
	overlays: HashMap<WindowId, OverlayWindow>,
	state: OverlayState,
}
impl App {
	fn new() -> Self {
		Self { overlays: HashMap::new(), state: OverlayState::new() }
	}

	fn ensure_windows(&mut self, event_loop: &ActiveEventLoop) {
		if !self.overlays.is_empty() {
			return;
		}

		let monitors = event_loop.available_monitors().collect::<Vec<_>>();

		if monitors.is_empty() {
			finish(OverlayOutput::Error { message: String::from("No monitor detected") });
		}

		for monitor in monitors {
			let origin = monitor_origin(&monitor);
			let mut attrs = winit::window::Window::default_attributes()
				.with_title("rsnap-overlay")
				.with_decorations(false)
				.with_resizable(false)
				.with_transparent(true)
				.with_window_level(WindowLevel::AlwaysOnTop);

			#[cfg(target_os = "macos")]
			{
				// On macOS, using `Fullscreen::Borderless` makes the overlay behave like a
				// dedicated full-screen Space, which is not the desired UX. Instead, keep a
				// normal borderless window sized to the monitor and rely on window level +
				// collection behavior to show above other apps.
				let scale_factor = monitor.scale_factor();
				let position: LogicalPosition<f64> =
					monitor.position().to_logical::<f64>(scale_factor);
				let size = monitor.size().to_logical::<f64>(scale_factor);

				attrs = attrs.with_inner_size(size).with_position(position);
			}
			#[cfg(not(target_os = "macos"))]
			{
				attrs = attrs
					.with_inner_size(PhysicalSize::new(monitor.size().width, monitor.size().height))
					.with_position(PhysicalPosition::new(
						monitor.position().x,
						monitor.position().y,
					))
					.with_fullscreen(Some(Fullscreen::Borderless(Some(monitor))));
			}

			let window = match event_loop.create_window(attrs) {
				Ok(window) => window,
				Err(err) => finish(OverlayOutput::Error {
					message: format!("Unable to create overlay window: {err}"),
				}),
			};

			set_overlay_opacity(&window, OVERLAY_ALPHA_U8);

			let display = match Display::from_window(&window) {
				Ok(display) => display,
				Err(err) => finish(OverlayOutput::Error { message: format!("{err:#}") }),
			};
			let renderer = match Renderer::new(display, window) {
				Ok(renderer) => renderer,
				Err(err) => finish(OverlayOutput::Error { message: format!("{err:#}") }),
			};

			self.overlays.insert(renderer.window().id(), OverlayWindow { origin, renderer });
		}

		self.request_redraw_all();
	}

	fn request_redraw_all(&self) {
		for overlay in self.overlays.values() {
			overlay.renderer.request_redraw();
		}
	}
}

impl ApplicationHandler for App {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		self.ensure_windows(event_loop);
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
		let Some(overlay) = self.overlays.get_mut(&window_id) else {
			return;
		};

		match event {
			WindowEvent::CloseRequested => finish(OverlayOutput::Cancel),
			WindowEvent::KeyboardInput { event, .. } => {
				if event.state == ElementState::Pressed
					&& matches!(event.logical_key, Key::Named(NamedKey::Escape))
				{
					finish(OverlayOutput::Cancel);
				}
			},
			WindowEvent::CursorMoved { position, .. } => {
				let (dx, dy) = cursor_delta_for_event(overlay.renderer.window(), position);

				self.state.cursor_global = Point {
					x: overlay.origin.x.saturating_add(dx),
					y: overlay.origin.y.saturating_add(dy),
				};

				if let Some(down_at) = self.state.mouse_down_at_global {
					let dx = (self.state.cursor_global.x - down_at.x).abs();
					let dy = (self.state.cursor_global.y - down_at.y).abs();

					if dx >= DRAG_THRESHOLD_PX || dy >= DRAG_THRESHOLD_PX {
						self.state.dragging = true;
					}
				}

				if !self.state.dragging {
					match hit_test_window_info(self.state.cursor_global) {
						Ok(Some((window_id, rect))) => {
							self.state.hover_window_id = Some(window_id);
							self.state.hover_rect = Some(rect);
						},
						Ok(None) => {
							self.state.hover_window_id = None;
							self.state.hover_rect = None;
						},

						Err(err) => finish(OverlayOutput::Error { message: format!("{err:#}") }),
					}
				} else {
					self.state.hover_window_id = None;
					self.state.hover_rect = None;
				}

				self.request_redraw_all();
			},

			WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
				ElementState::Pressed => {
					self.state.mouse_down_at_global = Some(self.state.cursor_global);
					self.state.dragging = false;

					self.request_redraw_all();
				},
				ElementState::Released => {
					let Some(down_at) = self.state.mouse_down_at_global.take() else {
						return;
					};

					if self.state.dragging {
						let rect = Rect::from_points(down_at, self.state.cursor_global);

						if rect.width == 0 || rect.height == 0 {
							finish(OverlayOutput::Cancel);
						}

						finish(OverlayOutput::Region { rect });
					}

					if let Some(window_id) = self.state.hover_window_id {
						finish(OverlayOutput::Window { window_id });
					}

					match hit_test_window_info(self.state.cursor_global) {
						Ok(Some((window_id, _))) => finish(OverlayOutput::Window { window_id }),
						Ok(None) => finish(OverlayOutput::Cancel),
						Err(err) => finish(OverlayOutput::Error { message: format!("{err:#}") }),
					}
				},
			},
			WindowEvent::Resized(size) => {
				if let Err(err) = overlay.renderer.resize(size) {
					finish(OverlayOutput::Error { message: format!("{err:#}") });
				}

				self.request_redraw_all();
			},
			WindowEvent::ScaleFactorChanged { .. } => {
				let size = overlay.renderer.window().inner_size();

				if let Err(err) = overlay.renderer.resize(size) {
					finish(OverlayOutput::Error { message: format!("{err:#}") });
				}

				self.request_redraw_all();
			},

			WindowEvent::RedrawRequested =>
				if let Err(err) = overlay.renderer.draw(&self.state, overlay.origin) {
					finish(OverlayOutput::Error { message: format!("{err:#}") });
				},
			_ => {},
		}
	}
}

struct OverlayWindow {
	origin: Point,
	renderer: Renderer,
}

struct OverlayState {
	cursor_global: Point,
	mouse_down_at_global: Option<Point>,
	dragging: bool,
	hover_window_id: Option<u32>,
	hover_rect: Option<Rect>,
}
impl OverlayState {
	fn new() -> Self {
		Self {
			cursor_global: Point { x: 0, y: 0 },
			mouse_down_at_global: None,
			dragging: false,
			hover_window_id: None,
			hover_rect: None,
		}
	}
}

#[derive(Debug, Clone, Copy)]
struct Display {
	raw: RawDisplayHandle,
}
impl Display {
	fn from_window(window: &winit::window::Window) -> Result<Self> {
		let raw = window
			.display_handle()
			.map(|handle| handle.as_raw())
			.map_err(|err| crate::eyre::eyre!("Failed to get display handle: {err}"))?;

		Ok(Self { raw })
	}
}

impl HasDisplayHandle for Display {
	fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
		Ok(unsafe { DisplayHandle::borrow_raw(self.raw) })
	}
}

struct Renderer {
	_context: Context<Display>,
	surface: Surface<Display, winit::window::Window>,
	size: PhysicalSize<u32>,
}
impl Renderer {
	fn new(display: Display, window: winit::window::Window) -> Result<Self> {
		let context = Context::new(display)
			.map_err(|err| crate::eyre::eyre!("Failed to create softbuffer context: {err}"))?;
		let size = window.inner_size();
		let size = PhysicalSize { width: size.width.max(1), height: size.height.max(1) };
		let width =
			NonZeroU32::new(size.width).ok_or_else(|| crate::eyre::eyre!("Invalid width"))?;
		let height =
			NonZeroU32::new(size.height).ok_or_else(|| crate::eyre::eyre!("Invalid height"))?;
		let mut surface = Surface::new(&context, window)
			.map_err(|err| crate::eyre::eyre!("Failed to create surface: {err}"))?;

		surface
			.resize(width, height)
			.map_err(|err| crate::eyre::eyre!("Failed to resize surface: {err}"))?;

		Ok(Self { _context: context, surface, size })
	}

	fn window(&self) -> &winit::window::Window {
		self.surface.window()
	}

	fn request_redraw(&self) {
		self.window().request_redraw();
	}

	fn resize(&mut self, size: PhysicalSize<u32>) -> Result<()> {
		let size = PhysicalSize { width: size.width.max(1), height: size.height.max(1) };
		let width =
			NonZeroU32::new(size.width).ok_or_else(|| crate::eyre::eyre!("Invalid width"))?;
		let height =
			NonZeroU32::new(size.height).ok_or_else(|| crate::eyre::eyre!("Invalid height"))?;

		self.surface
			.resize(width, height)
			.map_err(|err| crate::eyre::eyre!("Failed to resize surface: {err}"))?;

		self.size = size;

		Ok(())
	}

	fn draw(&mut self, state: &OverlayState, origin: Point) -> Result<()> {
		let mut buffer = self
			.surface
			.buffer_mut()
			.map_err(|err| crate::eyre::eyre!("Failed to acquire draw buffer: {err}"))?;

		for pixel in buffer.iter_mut() {
			*pixel = 0x00101010;
		}

		if !state.dragging
			&& let Some(hover) = state.hover_rect
		{
			let rect = translate_rect(hover, origin);

			draw_rect_outline(&mut buffer, self.size, rect, 0x0000ff00);
		}
		if state.dragging
			&& let Some(down_at) = state.mouse_down_at_global
		{
			let rect = Rect::from_points(down_at, state.cursor_global);
			let rect = translate_rect(rect, origin);

			draw_rect_outline(&mut buffer, self.size, rect, 0x00ffffff);
		}

		buffer.present().map_err(|err| crate::eyre::eyre!("Failed to present frame: {err}"))?;

		Ok(())
	}
}

fn main() {
	if let Err(err) = run() {
		finish_with(OverlayOutput::Error { message: format!("{err:#}") }, 1);
	}
}

fn run() -> Result<()> {
	color_eyre::install()?;

	let event_loop = EventLoop::new().wrap_err("Failed to create event loop")?;
	let mut app = App::new();

	event_loop.run_app(&mut app).wrap_err("Event loop failed")?;

	Ok(())
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

fn hit_test_window_info(point: Point) -> Result<Option<(u32, Rect)>> {
	let windows = xcap::Window::all().wrap_err("Failed to enumerate windows")?;
	let self_pid = std::process::id();
	let mut best: Option<(u32, Rect, i32)> = None;

	for window in windows.into_iter() {
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

fn monitor_origin(monitor: &MonitorHandle) -> Point {
	let position: LogicalPosition<f64> = monitor.position().to_logical(monitor.scale_factor());

	Point { x: position.x.round() as i32, y: position.y.round() as i32 }
}

fn cursor_delta_for_event(
	window: &winit::window::Window,
	position: PhysicalPosition<f64>,
) -> (i32, i32) {
	#[cfg(target_os = "macos")]
	{
		let logical: LogicalPosition<f64> = position.to_logical(window.scale_factor());

		(logical.x.round() as i32, logical.y.round() as i32)
	}

	#[cfg(not(target_os = "macos"))]
	{
		(position.x.round() as i32, position.y.round() as i32)
	}
}

fn translate_rect(rect: Rect, origin: Point) -> Rect {
	Rect { x: rect.x - origin.x, y: rect.y - origin.y, width: rect.width, height: rect.height }
}

fn draw_rect_outline(buffer: &mut [u32], size: PhysicalSize<u32>, rect: Rect, color: u32) {
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

fn set_overlay_opacity(window: &winit::window::Window, alpha: u8) {
	#[cfg(windows)]
	set_overlay_opacity_windows(window, alpha);
	#[cfg(target_os = "macos")]
	set_overlay_opacity_macos(window, alpha);
}

#[cfg(windows)]
fn set_overlay_opacity_windows(window: &winit::window::Window, alpha: u8) {
	use windows_sys::Win32::Foundation::HWND;

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
	let hwnd = handle.hwnd.get() as HWND;

	unsafe {
		let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
		let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED as i32);
		let _ = SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
	}
}

#[cfg(target_os = "macos")]
fn set_overlay_opacity_macos(window: &winit::window::Window, alpha: u8) {
	use objc::{class, msg_send, runtime::Object, sel, sel_impl};

	let handle = match window.window_handle() {
		Ok(handle) => handle,
		Err(_) => return,
	};
	let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
		return;
	};
	let ns_view = handle.ns_view.as_ptr() as *mut Object;
	let ns_window: *mut Object = unsafe { msg_send![ns_view, window] };

	if ns_window.is_null() {
		return;
	}

	let value = (alpha as f64) / 255.0;

	unsafe {
		let _: () = msg_send![ns_window, setOpaque: false];
		let _: () = msg_send![ns_window, setAlphaValue: value];
		// Ensure the overlay can appear above full-screen apps and across Spaces.
		// NSWindowCollectionBehaviorCanJoinAllSpaces | Transient | FullScreenAuxiliary
		let behavior: usize = 1 | 8 | 256;
		let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
		let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];

		if !ns_app.is_null() {
			let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
		}

		let _: () = msg_send![ns_window, makeKeyAndOrderFront: std::ptr::null::<Object>()];
	}
}
