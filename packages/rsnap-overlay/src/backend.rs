use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr};
use device_query::{DeviceQuery, DeviceState};
use image::RgbaImage;
use thiserror::Error;

use crate::state::{GlobalPoint, MonitorRect, Rgb};

pub trait CaptureBackend: Send {
	fn global_cursor_position(&mut self) -> Result<Option<GlobalPoint>>;
	fn capture_monitor(&mut self, monitor: MonitorRect) -> Result<RgbaImage>;
	fn pixel_rgb_in_monitor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
	) -> Result<Option<Rgb>>;
}

#[derive(Debug, Error)]
pub enum CaptureBackendError {
	#[error("screen capture is not supported on this platform (backend: {backend})")]
	NotSupported { backend: &'static str },

	#[error("no monitor matched rect: {monitor:?}")]
	MonitorNotFound { monitor: MonitorRect },
}

pub struct StubCaptureBackend {
	device_state: DeviceState,
}
impl StubCaptureBackend {
	#[must_use]
	pub fn new() -> Self {
		Self { device_state: DeviceState::new() }
	}
}

impl Default for StubCaptureBackend {
	fn default() -> Self {
		Self::new()
	}
}

impl CaptureBackend for StubCaptureBackend {
	fn global_cursor_position(&mut self) -> Result<Option<GlobalPoint>> {
		let mouse = self.device_state.get_mouse();

		Ok(Some(GlobalPoint::new(mouse.coords.0, mouse.coords.1)))
	}

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
}

pub struct XcapCaptureBackend {
	device_state: DeviceState,
	cache: Option<CaptureCache>,
	cache_ttl: Duration,
}
impl XcapCaptureBackend {
	#[must_use]
	pub fn new() -> Self {
		Self { device_state: DeviceState::new(), cache: None, cache_ttl: Duration::from_millis(80) }
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

		let image = capture_monitor_image(monitor)
			.wrap_err_with(|| format!("failed to capture monitor for rgb sampling: {monitor:?}"))?;

		self.cache = Some(CaptureCache { monitor, captured_at: Instant::now(), image });

		Ok(())
	}
}

impl Default for XcapCaptureBackend {
	fn default() -> Self {
		Self::new()
	}
}

impl CaptureBackend for XcapCaptureBackend {
	fn global_cursor_position(&mut self) -> Result<Option<GlobalPoint>> {
		let mouse = self.device_state.get_mouse();

		Ok(Some(GlobalPoint::new(mouse.coords.0, mouse.coords.1)))
	}

	fn capture_monitor(&mut self, monitor: MonitorRect) -> Result<RgbaImage> {
		let image =
			capture_monitor_image(monitor).wrap_err("failed to capture monitor screenshot")?;

		self.cache =
			Some(CaptureCache { monitor, captured_at: Instant::now(), image: image.clone() });

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

		self.ensure_cache(monitor)?;

		let Some(cache) = &self.cache else {
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
}

#[derive(Debug)]
struct CaptureCache {
	monitor: MonitorRect,
	captured_at: Instant,
	image: RgbaImage,
}

#[must_use]
pub fn default_capture_backend() -> Box<dyn CaptureBackend> {
	Box::new(XcapCaptureBackend::new())
}

fn capture_monitor_image(monitor: MonitorRect) -> Result<RgbaImage> {
	let xcap_monitor = xcap_find_monitor(monitor)?;
	let image = xcap_monitor.capture_image().wrap_err("xcap capture_image failed")?;

	Ok(image)
}

fn xcap_find_monitor(monitor: MonitorRect) -> Result<xcap::Monitor> {
	let monitors = xcap::Monitor::all().wrap_err("xcap Monitor::all failed")?;

	for m in monitors {
		if m.id() == monitor.id {
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

		assert!(pos.is_some());
	}
}
