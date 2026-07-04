#[cfg(target_os = "macos")]
use std::ffi::{CString, c_char, c_void};
use std::process;
#[cfg(target_os = "macos")]
use std::ptr;

use color_eyre::eyre::Result;
#[cfg(not(target_os = "macos"))]
use color_eyre::eyre::WrapErr;
#[cfg(not(target_os = "macos"))]
use xcap::Window;

use crate::state::WindowRect;

#[cfg(target_os = "macos")]
type CFBooleanRef = *const c_void;
#[cfg(target_os = "macos")]
type CFDictionaryRef = *const c_void;
#[cfg(target_os = "macos")]
type CFNumberRef = *const c_void;
#[cfg(target_os = "macos")]
type CFStringRef = *const c_void;
#[cfg(target_os = "macos")]
type CFTypeRef = *const c_void;
#[cfg(target_os = "macos")]
type CFArrayRef = *const c_void;

#[cfg(target_os = "macos")]
const KCF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
#[cfg(target_os = "macos")]
const KCG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1;
#[cfg(target_os = "macos")]
const KCG_WINDOW_LIST_OPTION_EXCLUDE_DESKTOP: u32 = 16;
#[cfg(target_os = "macos")]
const KCG_WINDOW_LAYER_MAX_FOR_TARGETING: u64 = 3;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_FLOAT64_TYPE: u32 = 6;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_FLOAT32_TYPE: u32 = 5;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_SINT64_TYPE: u32 = 4;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_SINT32_TYPE: u32 = 3;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_CG_FLOAT_TYPE: u32 = 16;

#[cfg(target_os = "macos")]
struct MacWindowListRefGuard(CFArrayRef);
#[cfg(target_os = "macos")]
impl Drop for MacWindowListRefGuard {
	fn drop(&mut self) {
		if !self.0.is_null() {
			unsafe { CFRelease(self.0) };
		}
	}
}

#[cfg(target_os = "macos")]
pub(super) fn collect_window_geometries(
	self_capture_exception_window_ids: &[u32],
) -> Result<Vec<WindowRect>> {
	let window_list_ref = unsafe {
		CGWindowListCopyWindowInfo(
			KCG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | KCG_WINDOW_LIST_OPTION_EXCLUDE_DESKTOP,
			0,
		)
	};

	if window_list_ref.is_null() {
		return Ok(Vec::new());
	}

	let _guard = MacWindowListRefGuard(window_list_ref);
	let window_count = unsafe { CFArrayGetCount(window_list_ref) };

	if window_count <= 0 {
		return Ok(Vec::new());
	}

	let mut windows = Vec::with_capacity(window_count as usize);
	let mut i = 0_isize;

	while i < window_count {
		let Some(window_dict) = cf_dictionary_at_index(window_list_ref, i) else {
			i += 1;

			continue;
		};

		if let Some(window_geometry) =
			window_geometry_from_dictionary(window_dict, self_capture_exception_window_ids)
		{
			windows.push(window_geometry);
		}

		i += 1;
	}

	Ok(windows)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn collect_window_geometries() -> Result<Vec<WindowRect>> {
	let windows = Window::all().wrap_err("xcap Window::all failed")?;
	let self_pid = process::id();
	let mut cached_windows = Vec::with_capacity(windows.len());

	for window in windows {
		let Ok(is_minimized) = window.is_minimized() else {
			continue;
		};

		if is_minimized {
			continue;
		}

		let Ok(window_pid) = window.pid() else {
			continue;
		};

		if window_pid == self_pid {
			continue;
		}

		let Ok(x) = window.x() else {
			continue;
		};
		let Ok(y) = window.y() else {
			continue;
		};
		let Ok(width) = window.width() else {
			continue;
		};
		let Ok(height) = window.height() else {
			continue;
		};
		let window_id = window.id().ok();
		let width = i64::from(width);
		let height = i64::from(height);

		if width <= 0 || height <= 0 {
			continue;
		}

		cached_windows.push(WindowRect {
			window_id,
			x: i64::from(x),
			y: i64::from(y),
			width,
			height,
		});
	}

	Ok(cached_windows)
}

#[cfg(target_os = "macos")]
fn window_geometry_from_dictionary(
	window_dictionary: CFDictionaryRef,
	self_capture_exception_window_ids: &[u32],
) -> Option<WindowRect> {
	let is_on_screen = cf_bool_value(window_dictionary, "kCGWindowIsOnscreen")?;
	let window_id = cf_number_to_u32(window_dictionary, "kCGWindowNumber");
	let owner_pid = cf_number_to_u32(window_dictionary, "kCGWindowOwnerPID");
	let layer = cf_number_to_u64(window_dictionary, "kCGWindowLayer")?;
	let bounds_dict = cf_dictionary_value(window_dictionary, "kCGWindowBounds")?;
	let x = cf_number_to_i64(bounds_dict, "X")?;
	let y = cf_number_to_i64(bounds_dict, "Y")?;
	let width = cf_number_to_i64(bounds_dict, "Width")?;
	let height = cf_number_to_i64(bounds_dict, "Height")?;

	if !is_on_screen || layer > KCG_WINDOW_LAYER_MAX_FOR_TARGETING || width <= 0 || height <= 0 {
		return None;
	}
	if should_exclude_current_process_window(
		window_id,
		owner_pid,
		self_capture_exception_window_ids,
	) {
		return None;
	}

	Some(WindowRect { window_id, x, y, width, height })
}

#[cfg(target_os = "macos")]
fn should_exclude_current_process_window(
	window_id: Option<u32>,
	owner_pid: Option<u32>,
	self_capture_exception_window_ids: &[u32],
) -> bool {
	owner_pid.is_some_and(|pid| pid == process::id())
		&& !window_id.is_some_and(|id| self_capture_exception_window_ids.contains(&id))
}

#[cfg(target_os = "macos")]
fn cf_dictionary_value(dictionary: CFDictionaryRef, key: &str) -> Option<CFTypeRef> {
	let key_ref = cf_string_ref_for_key(key)?;
	let value = unsafe { CFDictionaryGetValue(dictionary, key_ref.cast()) };

	unsafe { CFRelease(key_ref.cast()) };

	if value.is_null() { None } else { Some(value) }
}

#[cfg(target_os = "macos")]
fn cf_bool_value(dictionary: CFDictionaryRef, key: &str) -> Option<bool> {
	let raw = cf_dictionary_value(dictionary, key)? as CFBooleanRef;
	let value = unsafe { CFBooleanGetValue(raw) };

	Some(value != 0)
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
fn cf_number_to_u64(dictionary: CFDictionaryRef, key: &str) -> Option<u64> {
	let raw = cf_dictionary_value(dictionary, key)? as CFNumberRef;
	let value = cf_number_to_f64(raw)?;

	if !value.is_finite() || value < 0.0 { None } else { Some(value.trunc() as u64) }
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

		let mut f32_value = 0.0_f32;

		if CFNumberGetValue(
			number,
			K_CF_NUMBER_FLOAT32_TYPE,
			&mut f32_value as *mut _ as *mut c_void,
		) {
			return Some(f64::from(f32_value));
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

		let mut cg_float_value = 0_f64;

		if CFNumberGetValue(
			number,
			K_CF_NUMBER_CG_FLOAT_TYPE,
			&mut cg_float_value as *mut _ as *mut c_void,
		) {
			return Some(cg_float_value);
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

	if value.is_null() { None } else { Some(value) }
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
	fn CFBooleanGetValue(the_boolean: CFBooleanRef) -> u8;
	fn CFDictionaryGetValue(dict: CFDictionaryRef, key: *const c_void) -> CFTypeRef;
	fn CFNumberGetValue(number: CFNumberRef, the_type: u32, value: *mut c_void) -> bool;
	fn CFRelease(obj: CFTypeRef);
	fn CFStringCreateWithCString(
		allocator: CFTypeRef,
		c_string: *const c_char,
		encoding: u32,
	) -> CFStringRef;
}

#[cfg(test)]
mod tests {
	#[cfg(target_os = "macos")]
	use std::process;

	#[cfg(target_os = "macos")]
	#[test]
	fn current_process_windows_are_excluded_from_window_targeting_unless_excepted() {
		let self_pid = process::id();

		assert!(super::should_exclude_current_process_window(Some(41), Some(self_pid), &[]));
		assert!(!super::should_exclude_current_process_window(Some(41), Some(self_pid), &[41]));
		assert!(!super::should_exclude_current_process_window(Some(41), Some(self_pid + 1), &[],));
		assert!(!super::should_exclude_current_process_window(None, None, &[]));
	}
}
