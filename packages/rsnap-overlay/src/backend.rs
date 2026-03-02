#[cfg(target_os = "macos")]
use std::ffi::{CString, c_char, c_void};
use std::process;
use std::sync::Arc;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr};
use image::RgbaImage;
use thiserror::Error;

#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::MacLiveFrameStream;
use crate::state::{
	GlobalPoint, LiveCursorSample, MonitorImageSnapshot, MonitorRect, RectPoints, Rgb,
	WindowListSnapshot, WindowRect,
};

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
const K_CF_NUMBER_FLOAT64_TYPE: u32 = 6;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_FLOAT32_TYPE: u32 = 5;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_SINT64_TYPE: u32 = 4;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_SINT32_TYPE: u32 = 3;
#[cfg(target_os = "macos")]
const K_CF_NUMBER_CG_FLOAT_TYPE: u32 = 16;

pub trait CaptureBackend: Send {
	fn global_cursor_position(&mut self) -> Result<Option<GlobalPoint>> {
		Ok(None)
	}
	fn capture_monitor(&mut self, monitor: MonitorRect) -> Result<RgbaImage>;
	fn pixel_rgb_in_monitor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
	) -> Result<Option<Rgb>>;
	fn live_sample_cursor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		want_patch: bool,
		patch_width_px: u32,
		patch_height_px: u32,
	) -> Result<LiveCursorSample> {
		let rgb = self.pixel_rgb_in_monitor(monitor, point)?;
		let patch = if want_patch {
			self.rgba_patch_in_monitor(monitor, point, patch_width_px, patch_height_px)?
		} else {
			None
		};

		Ok(LiveCursorSample { rgb, patch })
	}
	fn hit_test_window_in_monitor(
		&mut self,
		_monitor: MonitorRect,
		_point: GlobalPoint,
	) -> Result<Option<RectPoints>> {
		Ok(None)
	}
	fn rgba_patch_in_monitor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		width_px: u32,
		height_px: u32,
	) -> Result<Option<RgbaImage>>;

	fn refresh_monitor_cache(
		&mut self,
		_monitor: MonitorRect,
	) -> Result<Arc<MonitorImageSnapshot>> {
		Err(CaptureBackendError::NotSupported { backend: "capture backend" }.into())
	}

	fn latest_monitor_cache_snapshot(&self) -> Option<Arc<MonitorImageSnapshot>> {
		None
	}

	fn refresh_window_cache(&mut self) -> Result<Arc<WindowListSnapshot>> {
		Err(CaptureBackendError::NotSupported { backend: "capture backend" }.into())
	}

	fn latest_window_cache_snapshot(&self) -> Option<Arc<WindowListSnapshot>> {
		None
	}
}

#[derive(Debug, Error)]
pub enum CaptureBackendError {
	#[error("screen capture is not supported on this platform (backend: {backend})")]
	NotSupported { backend: &'static str },

	#[error("no monitor matched rect: {monitor:?}")]
	MonitorNotFound { monitor: MonitorRect },
}

pub struct StubCaptureBackend {}
impl StubCaptureBackend {
	#[must_use]
	pub fn new() -> Self {
		Self {}
	}
}

impl Default for StubCaptureBackend {
	fn default() -> Self {
		Self::new()
	}
}

impl CaptureBackend for StubCaptureBackend {
	fn capture_monitor(&mut self, _monitor: MonitorRect) -> Result<RgbaImage> {
		Err(CaptureBackendError::NotSupported { backend: "stub" }.into())
	}

	fn pixel_rgb_in_monitor(
		&mut self,
		_monitor: MonitorRect,
		_point: GlobalPoint,
	) -> Result<Option<Rgb>> {
		Ok(None)
	}

	fn rgba_patch_in_monitor(
		&mut self,
		_monitor: MonitorRect,
		_point: GlobalPoint,
		_width_px: u32,
		_height_px: u32,
	) -> Result<Option<RgbaImage>> {
		Ok(None)
	}

	fn refresh_monitor_cache(
		&mut self,
		_monitor: MonitorRect,
	) -> Result<Arc<MonitorImageSnapshot>> {
		Err(CaptureBackendError::NotSupported { backend: "stub" }.into())
	}

	fn latest_monitor_cache_snapshot(&self) -> Option<Arc<MonitorImageSnapshot>> {
		None
	}

	fn refresh_window_cache(&mut self) -> Result<Arc<WindowListSnapshot>> {
		Err(CaptureBackendError::NotSupported { backend: "stub" }.into())
	}

	fn latest_window_cache_snapshot(&self) -> Option<Arc<WindowListSnapshot>> {
		None
	}
}

pub struct XcapCaptureBackend {
	cache: Option<Arc<MonitorImageSnapshot>>,
	cache_ttl: Duration,
	window_cache: Option<Arc<WindowListSnapshot>>,
	window_cache_ttl: Duration,
	#[cfg(target_os = "macos")]
	live_frame_stream: MacLiveFrameStream,
}
impl XcapCaptureBackend {
	#[must_use]
	pub fn new() -> Self {
		Self {
			cache: None,
			cache_ttl: Duration::from_millis(200),
			window_cache: None,
			window_cache_ttl: Duration::from_millis(250),
			#[cfg(target_os = "macos")]
			live_frame_stream: MacLiveFrameStream::new(),
		}
	}

	fn cache_valid_for(&self, monitor: MonitorRect) -> bool {
		let Some(cache) = &self.cache else {
			return false;
		};

		cache.monitor == monitor && cache.captured_at.elapsed() <= self.cache_ttl
	}

	fn ensure_cache(&mut self, monitor: MonitorRect) -> Result<()> {
		if self.cache_valid_for(monitor) {
			return Ok(());
		}

		self.refresh_monitor_cache(monitor)?;

		Ok(())
	}

	pub(crate) fn refresh_monitor_cache(
		&mut self,
		monitor: MonitorRect,
	) -> Result<Arc<MonitorImageSnapshot>> {
		#[cfg(target_os = "macos")]
		if let Some(snapshot) = self.live_frame_stream.latest_rgba_snapshot(monitor) {
			self.cache = Some(snapshot.clone());

			return Ok(snapshot);
		}

		let image = capture_monitor_image(monitor)
			.wrap_err_with(|| format!("failed to capture monitor for rgb sampling: {monitor:?}"))?;
		let snapshot = Arc::new(MonitorImageSnapshot {
			captured_at: Instant::now(),
			monitor,
			image: Arc::new(image),
		});

		self.cache = Some(snapshot.clone());

		Ok(snapshot)
	}

	pub(crate) fn latest_monitor_cache_snapshot(&self) -> Option<Arc<MonitorImageSnapshot>> {
		self.cache.clone()
	}

	fn window_cache_valid_for(&self) -> bool {
		let Some(cache) = &self.window_cache else {
			return false;
		};

		cache.captured_at.elapsed() <= self.window_cache_ttl
	}

	fn ensure_window_cache(&mut self) -> Result<()> {
		if self.window_cache_valid_for() {
			return Ok(());
		}

		self.refresh_window_cache()?;

		Ok(())
	}

	pub(crate) fn refresh_window_cache(&mut self) -> Result<Arc<WindowListSnapshot>> {
		let windows = collect_window_geometries().wrap_err("failed to refresh window cache")?;
		let snapshot = Arc::new(WindowListSnapshot {
			captured_at: Instant::now(),
			windows: Arc::new(windows),
		});

		self.window_cache = Some(snapshot.clone());

		Ok(snapshot)
	}

	pub(crate) fn latest_window_cache_snapshot(&self) -> Option<Arc<WindowListSnapshot>> {
		self.window_cache.clone()
	}
}

impl Default for XcapCaptureBackend {
	fn default() -> Self {
		Self::new()
	}
}

impl CaptureBackend for XcapCaptureBackend {
	fn refresh_monitor_cache(&mut self, monitor: MonitorRect) -> Result<Arc<MonitorImageSnapshot>> {
		XcapCaptureBackend::refresh_monitor_cache(self, monitor)
	}

	fn latest_monitor_cache_snapshot(&self) -> Option<Arc<MonitorImageSnapshot>> {
		XcapCaptureBackend::latest_monitor_cache_snapshot(self)
	}

	fn refresh_window_cache(&mut self) -> Result<Arc<WindowListSnapshot>> {
		XcapCaptureBackend::refresh_window_cache(self)
	}

	fn latest_window_cache_snapshot(&self) -> Option<Arc<WindowListSnapshot>> {
		XcapCaptureBackend::latest_window_cache_snapshot(self)
	}

	fn hit_test_window_in_monitor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
	) -> Result<Option<RectPoints>> {
		if !monitor.contains(point) {
			return Ok(None);
		}

		self.ensure_window_cache()?;

		let Some((local_x, local_y)) = monitor.local_u32(point) else {
			return Ok(None);
		};
		let Some(window_cache) = &self.window_cache else {
			return Ok(None);
		};

		for geometry in window_cache.windows.iter() {
			let Some(window_rect) = monitor.clip_global_rect_i64(
				geometry.x,
				geometry.y,
				geometry.x.saturating_add(geometry.width),
				geometry.y.saturating_add(geometry.height),
			) else {
				continue;
			};

			if !window_rect.contains((local_x, local_y)) {
				continue;
			}

			return Ok(Some(window_rect));
		}

		Ok(None)
	}

	fn capture_monitor(&mut self, monitor: MonitorRect) -> Result<RgbaImage> {
		let image = capture_monitor_image(monitor).wrap_err_with(|| {
			format!("failed to capture monitor for freeze/export: {monitor:?}")
		})?;

		self.cache = Some(Arc::new(MonitorImageSnapshot {
			captured_at: Instant::now(),
			monitor,
			image: Arc::new(image.clone()),
		}));

		Ok(image)
	}

	fn pixel_rgb_in_monitor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
	) -> Result<Option<Rgb>> {
		if !monitor.contains(point) {
			return Ok(None);
		}

		#[cfg(target_os = "macos")]
		if let Some((x, y)) = monitor.local_u32_pixels(point)
			&& let Some(rgb) = self.live_frame_stream.sample_rgb(monitor, x, y)
		{
			return Ok(Some(rgb));
		}

		self.ensure_cache(monitor)?;

		let Some(cache) = self.cache.as_ref() else {
			return Ok(None);
		};
		let Some((x, y)) = monitor.local_u32_pixels(point) else {
			return Ok(None);
		};
		let Some(pixel) = cache.image.get_pixel_checked(x, y) else {
			return Ok(None);
		};

		Ok(Some(Rgb::new(pixel.0[0], pixel.0[1], pixel.0[2])))
	}

	fn live_sample_cursor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		want_patch: bool,
		patch_width_px: u32,
		patch_height_px: u32,
	) -> Result<LiveCursorSample> {
		#[cfg(target_os = "macos")]
		{
			if let Some((x_px, y_px)) = monitor.local_u32_pixels(point) {
				if let Some(sample) = self.live_frame_stream.sample_cursor(
					monitor,
					x_px,
					y_px,
					want_patch,
					patch_width_px,
					patch_height_px,
				) {
					return Ok(sample);
				}

				let rgb = self.pixel_rgb_in_monitor(monitor, point)?;
				let patch = if want_patch {
					self.rgba_patch_in_monitor(monitor, point, patch_width_px, patch_height_px)?
				} else {
					None
				};

				Ok(LiveCursorSample { rgb, patch })
			} else {
				Ok(LiveCursorSample { rgb: None, patch: None })
			}
		}
		#[cfg(not(target_os = "macos"))]
		{
			let rgb = self.pixel_rgb_in_monitor(monitor, point)?;
			let patch = if want_patch {
				self.rgba_patch_in_monitor(monitor, point, patch_width_px, patch_height_px)?
			} else {
				None
			};

			Ok(LiveCursorSample { rgb, patch })
		}
	}

	fn rgba_patch_in_monitor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		width_px: u32,
		height_px: u32,
	) -> Result<Option<RgbaImage>> {
		if !monitor.contains(point) {
			return Ok(None);
		}

		#[cfg(target_os = "macos")]
		if let Some((center_x, center_y)) = monitor.local_u32_pixels(point)
			&& let Some(patch) = self
				.live_frame_stream
				.sample_rgba_patch(monitor, center_x, center_y, width_px, height_px)
		{
			return Ok(Some(patch));
		}

		self.ensure_cache(monitor)?;

		let Some(cache) = self.cache.as_ref() else {
			return Ok(None);
		};
		let Some((center_x, center_y)) = monitor.local_u32_pixels(point) else {
			return Ok(None);
		};

		Ok(Some(copy_rgba_patch(&cache.image, center_x, center_y, width_px, height_px)))
	}
}

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

#[must_use]
pub fn default_capture_backend() -> Box<dyn CaptureBackend> {
	Box::new(XcapCaptureBackend::new())
}

fn copy_rgba_patch(
	image: &RgbaImage,
	center_x: u32,
	center_y: u32,
	width_px: u32,
	height_px: u32,
) -> RgbaImage {
	let mut out = RgbaImage::new(width_px.max(1), height_px.max(1));
	let out_w = out.width();
	let out_h = out.height();
	let in_w = image.width() as i32;
	let in_h = image.height() as i32;
	let half_w = (out_w as i32) / 2;
	let half_h = (out_h as i32) / 2;
	let center_x = center_x as i32;
	let center_y = center_y as i32;

	for oy in 0..out_h {
		for ox in 0..out_w {
			let ix = center_x + (ox as i32) - half_w;
			let iy = center_y + (oy as i32) - half_h;

			if ix >= 0 && iy >= 0 && ix < in_w && iy < in_h {
				let pixel = image.get_pixel(ix as u32, iy as u32);

				out.put_pixel(ox, oy, *pixel);
			} else {
				out.put_pixel(ox, oy, image::Rgba([0, 0, 0, 0]));
			}
		}
	}

	out
}

#[cfg(target_os = "macos")]
fn collect_window_geometries() -> Result<Vec<WindowRect>> {
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

	let self_pid = process::id();
	let mut windows = Vec::with_capacity(window_count as usize);
	let mut i = 0_isize;

	while i < window_count {
		let Some(window_dict) = cf_dictionary_at_index(window_list_ref, i) else {
			i += 1;

			continue;
		};

		if let Some(window_geometry) = window_geometry_from_dictionary(window_dict, self_pid) {
			windows.push(window_geometry);
		}

		i += 1;
	}

	Ok(windows)
}

#[cfg(target_os = "macos")]
fn window_geometry_from_dictionary(
	window_dictionary: CFDictionaryRef,
	self_pid: u32,
) -> Option<WindowRect> {
	let is_on_screen = cf_bool_value(window_dictionary, "kCGWindowIsOnscreen")?;
	let window_pid = cf_number_to_u32(window_dictionary, "kCGWindowOwnerPID")?;
	let layer = cf_number_to_u64(window_dictionary, "kCGWindowLayer")?;
	let bounds_dict = cf_dictionary_value(window_dictionary, "kCGWindowBounds")?;
	let x = cf_number_to_i64(bounds_dict, "X")?;
	let y = cf_number_to_i64(bounds_dict, "Y")?;
	let width = cf_number_to_i64(bounds_dict, "Width")?;
	let height = cf_number_to_i64(bounds_dict, "Height")?;

	if !is_on_screen || layer != 0 || window_pid == self_pid || width <= 0 || height <= 0 {
		return None;
	}

	Some(WindowRect { x, y, width, height })
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
	let value = unsafe {
		CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), KCF_STRING_ENCODING_UTF8)
	};

	if value.is_null() { None } else { Some(value) }
}

#[cfg(target_os = "macos")]
fn cf_dictionary_at_index(array: CFArrayRef, index: isize) -> Option<CFDictionaryRef> {
	let value = unsafe { CFArrayGetValueAtIndex(array, index) };

	if value.is_null() { None } else { Some(value) }
}

#[cfg(not(target_os = "macos"))]
fn collect_window_geometries() -> Result<Vec<WindowRect>> {
	let windows = xcap::Window::all().wrap_err("xcap Window::all failed")?;
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
		let width = i64::from(width);
		let height = i64::from(height);

		if width <= 0 || height <= 0 {
			continue;
		}

		cached_windows.push(WindowRect { x: i64::from(x), y: i64::from(y), width, height });
	}

	Ok(cached_windows)
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

fn capture_monitor_image(monitor: MonitorRect) -> Result<RgbaImage> {
	let xcap_monitor = xcap_find_monitor(monitor)?;
	let image = xcap_monitor.capture_image().wrap_err("xcap capture_image failed")?;

	Ok(image)
}

fn xcap_find_monitor(monitor: MonitorRect) -> Result<xcap::Monitor> {
	let monitors = xcap::Monitor::all().wrap_err("xcap Monitor::all failed")?;

	for m in monitors {
		if m.id().wrap_err("Failed to read xcap monitor id")? == monitor.id {
			return Ok(m);
		}
	}

	Err(CaptureBackendError::MonitorNotFound { monitor }.into())
}

#[cfg(test)]
mod tests {
	use crate::backend::CaptureBackend;

	use crate::backend::StubCaptureBackend;

	#[test]
	fn stub_backend_returns_cursor_position() {
		let mut backend = StubCaptureBackend::new();
		let pos = backend.global_cursor_position().unwrap();

		assert!(pos.is_none());
	}
}
