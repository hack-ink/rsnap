//! Small pixel-space point type for transition rendering helpers.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PixelPoint {
	pub(crate) x: f32,
	pub(crate) y: f32,
}
impl PixelPoint {
	pub(crate) const fn new(x: f32, y: f32) -> Self {
		Self { x, y }
	}

	pub(crate) fn distance(self, other: Self) -> f32 {
		(self.x - other.x).hypot(self.y - other.y)
	}
}
