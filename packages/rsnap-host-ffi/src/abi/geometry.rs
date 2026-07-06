use std::ptr;

/// FFI-safe owned RGBA image region whose buffer is retained by Rust until explicitly freed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapOwnedRgbaRegion {
	/// Region width in pixels.
	pub width: u32,
	/// Region height in pixels.
	pub height: u32,
	/// Byte count in `rgba`.
	pub len: usize,
	/// Reserved buffer capacity in bytes.
	pub capacity: usize,
	/// Owned RGBA byte buffer in row-major order.
	pub rgba: *mut u8,
}
impl Default for RsnapOwnedRgbaRegion {
	fn default() -> Self {
		Self { width: 0, height: 0, len: 0, capacity: 0, rgba: ptr::null_mut() }
	}
}

/// FFI-safe owned byte buffer retained by Rust until explicitly freed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapOwnedBytes {
	/// Byte count in `bytes`.
	pub len: usize,
	/// Reserved buffer capacity in bytes.
	pub capacity: usize,
	/// Owned byte buffer.
	pub bytes: *mut u8,
}
impl Default for RsnapOwnedBytes {
	fn default() -> Self {
		Self { len: 0, capacity: 0, bytes: ptr::null_mut() }
	}
}

/// FFI-safe pixel-space rectangle.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapPixelRect {
	/// Left coordinate in pixels.
	pub x: u32,
	/// Top coordinate in pixels.
	pub y: u32,
	/// Rectangle width in pixels.
	pub width: u32,
	/// Rectangle height in pixels.
	pub height: u32,
}

/// FFI-safe display-space rectangle.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RsnapFloatRect {
	/// Left coordinate in display points.
	pub x: f64,
	/// Top coordinate in display points.
	pub y: f64,
	/// Rectangle width in display points.
	pub width: f64,
	/// Rectangle height in display points.
	pub height: f64,
}

/// FFI-safe display-space point.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RsnapFloatPoint {
	/// X coordinate in display points.
	pub x: f64,
	/// Y coordinate in display points.
	pub y: f64,
}

/// FFI-safe global point.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapPoint {
	/// Global X coordinate.
	pub x: i32,
	/// Global Y coordinate.
	pub y: i32,
}

/// FFI-safe RGB sample.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapRgb {
	/// Red channel.
	pub r: u8,
	/// Green channel.
	pub g: u8,
	/// Blue channel.
	pub b: u8,
}

/// FFI-safe global rectangle.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapRect {
	/// Global left coordinate.
	pub x: i32,
	/// Global top coordinate.
	pub y: i32,
	/// Rectangle width.
	pub width: u32,
	/// Rectangle height.
	pub height: u32,
}

/// FFI-safe monitor rectangle snapshot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapMonitorRect {
	/// Stable monitor identifier.
	pub id: u32,
	/// Monitor origin in global points.
	pub origin: RsnapPoint,
	/// Monitor width in points.
	pub width: u32,
	/// Monitor height in points.
	pub height: u32,
	/// Monitor pixel scale factor in thousandths.
	pub scale_factor_x1000: u32,
}

/// FFI-safe highlighted window snapshot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RsnapWindowRect {
	/// Window identifier when one exists.
	pub window_id: u32,
	/// Non-zero when `window_id` is present.
	pub has_window_id: u8,
	/// Global left coordinate.
	pub x: i64,
	/// Global top coordinate.
	pub y: i64,
	/// Window width in points.
	pub width: i64,
	/// Window height in points.
	pub height: i64,
}
