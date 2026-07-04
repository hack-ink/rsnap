#![allow(
	dead_code,
	reason = "XY-113 narrows the public crate facade while leaving backend implementation cleanup to a separate follow-up lane."
)]

mod image_capture;
#[cfg(target_os = "macos")]
mod macos_region_capture;
mod window_list;

use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use color_eyre::eyre;
use color_eyre::eyre::{Result, WrapErr};
use image::RgbaImage;
#[cfg(target_os = "macos")]
use objc2_core_foundation::CGRect;
#[allow(
	deprecated,
	reason = "Legacy CG capture remains as the macOS fallback while ScreenCaptureKit owns the primary capture path."
)]
#[cfg(target_os = "macos")]
use objc2_core_graphics::{CGDisplayCreateImage, CGWindowListCreateImage};
#[cfg(target_os = "macos")]
use objc2_core_graphics::{CGRectNull, CGWindowID, CGWindowImageOption, CGWindowListOption};
use thiserror::Error;
#[cfg(not(target_os = "macos"))]
use xcap::Window;

#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::{CursorSampleRequest, MacLiveFrameStream};
use crate::state::{
	GlobalPoint, LiveCursorSample, MonitorImageSnapshot, MonitorRect, RectPoints, Rgb, WindowHit,
	WindowListSnapshot,
};

/// Capture backend contract used by the overlay worker.
pub trait CaptureBackend: Send {
	/// Returns the current global cursor position when the backend can provide it.
	fn global_cursor_position(&mut self) -> Result<Option<GlobalPoint>> {
		Ok(None)
	}

	/// Captures a full monitor image for the provided monitor descriptor.
	fn capture_monitor(&mut self, monitor: MonitorRect) -> Result<RgbaImage>;

	/// Captures a monitor sub-rectangle in monitor-local pixels.
	fn capture_monitor_region(
		&mut self,
		_monitor: MonitorRect,
		_rect_px: RectPoints,
	) -> Result<RgbaImage> {
		Err(CaptureBackendError::NotSupported { backend: "capture backend" }.into())
	}

	/// Captures a fresh monitor region suitable for scroll-capture sampling when supported.
	fn capture_monitor_region_for_scroll_capture(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
	) -> Result<Option<RgbaImage>> {
		self.capture_monitor_region(monitor, rect_px).map(Some)
	}

	/// Samples the RGB color under a global point on the target monitor.
	fn pixel_rgb_in_monitor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
	) -> Result<Option<Rgb>>;

	/// Samples the live cursor color and optional loupe patch from the target monitor.
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

	/// Hit-tests the topmost window under the given point on the target monitor.
	fn hit_test_window_in_monitor(
		&mut self,
		_monitor: MonitorRect,
		_point: GlobalPoint,
	) -> Result<Option<WindowHit>> {
		Ok(None)
	}

	/// Captures a single window by window identifier when supported.
	fn capture_window(&mut self, _window_id: u32) -> Result<RgbaImage> {
		Err(CaptureBackendError::NotSupported { backend: "capture backend" }.into())
	}

	/// Samples an RGBA patch around a global point on the target monitor.
	fn rgba_patch_in_monitor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		width_px: u32,
		height_px: u32,
	) -> Result<Option<RgbaImage>>;

	/// Refreshes any cached full-monitor frame used for repeated sampling operations.
	fn refresh_monitor_cache(
		&mut self,
		_monitor: MonitorRect,
	) -> Result<Arc<MonitorImageSnapshot>> {
		Err(CaptureBackendError::NotSupported { backend: "capture backend" }.into())
	}

	/// Returns the latest cached full-monitor frame when available.
	fn latest_monitor_cache_snapshot(&self) -> Option<Arc<MonitorImageSnapshot>> {
		None
	}

	/// Refreshes any cached window-list snapshot used for hit testing.
	fn refresh_window_cache(&mut self) -> Result<Arc<WindowListSnapshot>> {
		Err(CaptureBackendError::NotSupported { backend: "capture backend" }.into())
	}

	/// Returns the latest cached window-list snapshot when available.
	fn latest_window_cache_snapshot(&self) -> Option<Arc<WindowListSnapshot>> {
		None
	}
}

#[derive(Debug, Error)]
/// Backend-specific capture errors surfaced through the overlay worker.
pub enum CaptureBackendError {
	#[error("screen capture is not supported on this platform (backend: {backend})")]
	NotSupported { backend: &'static str },

	#[cfg(not(target_os = "macos"))]
	#[error("no monitor matched rect: {monitor:?}")]
	MonitorNotFound { monitor: MonitorRect },

	#[error("no window matched id: {window_id}")]
	WindowNotFound { window_id: u32 },
}

/// No-op backend used by tests and unsupported-code paths.
pub struct StubCaptureBackend {}
impl StubCaptureBackend {
	#[must_use]
	/// Creates a stub backend that reports unsupported operations.
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

	fn capture_monitor_region(
		&mut self,
		_monitor: MonitorRect,
		_rect_px: RectPoints,
	) -> Result<RgbaImage> {
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

	fn capture_window(&mut self, _window_id: u32) -> Result<RgbaImage> {
		Err(CaptureBackendError::NotSupported { backend: "stub" }.into())
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

/// Production backend that captures monitors and windows through the native platform stack.
pub struct XcapCaptureBackend {
	cache: Option<Arc<MonitorImageSnapshot>>,
	cache_ttl: Duration,
	window_cache: Option<Arc<WindowListSnapshot>>,
	window_cache_ttl: Duration,
	#[cfg(target_os = "macos")]
	self_capture_exception_window_ids: Vec<u32>,
	#[cfg(target_os = "macos")]
	live_frame_stream: MacLiveFrameStream,
	#[cfg(target_os = "macos")]
	region_capture_tracker: macos_region_capture::RegionCaptureTracker,
}
impl XcapCaptureBackend {
	#[must_use]
	/// Creates a backend with the default cache and stream timings.
	pub fn new() -> Self {
		Self::with_self_capture_exception_window_ids(Vec::new())
	}

	#[must_use]
	/// Creates a backend with an explicit allowlist of current-process windows that remain capturable.
	pub fn with_self_capture_exception_window_ids(
		self_capture_exception_window_ids: Vec<u32>,
	) -> Self {
		#[cfg(not(target_os = "macos"))]
		let _ = self_capture_exception_window_ids;

		Self {
			cache: None,
			cache_ttl: Duration::from_millis(200),
			window_cache: None,
			window_cache_ttl: Duration::from_millis(250),
			#[cfg(target_os = "macos")]
			self_capture_exception_window_ids: self_capture_exception_window_ids.clone(),
			#[cfg(target_os = "macos")]
			live_frame_stream: MacLiveFrameStream::with_self_capture_exception_window_ids(
				self_capture_exception_window_ids,
			),
			#[cfg(target_os = "macos")]
			region_capture_tracker: macos_region_capture::RegionCaptureTracker::default(),
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

		self.refresh_monitor_cache_impl(monitor)?;

		Ok(())
	}

	fn refresh_monitor_cache_impl(
		&mut self,
		monitor: MonitorRect,
	) -> Result<Arc<MonitorImageSnapshot>> {
		#[cfg(target_os = "macos")]
		if let Some(snapshot) = self.live_frame_stream.latest_rgba_snapshot(monitor) {
			self.cache = Some(snapshot.clone());

			return Ok(snapshot);
		}

		let image = self
			.capture_monitor_image(monitor)
			.wrap_err_with(|| format!("failed to capture monitor for rgb sampling: {monitor:?}"))?;
		let snapshot = Arc::new(MonitorImageSnapshot {
			captured_at: Instant::now(),
			stream_generation: 0,
			monitor,
			image: Arc::new(image),
		});

		self.cache = Some(snapshot.clone());

		Ok(snapshot)
	}

	fn latest_monitor_cache_snapshot_impl(&self) -> Option<Arc<MonitorImageSnapshot>> {
		self.cache.clone()
	}

	#[cfg(target_os = "macos")]
	#[expect(
		deprecated,
		reason = "CoreGraphics monitor capture remains the verified macOS fallback until XY-74/XY-75 replace this path."
	)]
	fn capture_monitor_image(&mut self, monitor: MonitorRect) -> Result<RgbaImage> {
		let cg_image = CGDisplayCreateImage(monitor.id)
			.ok_or_else(|| eyre::eyre!("CGDisplayCreateImage returned null"))?;

		image_capture::rgba_image_from_cg_image_for_display(cg_image.as_ref(), Some(monitor.id))
			.wrap_err_with(|| format!("failed to decode display image for monitor: {monitor:?}"))
	}

	#[cfg(not(target_os = "macos"))]
	fn capture_monitor_image(&mut self, monitor: MonitorRect) -> Result<RgbaImage> {
		image_capture::capture_monitor_image(monitor)
	}

	#[cfg(target_os = "macos")]
	#[expect(
		deprecated,
		reason = "CoreGraphics window capture remains the verified macOS fallback until XY-74/XY-75 replace this path."
	)]
	fn capture_window_image(&mut self, window_id: u32) -> Result<RgbaImage> {
		let cg_rect: CGRect = unsafe { CGRectNull };
		let image_option =
			CGWindowImageOption::BoundsIgnoreFraming | CGWindowImageOption::BestResolution;
		let cg_image = CGWindowListCreateImage(
			cg_rect,
			CGWindowListOption::OptionIncludingWindow,
			window_id as CGWindowID,
			image_option,
		);
		let Some(cg_image) = cg_image.as_deref() else {
			return Err(CaptureBackendError::WindowNotFound { window_id }.into());
		};

		image_capture::rgba_image_from_cg_image_for_display(cg_image, None)
			.wrap_err_with(|| format!("Failed to decode window capture bytes: {window_id}"))
	}

	#[cfg(not(target_os = "macos"))]
	fn capture_window_image(&mut self, window_id: u32) -> Result<RgbaImage> {
		let windows = Window::all().wrap_err("xcap Window::all failed")?;

		for window in windows {
			let id = window.id().wrap_err("Failed to read xcap window id")?;

			if id != window_id {
				continue;
			}

			return window.capture_image().wrap_err("xcap window capture_image failed");
		}

		Err(CaptureBackendError::WindowNotFound { window_id }.into())
	}

	#[cfg(not(target_os = "macos"))]
	fn capture_monitor_region_with_xcap(
		&mut self,
		monitor: MonitorRect,
		x: u32,
		y: u32,
		width: u32,
		height: u32,
	) -> Result<RgbaImage> {
		let xcap_monitor = image_capture::xcap_find_monitor(monitor)?;
		let monitor_width = xcap_monitor.width().wrap_err("Failed to read xcap monitor width")?;
		let monitor_height =
			xcap_monitor.height().wrap_err("Failed to read xcap monitor height")?;
		let width = width.max(1).min(monitor_width.max(1));
		let height = height.max(1).min(monitor_height.max(1));
		let x = x.min(monitor_width.saturating_sub(width));
		let y = y.min(monitor_height.saturating_sub(height));
		let image = xcap_monitor
			.capture_region(x, y, width, height)
			.wrap_err("xcap capture_region failed")?;

		Ok(image)
	}

	#[cfg(not(target_os = "macos"))]
	fn crop_monitor_region_fallback(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
	) -> Result<RgbaImage> {
		let image = self.capture_monitor_image(monitor)?;

		image_capture::crop_monitor_image_region(&image, rect_px)
	}

	#[cfg(target_os = "macos")]
	fn capture_monitor_region_with_system_apis(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
	) -> Result<RgbaImage> {
		self.region_capture_tracker.capture_monitor_region(
			&mut self.live_frame_stream,
			monitor,
			rect_px,
		)
	}

	#[cfg(target_os = "macos")]
	fn capture_monitor_region_with_system_apis_for_scroll_capture(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
	) -> Result<Option<RgbaImage>> {
		macos_region_capture::capture_monitor_region_for_scroll_capture(monitor, rect_px)
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

		self.refresh_window_cache_impl()?;

		Ok(())
	}

	fn refresh_window_cache_impl(&mut self) -> Result<Arc<WindowListSnapshot>> {
		let windows = window_list::collect_window_geometries(
			#[cfg(target_os = "macos")]
			&self.self_capture_exception_window_ids,
		)
		.wrap_err("failed to refresh window cache")?;
		let snapshot = Arc::new(WindowListSnapshot {
			captured_at: Instant::now(),
			windows: Arc::new(windows),
		});

		self.window_cache = Some(snapshot.clone());

		Ok(snapshot)
	}

	fn latest_window_cache_snapshot_impl(&self) -> Option<Arc<WindowListSnapshot>> {
		self.window_cache.clone()
	}
}

impl Default for XcapCaptureBackend {
	fn default() -> Self {
		Self::new()
	}
}

impl CaptureBackend for XcapCaptureBackend {
	fn capture_monitor_region(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
	) -> Result<RgbaImage> {
		let rect_px = image_capture::normalize_capture_rect(rect_px);

		#[cfg(target_os = "macos")]
		{
			self.capture_monitor_region_with_system_apis(monitor, rect_px).wrap_err_with(|| {
				format!(
					"failed to capture monitor region for freeze/export: {monitor:?} rect={rect_px:?}"
				)
			})
		}

		#[cfg(not(target_os = "macos"))]
		// TODO(system-api): replace xcap-based monitor region capture with a native per-platform path.
		if let Ok(image) = self.capture_monitor_region_with_xcap(
			monitor,
			rect_px.x,
			rect_px.y,
			rect_px.width,
			rect_px.height,
		) {
			return Ok(image);
		}

		#[cfg(not(target_os = "macos"))]
		self.crop_monitor_region_fallback(monitor, rect_px).wrap_err_with(|| {
			format!(
				"failed to capture monitor region for freeze/export: {monitor:?} rect={rect_px:?}"
			)
		})
	}

	fn capture_monitor_region_for_scroll_capture(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
	) -> Result<Option<RgbaImage>> {
		let rect_px = image_capture::normalize_capture_rect(rect_px);

		#[cfg(target_os = "macos")]
		{
			self.capture_monitor_region_with_system_apis_for_scroll_capture(monitor, rect_px)
				.wrap_err_with(|| {
					format!(
						"failed to capture fresh monitor region for scroll capture: {monitor:?} rect={rect_px:?}"
					)
				})
		}
		#[cfg(not(target_os = "macos"))]
		{
			self.capture_monitor_region(monitor, rect_px).map(Some)
		}
	}

	fn refresh_monitor_cache(&mut self, monitor: MonitorRect) -> Result<Arc<MonitorImageSnapshot>> {
		self.refresh_monitor_cache_impl(monitor)
	}

	fn latest_monitor_cache_snapshot(&self) -> Option<Arc<MonitorImageSnapshot>> {
		self.latest_monitor_cache_snapshot_impl()
	}

	fn refresh_window_cache(&mut self) -> Result<Arc<WindowListSnapshot>> {
		self.refresh_window_cache_impl()
	}

	fn latest_window_cache_snapshot(&self) -> Option<Arc<WindowListSnapshot>> {
		self.latest_window_cache_snapshot_impl()
	}

	fn hit_test_window_in_monitor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
	) -> Result<Option<WindowHit>> {
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

			return Ok(Some(WindowHit { window_id: geometry.window_id, rect: window_rect }));
		}

		Ok(None)
	}

	fn capture_monitor(&mut self, monitor: MonitorRect) -> Result<RgbaImage> {
		let image = self.capture_monitor_image(monitor).wrap_err_with(|| {
			format!("failed to capture monitor for freeze/export: {monitor:?}")
		})?;

		self.cache = Some(Arc::new(MonitorImageSnapshot {
			captured_at: Instant::now(),
			stream_generation: 0,
			monitor,
			image: Arc::new(image.clone()),
		}));

		Ok(image)
	}

	fn capture_window(&mut self, window_id: u32) -> Result<RgbaImage> {
		self.capture_window_image(window_id)
			.wrap_err_with(|| format!("failed to capture window for freeze/export: {window_id}"))
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

		let Some((x, y)) = monitor.local_u32_pixels(point) else {
			return Ok(None);
		};
		let patch = {
			#[cfg(target_os = "macos")]
			{
				if let Ok(patch) = self.capture_monitor_region(monitor, RectPoints::new(x, y, 1, 1))
				{
					patch
				} else {
					self.ensure_cache(monitor)?;

					let Some(cache) = self.cache.as_ref() else {
						return Ok(None);
					};

					image_capture::copy_rgba_patch(&cache.image, x, y, 1, 1)
				}
			}
			#[cfg(not(target_os = "macos"))]
			{
				self.ensure_cache(monitor)?;

				let Some(cache) = self.cache.as_ref() else {
					return Ok(None);
				};

				image_capture::copy_rgba_patch(&cache.image, x, y, 1, 1)
			}
		};
		let Some(pixel) = patch.get_pixel_checked(0, 0) else {
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
			let Some((x_px, y_px)) = monitor.local_u32_pixels(point) else {
				return Ok(LiveCursorSample { rgb: None, patch: None });
			};
			let sample = self
				.live_frame_stream
				.latest_cursor_sample(
					monitor,
					CursorSampleRequest::with_optional_patch(
						x_px,
						y_px,
						want_patch,
						patch_width_px,
						patch_height_px,
					),
				)
				.unwrap_or(LiveCursorSample { rgb: None, patch: None });

			Ok(sample)
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

		let Some((center_x, center_y)) = monitor.local_u32_pixels(point) else {
			return Ok(None);
		};
		let patch = {
			#[cfg(target_os = "macos")]
			{
				let monitor_width = image_capture::point_extent_to_pixel_extent(
					monitor.width,
					monitor.scale_factor(),
				);
				let monitor_height = image_capture::point_extent_to_pixel_extent(
					monitor.height,
					monitor.scale_factor(),
				);
				let width = width_px.max(1).min(monitor_width.max(1));
				let height = height_px.max(1).min(monitor_height.max(1));
				let region_x =
					center_x.saturating_sub(width / 2).min(monitor_width.saturating_sub(width));
				let region_y =
					center_y.saturating_sub(height / 2).min(monitor_height.saturating_sub(height));
				let rect_px = RectPoints::new(region_x, region_y, width, height);

				match image_capture::capture_monitor_region_with_core_graphics(monitor, rect_px) {
					Ok(patch) => patch,
					Err(_) => {
						self.ensure_cache(monitor)?;

						let Some(cache) = self.cache.as_ref() else {
							return Ok(None);
						};

						image_capture::copy_rgba_patch(
							&cache.image,
							center_x,
							center_y,
							width,
							height,
						)
					},
				}
			}
			#[cfg(not(target_os = "macos"))]
			{
				self.ensure_cache(monitor)?;

				let Some(cache) = self.cache.as_ref() else {
					return Ok(None);
				};

				image_capture::copy_rgba_patch(
					&cache.image,
					center_x,
					center_y,
					width_px,
					height_px,
				)
			}
		};

		Ok(Some(patch))
	}
}

#[must_use]
/// Builds the default capture backend used by overlay worker threads.
pub fn default_capture_backend() -> Box<dyn CaptureBackend> {
	Box::new(XcapCaptureBackend::new())
}

#[must_use]
/// Builds the default capture backend with explicit current-process self-capture exceptions.
pub fn default_capture_backend_with_self_capture_exception_window_ids(
	self_capture_exception_window_ids: Vec<u32>,
) -> Box<dyn CaptureBackend> {
	Box::new(XcapCaptureBackend::with_self_capture_exception_window_ids(
		self_capture_exception_window_ids,
	))
}

#[cfg(test)]
mod tests {
	use crate::backend::{CaptureBackend, StubCaptureBackend};

	#[test]
	fn stub_backend_returns_cursor_position() {
		let mut backend = StubCaptureBackend::new();
		let pos = backend.global_cursor_position().unwrap();

		assert!(pos.is_none());
	}
}
