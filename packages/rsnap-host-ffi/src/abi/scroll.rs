use crate::abi::RsnapFloatRect;

/// FFI-safe scroll-capture minimap layout plan.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RsnapScrollMinimapPlan {
	/// Outer minimap frame.
	pub frame: RsnapFloatRect,
	/// Preview image frame inside `frame`.
	pub image_frame: RsnapFloatRect,
	/// Non-zero when `viewport_frame` contains a visible marker.
	pub has_viewport_frame: u8,
	/// Viewport marker frame inside `image_frame`.
	pub viewport_frame: RsnapFloatRect,
}

/// FFI-safe scroll-capture observation discriminator.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RsnapScrollObserveOutcomeKind {
	/// The candidate did not change committed output.
	NoChange = 0,
	/// Preview-only state changed.
	PreviewUpdated = 1,
	/// Downward growth was committed.
	Committed = 2,
	/// The candidate proved motion in a direction not appended by this wrapper.
	UnsupportedDirection = 3,
}

/// FFI-safe result for one scroll-capture frame observation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapScrollObserveResult {
	/// Observation outcome.
	pub kind: u32,
	/// Appended row count when `kind` is committed.
	pub growth_rows: u32,
	/// Current committed export width in pixels.
	pub export_width: u32,
	/// Current committed export height in pixels.
	pub export_height: u32,
	/// Current committed viewport top in pixels.
	pub current_viewport_top_y: i32,
}
impl Default for RsnapScrollObserveResult {
	fn default() -> Self {
		Self {
			kind: RsnapScrollObserveOutcomeKind::NoChange as u32,
			growth_rows: 0,
			export_width: 0,
			export_height: 0,
			current_viewport_top_y: 0,
		}
	}
}
