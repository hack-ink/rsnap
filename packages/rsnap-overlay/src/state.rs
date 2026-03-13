use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;

#[derive(Debug)]
pub(crate) struct LoupeSample {
	pub center: GlobalPoint,
	pub patch: RgbaImage,
}

#[derive(Debug)]
/// Cached full-monitor frame used for RGB and loupe sampling.
pub struct MonitorImageSnapshot {
	/// When the frame was captured.
	pub captured_at: Instant,
	/// The monitor that produced this frame.
	pub monitor: MonitorRect,
	/// The captured monitor image in RGBA pixel format.
	pub image: Arc<RgbaImage>,
}

#[derive(Debug)]
/// Combined live cursor sample containing the current RGB and optional loupe patch.
pub struct LiveCursorSample {
	/// The sampled RGB value under the cursor when available.
	pub rgb: Option<Rgb>,
	/// The sampled loupe patch when requested and available.
	pub patch: Option<RgbaImage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Window bounds expressed in global point coordinates.
pub struct WindowRect {
	/// The source window identifier when one exists.
	pub window_id: Option<u32>,
	/// Global left coordinate in points.
	pub x: i64,
	/// Global top coordinate in points.
	pub y: i64,
	/// Window width in points.
	pub width: i64,
	/// Window height in points.
	pub height: i64,
}

#[derive(Debug)]
/// Cached window-list snapshot used for live hit testing.
pub struct WindowListSnapshot {
	/// When the snapshot was captured.
	pub captured_at: Instant,
	/// Windows ordered for hit testing.
	pub windows: Arc<Vec<WindowRect>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Result of hit testing a point against a window.
pub struct WindowHit {
	/// The source window identifier when one exists.
	pub window_id: Option<u32>,
	/// Monitor-local rectangle for the hit window.
	pub rect: RectPoints,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Rectangle in monitor-local point or pixel coordinates, depending on context.
pub struct RectPoints {
	/// Left coordinate.
	pub x: u32,
	/// Top coordinate.
	pub y: u32,
	/// Rectangle width.
	pub width: u32,
	/// Rectangle height.
	pub height: u32,
}
impl RectPoints {
	#[must_use]
	/// Creates a rectangle from origin and size components.
	pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
		Self { x, y, width, height }
	}

	#[must_use]
	/// Returns `true` when either rectangle dimension is zero.
	pub fn is_empty(&self) -> bool {
		self.width == 0 || self.height == 0
	}

	#[must_use]
	/// Returns `true` when the point lies inside the rectangle bounds.
	pub fn contains(&self, point: (u32, u32)) -> bool {
		point.0 >= self.x
			&& point.1 >= self.y
			&& point.0 < self.x.saturating_add(self.width)
			&& point.1 < self.y.saturating_add(self.height)
	}

	#[must_use]
	/// Scales the rectangle by the provided monitor scale factor.
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
/// Global point in desktop coordinate space.
pub struct GlobalPoint {
	/// Global X coordinate.
	pub x: i32,
	/// Global Y coordinate.
	pub y: i32,
}
impl GlobalPoint {
	#[must_use]
	/// Creates a new global point.
	pub fn new(x: i32, y: i32) -> Self {
		Self { x, y }
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// RGB color sample without alpha.
pub struct Rgb {
	/// Red channel.
	pub r: u8,
	/// Green channel.
	pub g: u8,
	/// Blue channel.
	pub b: u8,
}
impl Rgb {
	#[must_use]
	/// Creates a new RGB sample from channel values.
	pub fn new(r: u8, g: u8, b: u8) -> Self {
		Self { r, g, b }
	}

	#[must_use]
	/// Formats the RGB color as an uppercase `#RRGGBB` string.
	pub fn hex_upper(self) -> String {
		format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Monitor bounds and scale factor in global desktop space.
pub struct MonitorRect {
	/// Stable monitor identifier used by the capture stack.
	pub id: u32,
	/// Monitor origin in global points.
	pub origin: GlobalPoint,
	/// Monitor width in points.
	pub width: u32,
	/// Monitor height in points.
	pub height: u32,
	/// Monitor pixel scale factor in thousandths (e.g. 1.0 -> 1000, 2.0 -> 2000).
	pub scale_factor_x1000: u32,
}
impl MonitorRect {
	#[must_use]
	/// Returns the floating-point scale factor derived from `scale_factor_x1000`.
	pub fn scale_factor(&self) -> f32 {
		(self.scale_factor_x1000 as f32) / 1_000.0
	}

	#[must_use]
	/// Returns `true` when the global point lies inside the monitor bounds.
	pub fn contains(&self, point: GlobalPoint) -> bool {
		let x_ok =
			point.x >= self.origin.x && point.x < self.origin.x.saturating_add_unsigned(self.width);
		let y_ok = point.y >= self.origin.y
			&& point.y < self.origin.y.saturating_add_unsigned(self.height);

		x_ok && y_ok
	}

	#[must_use]
	/// Converts a global point into monitor-local point coordinates.
	pub fn local_u32(&self, point: GlobalPoint) -> Option<(u32, u32)> {
		if !self.contains(point) {
			return None;
		}

		let local_x = point.x.saturating_sub(self.origin.x) as u32;
		let local_y = point.y.saturating_sub(self.origin.y) as u32;

		Some((local_x, local_y))
	}

	#[must_use]
	/// Converts a global point into monitor-local pixel coordinates.
	pub fn local_u32_pixels(&self, point: GlobalPoint) -> Option<(u32, u32)> {
		let (local_x, local_y) = self.local_u32(point)?;
		let sf = self.scale_factor();
		let px = ((local_x as f32) * sf).round() as u32;
		let py = ((local_y as f32) * sf).round() as u32;

		Some((px, py))
	}

	#[must_use]
	/// Clips a global rectangle expressed as `i64` bounds into monitor-local coordinates.
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
	/// Clips a global rectangle expressed as `i32` bounds into monitor-local coordinates.
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
	/// Builds a clipped monitor-local rectangle from two global corner points.
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
	/// Converts a monitor-local point rectangle into pixel coordinates.
	pub fn local_rect_to_pixels(&self, rect: RectPoints) -> RectPoints {
		rect.scaled(self.scale_factor())
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Associates a monitor identifier with a monitor-local rectangle.
pub struct MonitorRectPoints {
	/// The monitor that owns the rectangle.
	pub monitor_id: u32,
	/// The rectangle expressed in that monitor's local coordinates.
	pub rect: RectPoints,
}

#[derive(Clone, Copy, Debug)]
/// Internal overlay runtime mode.
pub enum OverlayMode {
	Live,
	Frozen,
}

#[derive(Debug)]
/// Internal mutable state owned by a running overlay session.
pub struct OverlayState {
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

	pub fn reset_for_start(&mut self, loupe_patch_side_px: u32) {
		*self = Self::new();
		self.loupe_patch_side_px = loupe_patch_side_px;
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
