//! Shared geometric and sampling data types for the Rust product core.

use serde::{Deserialize, Serialize};

/// Global point in desktop coordinate space.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct GlobalPoint {
	/// Global X coordinate.
	pub x: i32,
	/// Global Y coordinate.
	pub y: i32,
}
impl GlobalPoint {
	/// Creates a new global point.
	#[must_use]
	pub const fn new(x: i32, y: i32) -> Self {
		Self { x, y }
	}
}

/// Rectangle in global desktop coordinate space.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct GlobalRect {
	/// Global left coordinate.
	pub x: i32,
	/// Global top coordinate.
	pub y: i32,
	/// Rectangle width in points.
	pub width: u32,
	/// Rectangle height in points.
	pub height: u32,
}
impl GlobalRect {
	/// Creates a new global rectangle.
	#[must_use]
	pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
		Self { x, y, width, height }
	}

	/// Returns `true` when either rectangle dimension is zero.
	#[must_use]
	pub const fn is_empty(&self) -> bool {
		self.width == 0 || self.height == 0
	}

	/// Returns `true` when the point lies inside the rectangle bounds.
	#[must_use]
	pub fn contains(&self, point: GlobalPoint) -> bool {
		point.x >= self.x
			&& point.x < self.x.saturating_add_unsigned(self.width)
			&& point.y >= self.y
			&& point.y < self.y.saturating_add_unsigned(self.height)
	}
}

/// Rectangle in monitor-local point or pixel coordinates, depending on context.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
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
	/// Creates a rectangle from origin and size components.
	#[must_use]
	pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
		Self { x, y, width, height }
	}

	/// Returns `true` when either rectangle dimension is zero.
	#[must_use]
	pub const fn is_empty(&self) -> bool {
		self.width == 0 || self.height == 0
	}

	/// Returns `true` when the point lies inside the rectangle bounds.
	#[must_use]
	pub fn contains(&self, point: (u32, u32)) -> bool {
		point.0 >= self.x
			&& point.1 >= self.y
			&& point.0 < self.x.saturating_add(self.width)
			&& point.1 < self.y.saturating_add(self.height)
	}

	/// Scales the rectangle by the provided monitor scale factor.
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

/// RGB color sample without alpha.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Rgb {
	/// Red channel.
	pub r: u8,
	/// Green channel.
	pub g: u8,
	/// Blue channel.
	pub b: u8,
}
impl Rgb {
	/// Creates a new RGB sample from channel values.
	#[must_use]
	pub const fn new(r: u8, g: u8, b: u8) -> Self {
		Self { r, g, b }
	}

	/// Formats the RGB color as an uppercase `#RRGGBB` string.
	#[must_use]
	pub fn hex_upper(self) -> String {
		format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
	}
}

/// Monitor bounds and scale factor in global desktop space.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
	/// Returns the floating-point scale factor derived from `scale_factor_x1000`.
	#[must_use]
	pub fn scale_factor(&self) -> f32 {
		(self.scale_factor_x1000 as f32) / 1_000.0
	}

	/// Returns `true` when the global point lies inside the monitor bounds.
	#[must_use]
	pub fn contains(&self, point: GlobalPoint) -> bool {
		let x_ok =
			point.x >= self.origin.x && point.x < self.origin.x.saturating_add_unsigned(self.width);
		let y_ok = point.y >= self.origin.y
			&& point.y < self.origin.y.saturating_add_unsigned(self.height);

		x_ok && y_ok
	}

	/// Converts a global point into monitor-local point coordinates.
	#[must_use]
	pub fn local_u32(&self, point: GlobalPoint) -> Option<(u32, u32)> {
		if !self.contains(point) {
			return None;
		}

		let local_x = point.x.saturating_sub(self.origin.x) as u32;
		let local_y = point.y.saturating_sub(self.origin.y) as u32;

		Some((local_x, local_y))
	}

	/// Converts a global point into monitor-local pixel coordinates.
	#[must_use]
	pub fn local_u32_pixels(&self, point: GlobalPoint) -> Option<(u32, u32)> {
		let (local_x, local_y) = self.local_u32(point)?;
		let scale_factor = self.scale_factor();
		let px = ((local_x as f32) * scale_factor).round() as u32;
		let py = ((local_y as f32) * scale_factor).round() as u32;

		Some((px, py))
	}

	/// Clips a global rectangle expressed as `i64` bounds into monitor-local coordinates.
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

	/// Clips a global rectangle expressed as `i32` bounds into monitor-local coordinates.
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

	/// Builds a clipped monitor-local rectangle from two global corner points.
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

	/// Converts a monitor-local point rectangle into pixel coordinates.
	#[must_use]
	pub fn local_rect_to_pixels(&self, rect: RectPoints) -> RectPoints {
		rect.scaled(self.scale_factor())
	}
}

/// Associates a monitor identifier with a monitor-local rectangle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MonitorRectPoints {
	/// The monitor that owns the rectangle.
	pub monitor_id: u32,
	/// The rectangle expressed in that monitor's local coordinates.
	pub rect: RectPoints,
}

/// Window bounds expressed in global point coordinates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
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
impl WindowRect {
	/// Converts the window bounds into a global rectangle when the size is valid.
	#[must_use]
	pub fn global_rect(self) -> Option<GlobalRect> {
		Some(GlobalRect::new(
			i32::try_from(self.x).ok()?,
			i32::try_from(self.y).ok()?,
			u32::try_from(self.width).ok()?,
			u32::try_from(self.height).ok()?,
		))
	}
}

/// Result of hit testing a point against a window.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WindowHit {
	/// The source window identifier when one exists.
	pub window_id: Option<u32>,
	/// Monitor-local rectangle for the hit window.
	pub rect: RectPoints,
}

#[cfg(test)]
mod tests {
	use super::{GlobalPoint, GlobalRect, MonitorRect, RectPoints, Rgb, WindowRect};

	#[test]
	fn global_rect_contains_point() {
		let rect = GlobalRect::new(10, 20, 30, 40);

		assert!(rect.contains(GlobalPoint::new(10, 20)));
		assert!(rect.contains(GlobalPoint::new(39, 59)));
		assert!(!rect.contains(GlobalPoint::new(40, 59)));
		assert!(!rect.contains(GlobalPoint::new(39, 60)));
	}

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

	#[test]
	fn rgb_formats_hex_uppercase() {
		assert_eq!(Rgb::new(0x12, 0x34, 0xab).hex_upper(), "#1234AB");
	}

	#[test]
	fn window_rect_maps_to_global_rect() {
		let rect = WindowRect { window_id: Some(7), x: 40, y: 50, width: 120, height: 60 };

		assert_eq!(rect.global_rect(), Some(GlobalRect::new(40, 50, 120, 60)));
	}
}
