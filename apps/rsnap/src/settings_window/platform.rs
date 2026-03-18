#[cfg(target_os = "macos")]
use std::ffi::{CString, c_char, c_void};
#[cfg(target_os = "macos")]
use std::process;
#[cfg(target_os = "macos")]
use std::ptr;

#[cfg(target_os = "macos")]
use egui::Sense;
use egui::{Rect, Ui};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSView, NSWindow};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, ModifiersState};
#[cfg(target_os = "macos")]
use winit::platform::macos::WindowAttributesExtMacOS;
#[cfg(target_os = "macos")]
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes};

#[cfg(target_os = "macos")]
type CFArrayRef = *const c_void;

#[cfg(target_os = "macos")]
type CFDictionaryRef = *const c_void;

#[cfg(target_os = "macos")]
type CFNumberRef = *const c_void;

#[cfg(target_os = "macos")]
type CFStringRef = *const c_void;

const SETTINGS_TITLEBAR_THEME_BUTTONS_Y_OFFSET_MACOS: f32 = -3.0;
const SETTINGS_TITLEBAR_THEME_BUTTONS_Y_OFFSET_DEFAULT: f32 = 0.0;
const SAVE_SHORTCUT_LABEL_MACOS: &str = "Cmd+S";
const SAVE_SHORTCUT_LABEL_DEFAULT: &str = "Ctrl+S";
#[cfg(any(test, target_os = "macos"))]
const WINDOW_SERVER_BOUNDS_MATCH_TOLERANCE_POINTS: i64 = 2;
#[cfg(target_os = "macos")]
const KCF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
#[cfg(target_os = "macos")]
const KCG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1;
#[cfg(target_os = "macos")]
const KCG_WINDOW_LIST_OPTION_EXCLUDE_DESKTOP: u32 = 16;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_FLOAT64_TYPE: u32 = 6;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_SINT64_TYPE: u32 = 4;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_SINT32_TYPE: u32 = 3;

#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowServerBounds {
	x: i64,
	y: i64,
	width: i64,
	height: i64,
}

#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowServerWindowInfo {
	window_id: u32,
	owner_pid: u32,
	bounds: WindowServerBounds,
}

#[cfg(target_os = "macos")]
struct WindowListRefGuard(CFArrayRef);
#[cfg(target_os = "macos")]
impl Drop for WindowListRefGuard {
	fn drop(&mut self) {
		unsafe {
			CFRelease(self.0.cast());
		}
	}
}

pub(super) fn settings_window_attributes() -> WindowAttributes {
	let attrs = Window::default_attributes()
		.with_title("Settings")
		.with_inner_size(LogicalSize::new(520.0, 360.0))
		.with_resizable(false)
		.with_visible(true);

	#[cfg(target_os = "macos")]
	{
		attrs
			.with_titlebar_transparent(true)
			.with_title_hidden(true)
			.with_fullsize_content_view(true)
			.with_movable_by_window_background(false)
	}

	#[cfg(not(target_os = "macos"))]
	{
		attrs
	}
}

pub(super) fn save_shortcut_label() -> &'static str {
	if cfg!(target_os = "macos") { SAVE_SHORTCUT_LABEL_MACOS } else { SAVE_SHORTCUT_LABEL_DEFAULT }
}

pub(super) fn theme_buttons_y_offset() -> f32 {
	if cfg!(target_os = "macos") {
		SETTINGS_TITLEBAR_THEME_BUTTONS_Y_OFFSET_MACOS
	} else {
		SETTINGS_TITLEBAR_THEME_BUTTONS_Y_OFFSET_DEFAULT
	}
}

pub(super) fn should_close_from_keyboard(modifiers: ModifiersState, event: &KeyEvent) -> bool {
	cfg!(target_os = "macos")
		&& event.state == ElementState::Pressed
		&& modifiers.super_key()
		&& matches!(&event.logical_key, Key::Character(c) if c.as_str().eq_ignore_ascii_case("w"))
}

pub(super) fn install_titlebar_drag(ui: &mut Ui, bar_rect: Rect, window: &Window) {
	#[cfg(target_os = "macos")]
	{
		let drag_response = ui.interact(
			bar_rect,
			ui.make_persistent_id("settings_titlebar_drag"),
			Sense::click_and_drag(),
		);

		if drag_response.drag_started() {
			let _ = window.drag_window();
		}
	}

	#[cfg(not(target_os = "macos"))]
	let _ = (ui, bar_rect, window);
}

pub(super) fn capture_window_id(window: &Window) -> Option<u32> {
	#[cfg(target_os = "macos")]
	{
		let handle = window.window_handle().ok()?;
		let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
			return None;
		};
		let ns_view = unsafe { handle.ns_view.as_ptr().cast::<NSView>().as_ref() }?;
		let ns_window = ns_view.window()?;
		let target_bounds = window_server_bounds_from_ns_window(&ns_window);
		let appkit_window_number = u32::try_from(ns_window.windowNumber()).ok();
		let window_server_windows = current_process_window_server_windows().ok()?;

		select_window_server_window_id(
			process::id(),
			target_bounds,
			appkit_window_number,
			&window_server_windows,
		)
	}
	#[cfg(not(target_os = "macos"))]
	{
		let _ = window;

		None
	}
}

#[cfg(any(test, target_os = "macos"))]
fn select_window_server_window_id(
	target_owner_pid: u32,
	target_bounds: WindowServerBounds,
	appkit_window_number: Option<u32>,
	windows: &[WindowServerWindowInfo],
) -> Option<u32> {
	let mut bounds_match = None;

	for window in windows {
		if window.owner_pid != target_owner_pid
			|| !window_server_bounds_match(window.bounds, target_bounds)
		{
			continue;
		}
		if Some(window.window_id) == appkit_window_number {
			return Some(window.window_id);
		}

		bounds_match.get_or_insert(window.window_id);
	}

	bounds_match
}

#[cfg(any(test, target_os = "macos"))]
fn window_server_bounds_match(lhs: WindowServerBounds, rhs: WindowServerBounds) -> bool {
	approx_equal_i64(lhs.x, rhs.x)
		&& approx_equal_i64(lhs.y, rhs.y)
		&& approx_equal_i64(lhs.width, rhs.width)
		&& approx_equal_i64(lhs.height, rhs.height)
}

#[cfg(any(test, target_os = "macos"))]
fn approx_equal_i64(lhs: i64, rhs: i64) -> bool {
	(lhs - rhs).abs() <= WINDOW_SERVER_BOUNDS_MATCH_TOLERANCE_POINTS
}

#[cfg(target_os = "macos")]
fn window_server_bounds_from_ns_window(ns_window: &NSWindow) -> WindowServerBounds {
	let frame = ns_window.frame();

	WindowServerBounds {
		x: frame.origin.x.round() as i64,
		y: frame.origin.y.round() as i64,
		width: frame.size.width.round() as i64,
		height: frame.size.height.round() as i64,
	}
}

#[cfg(target_os = "macos")]
fn current_process_window_server_windows() -> Result<Vec<WindowServerWindowInfo>, ()> {
	let window_list_ref = unsafe {
		CGWindowListCopyWindowInfo(
			KCG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | KCG_WINDOW_LIST_OPTION_EXCLUDE_DESKTOP,
			0,
		)
	};

	if window_list_ref.is_null() {
		return Ok(Vec::new());
	}

	let _guard = WindowListRefGuard(window_list_ref);
	let window_count = unsafe { CFArrayGetCount(window_list_ref) };

	if window_count <= 0 {
		return Ok(Vec::new());
	}

	let mut windows = Vec::with_capacity(window_count as usize);
	let mut index = 0_isize;

	while index < window_count {
		if let Some(window_dictionary) = cf_dictionary_at_index(window_list_ref, index)
			&& let Some(window_info) = window_server_window_from_dictionary(window_dictionary)
		{
			windows.push(window_info);
		}

		index += 1;
	}

	Ok(windows)
}

#[cfg(target_os = "macos")]
fn window_server_window_from_dictionary(
	window_dictionary: CFDictionaryRef,
) -> Option<WindowServerWindowInfo> {
	let window_id = cf_number_to_u32(window_dictionary, "kCGWindowNumber")?;
	let owner_pid = cf_number_to_u32(window_dictionary, "kCGWindowOwnerPID")?;
	let bounds_dictionary = cf_dictionary_value(window_dictionary, "kCGWindowBounds")?;
	let x = cf_number_to_i64(bounds_dictionary, "X")?;
	let y = cf_number_to_i64(bounds_dictionary, "Y")?;
	let width = cf_number_to_i64(bounds_dictionary, "Width")?;
	let height = cf_number_to_i64(bounds_dictionary, "Height")?;

	Some(WindowServerWindowInfo {
		window_id,
		owner_pid,
		bounds: WindowServerBounds { x, y, width, height },
	})
}

#[cfg(target_os = "macos")]
fn cf_dictionary_value(dictionary: CFDictionaryRef, key: &str) -> Option<CFDictionaryRef> {
	let key_ref = cf_string_ref_for_key(key)?;
	let value = unsafe { CFDictionaryGetValue(dictionary, key_ref.cast()) };

	unsafe {
		CFRelease(key_ref.cast());
	}

	if value.is_null() { None } else { Some(value.cast()) }
}

#[cfg(target_os = "macos")]
fn cf_number_to_i64(dictionary: CFDictionaryRef, key: &str) -> Option<i64> {
	let raw = cf_dictionary_value(dictionary, key)? as CFNumberRef;
	let value = cf_number_to_f64(raw)?;

	if !value.is_finite() { None } else { Some(value.trunc() as i64) }
}

#[cfg(target_os = "macos")]
fn cf_number_to_u32(dictionary: CFDictionaryRef, key: &str) -> Option<u32> {
	let raw = cf_dictionary_value(dictionary, key)? as CFNumberRef;
	let value = cf_number_to_f64(raw)?;

	if !value.is_finite() || value < 0.0 { None } else { Some(value.trunc() as u32) }
}

#[cfg(target_os = "macos")]
fn cf_number_to_f64(number: CFNumberRef) -> Option<f64> {
	let mut f64_value = 0.0_f64;

	unsafe {
		if CFNumberGetValue(
			number,
			K_CF_NUMBER_FLOAT64_TYPE,
			&mut f64_value as *mut _ as *mut c_void,
		) {
			return Some(f64_value);
		}

		let mut int64_value = 0_i64;

		if CFNumberGetValue(
			number,
			K_CF_NUMBER_SINT64_TYPE,
			&mut int64_value as *mut _ as *mut c_void,
		) {
			return Some(int64_value as f64);
		}

		let mut int32_value = 0_i32;

		if CFNumberGetValue(
			number,
			K_CF_NUMBER_SINT32_TYPE,
			&mut int32_value as *mut _ as *mut c_void,
		) {
			return Some(int32_value as f64);
		}
	}

	None
}

#[cfg(target_os = "macos")]
fn cf_string_ref_for_key(key: &str) -> Option<CFStringRef> {
	let key = CString::new(key).ok()?;
	let value =
		unsafe { CFStringCreateWithCString(ptr::null(), key.as_ptr(), KCF_STRING_ENCODING_UTF8) };

	if value.is_null() { None } else { Some(value) }
}

#[cfg(target_os = "macos")]
fn cf_dictionary_at_index(array: CFArrayRef, index: isize) -> Option<CFDictionaryRef> {
	let value = unsafe { CFArrayGetValueAtIndex(array, index) };

	if value.is_null() { None } else { Some(value.cast()) }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
	fn CGWindowListCopyWindowInfo(options: u32, relative_to_window: u32) -> CFArrayRef;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
	fn CFArrayGetCount(array: CFArrayRef) -> isize;
	fn CFArrayGetValueAtIndex(array: CFArrayRef, index: isize) -> *const c_void;
	fn CFDictionaryGetValue(dict: CFDictionaryRef, key: *const c_void) -> *const c_void;
	fn CFNumberGetValue(number: CFNumberRef, the_type: u32, value: *mut c_void) -> bool;
	fn CFRelease(obj: *const c_void);
	fn CFStringCreateWithCString(
		allocator: *const c_void,
		c_string: *const c_char,
		encoding: u32,
	) -> CFStringRef;
}

#[cfg(test)]
mod tests {
	use crate::settings_window::platform::{self, WindowServerBounds, WindowServerWindowInfo};

	#[test]
	fn select_window_server_window_id_matches_current_process_bounds() {
		let windows = [
			WindowServerWindowInfo {
				window_id: 11,
				owner_pid: 7,
				bounds: WindowServerBounds { x: 10, y: 20, width: 520, height: 360 },
			},
			WindowServerWindowInfo {
				window_id: 42,
				owner_pid: 99,
				bounds: WindowServerBounds { x: 10, y: 20, width: 520, height: 360 },
			},
		];

		assert_eq!(
			platform::select_window_server_window_id(
				7,
				WindowServerBounds { x: 10, y: 20, width: 520, height: 360 },
				None,
				&windows,
			),
			Some(11)
		);
	}

	#[test]
	fn select_window_server_window_id_uses_appkit_number_as_tiebreaker() {
		let windows = [
			WindowServerWindowInfo {
				window_id: 11,
				owner_pid: 7,
				bounds: WindowServerBounds { x: 10, y: 20, width: 520, height: 360 },
			},
			WindowServerWindowInfo {
				window_id: 17,
				owner_pid: 7,
				bounds: WindowServerBounds { x: 11, y: 19, width: 520, height: 360 },
			},
		];

		assert_eq!(
			platform::select_window_server_window_id(
				7,
				WindowServerBounds { x: 10, y: 20, width: 520, height: 360 },
				Some(17),
				&windows,
			),
			Some(17)
		);
	}
}
