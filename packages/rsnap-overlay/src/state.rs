use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;

#[derive(Debug)]
pub struct LoupeSample {
	pub center: GlobalPoint,
	pub patch: RgbaImage,
}

#[derive(Debug)]
pub struct MonitorImageSnapshot {
	pub captured_at: Instant,
	pub monitor: MonitorRect,
	pub image: Arc<RgbaImage>,
}

#[derive(Debug)]
pub struct LiveCursorSample {
	pub rgb: Option<Rgb>,
	pub patch: Option<RgbaImage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowRect {
	pub x: i64,
	pub y: i64,
	pub width: i64,
	pub height: i64,
}

#[derive(Debug)]
pub struct WindowListSnapshot {
	pub captured_at: Instant,
	pub windows: Arc<Vec<WindowRect>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RectPoints {
	pub x: u32,
	pub y: u32,
	pub width: u32,
	pub height: u32,
}
impl RectPoints {
	#[must_use]
	pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
		Self { x, y, width, height }
	}

	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.width == 0 || self.height == 0
	}

	#[must_use]
	pub fn contains(&self, point: (u32, u32)) -> bool {
		point.0 >= self.x
			&& point.1 >= self.y
			&& point.0 < self.x.saturating_add(self.width)
			&& point.1 < self.y.saturating_add(self.height)
	}

	#[must_use]
	pub fn scaled(self, scale_factor: f32) -> Self {
		Self {
			x: (self.x as f32 * scale_factor).round() as u32,
			y: (self.y as f32 * scale_factor).round() as u32,
			width: (self.width as f32 * scale_factor).round() as u32,
			height: (self.height as f32 * scale_factor).round() as u32,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonitorRectPoints {
	pub monitor_id: u32,
	pub rect: RectPoints,
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

	#[must_use]
	pub fn clip_global_rect_i64(
		&self,
		left: i64,
		top: i64,
		right: i64,
		bottom: i64,
	) -> Option<RectPoints> {
		let monitor_left = i64::from(self.origin.x);
		let monitor_top = i64::from(self.origin.y);
		let monitor_right = monitor_left.saturating_add(i64::from(self.width));
		let monitor_bottom = monitor_top.saturating_add(i64::from(self.height));
		let clipped_left = left.max(monitor_left);
		let clipped_top = top.max(monitor_top);
		let clipped_right = right.min(monitor_right);
		let clipped_bottom = bottom.min(monitor_bottom);

		if clipped_left >= clipped_right || clipped_top >= clipped_bottom {
			return None;
		}

		let rect = RectPoints::new(
			u32::try_from(clipped_left - monitor_left).ok()?,
			u32::try_from(clipped_top - monitor_top).ok()?,
			u32::try_from(clipped_right - clipped_left).ok()?,
			u32::try_from(clipped_bottom - clipped_top).ok()?,
		);

		if rect.is_empty() {
			return None;
		}

		Some(rect)
	}

	#[must_use]
	pub fn clip_global_rect(
		&self,
		left: i32,
		top: i32,
		right: i32,
		bottom: i32,
	) -> Option<RectPoints> {
		self.clip_global_rect_i64(
			i64::from(left),
			i64::from(top),
			i64::from(right),
			i64::from(bottom),
		)
	}

	#[must_use]
	pub fn local_rect_from_points(
		&self,
		first: GlobalPoint,
		second: GlobalPoint,
	) -> Option<RectPoints> {
		let left = first.x.min(second.x);
		let top = first.y.min(second.y);
		let right = first.x.max(second.x);
		let bottom = first.y.max(second.y);

		self.clip_global_rect(left, top, right, bottom)
	}

	#[must_use]
	pub fn local_rect_to_pixels(&self, rect: RectPoints) -> RectPoints {
		rect.scaled(self.scale_factor())
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
	pub hovered_window_rect: Option<MonitorRectPoints>,
	pub drag_rect: Option<MonitorRectPoints>,
	pub frozen_capture_rect: Option<RectPoints>,
	pub live_bg_monitor: Option<MonitorRect>,
	pub live_bg_image: Option<RgbaImage>,
	pub live_bg_generation: u64,
	pub frozen_image: Option<RgbaImage>,
	pub frozen_generation: u64,
	pub error_message: Option<String>,
	pub alt_held: bool,
	pub loupe: Option<LoupeSample>,
	pub loupe_patch_side_px: u32,
}
impl OverlayState {
	pub fn new() -> Self {
		Self {
			mode: OverlayMode::Live,
			cursor: None,
			rgb: None,
			monitor: None,
			hovered_window_rect: None,
			drag_rect: None,
			frozen_capture_rect: None,
			live_bg_monitor: None,
			live_bg_image: None,
			live_bg_generation: 0,
			frozen_image: None,
			frozen_generation: 0,
			error_message: None,
			alt_held: false,
			loupe: None,
			loupe_patch_side_px: 21,
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
	use crate::state::{GlobalPoint, MonitorRect, RectPoints};

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

	#[test]
	fn local_rect_and_pixels() {
		let monitor = MonitorRect {
			id: 0,
			origin: GlobalPoint::new(-100, -100),
			width: 300,
			height: 200,
			scale_factor_x1000: 2_000,
		};
		let rect = monitor.clip_global_rect(-90, -80, 40, 50).expect("clipped local rect");

		assert_eq!(rect, RectPoints::new(10, 20, 130, 130));
		assert!(rect.contains((20, 30)));

		let pixel_rect = monitor.local_rect_to_pixels(rect);

		assert_eq!(pixel_rect, RectPoints::new(20, 40, 260, 260));
	}
}
