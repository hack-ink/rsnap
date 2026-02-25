use image::RgbaImage;

#[derive(Debug)]
pub struct LoupeSample {
	pub center: GlobalPoint,
	pub patch: RgbaImage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlobalPoint {
	pub x: i32,
	pub y: i32,
}
impl GlobalPoint {
	#[must_use]
	pub fn new(x: i32, y: i32) -> Self {
		Self { x, y }
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb {
	pub r: u8,
	pub g: u8,
	pub b: u8,
}
impl Rgb {
	#[must_use]
	pub fn new(r: u8, g: u8, b: u8) -> Self {
		Self { r, g, b }
	}

	#[must_use]
	pub fn hex_upper(self) -> String {
		format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonitorRect {
	pub id: u32,
	pub origin: GlobalPoint,
	pub width: u32,
	pub height: u32,
	/// Monitor pixel scale factor in thousandths (e.g. 1.0 -> 1000, 2.0 -> 2000).
	pub scale_factor_x1000: u32,
}
impl MonitorRect {
	#[must_use]
	pub fn scale_factor(&self) -> f32 {
		(self.scale_factor_x1000 as f32) / 1_000.0
	}

	#[must_use]
	pub fn contains(&self, point: GlobalPoint) -> bool {
		let x_ok =
			point.x >= self.origin.x && point.x < self.origin.x.saturating_add_unsigned(self.width);
		let y_ok = point.y >= self.origin.y
			&& point.y < self.origin.y.saturating_add_unsigned(self.height);

		x_ok && y_ok
	}

	#[must_use]
	pub fn local_u32(&self, point: GlobalPoint) -> Option<(u32, u32)> {
		if !self.contains(point) {
			return None;
		}

		let local_x = point.x.saturating_sub(self.origin.x) as u32;
		let local_y = point.y.saturating_sub(self.origin.y) as u32;

		Some((local_x, local_y))
	}

	#[must_use]
	pub fn local_u32_pixels(&self, point: GlobalPoint) -> Option<(u32, u32)> {
		let (local_x, local_y) = self.local_u32(point)?;
		let sf = self.scale_factor();
		let px = ((local_x as f32) * sf).round() as u32;
		let py = ((local_y as f32) * sf).round() as u32;

		Some((px, py))
	}
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum OverlayMode {
	Live,
	Frozen,
}

#[derive(Debug)]
pub(crate) struct OverlayState {
	pub mode: OverlayMode,
	pub cursor: Option<GlobalPoint>,
	pub rgb: Option<Rgb>,
	pub monitor: Option<MonitorRect>,
	pub frozen_image: Option<RgbaImage>,
	pub frozen_generation: u64,
	pub error_message: Option<String>,
	pub alt_held: bool,
	pub loupe: Option<LoupeSample>,
}
impl OverlayState {
	pub fn new() -> Self {
		Self {
			mode: OverlayMode::Live,
			cursor: None,
			rgb: None,
			monitor: None,
			frozen_image: None,
			frozen_generation: 0,
			error_message: None,
			alt_held: false,
			loupe: None,
		}
	}

	pub fn set_error(&mut self, message: impl Into<String>) {
		self.error_message = Some(message.into());
	}

	pub fn clear_error(&mut self) {
		self.error_message = None;
	}

	pub fn begin_freeze(&mut self, monitor: MonitorRect) {
		self.monitor = Some(monitor);
		self.frozen_image = None;
		self.loupe = None;
		self.mode = OverlayMode::Frozen;
		self.frozen_generation = self.frozen_generation.wrapping_add(1);
	}

	pub fn finish_freeze(&mut self, monitor: MonitorRect, image: RgbaImage) {
		// Keep the existing generation set by `begin_freeze` so renderers can key off a single
		// freeze request/response cycle.
		self.monitor = Some(monitor);
		self.frozen_image = Some(image);
		self.mode = OverlayMode::Frozen;
	}

	#[allow(dead_code)]
	pub fn unfreeze_to_live(&mut self) {
		self.mode = OverlayMode::Live;
		self.frozen_image = None;
		self.loupe = None;
		self.frozen_generation = self.frozen_generation.wrapping_add(1);
	}
}

#[cfg(test)]
mod tests {
	use crate::state::{GlobalPoint, MonitorRect};

	#[test]
	fn monitor_contains_and_local_coords() {
		let monitor = MonitorRect {
			id: 0,
			origin: GlobalPoint::new(-100, 50),
			width: 200,
			height: 100,
			scale_factor_x1000: 1_000,
		};

		assert!(monitor.contains(GlobalPoint::new(-100, 50)));
		assert!(monitor.contains(GlobalPoint::new(99, 149)));
		assert!(!monitor.contains(GlobalPoint::new(100, 149)));
		assert!(!monitor.contains(GlobalPoint::new(99, 150)));
		assert_eq!(monitor.local_u32(GlobalPoint::new(-100, 50)), Some((0, 0)));
		assert_eq!(monitor.local_u32(GlobalPoint::new(-1, 51)), Some((99, 1)));
		assert_eq!(monitor.local_u32(GlobalPoint::new(100, 50)), None);
	}
}
