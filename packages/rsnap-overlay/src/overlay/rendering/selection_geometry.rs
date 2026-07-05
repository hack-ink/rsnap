use crate::overlay::{Pos2, Rect, RectPoints, Vec2};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::overlay) struct SelectionFlowGeometryCacheKey {
	rect_min_x_bits: u32,
	rect_min_y_bits: u32,
	rect_max_x_bits: u32,
	rect_max_y_bits: u32,
	corner_radius_bits: u32,
	seam_offset_bits: u32,
	sample_count: usize,
}
impl SelectionFlowGeometryCacheKey {
	pub(in crate::overlay::rendering) const fn new(
		rect: Rect,
		corner_radius: f32,
		seam_offset: f32,
		sample_count: usize,
	) -> Self {
		Self {
			rect_min_x_bits: rect.min.x.to_bits(),
			rect_min_y_bits: rect.min.y.to_bits(),
			rect_max_x_bits: rect.max.x.to_bits(),
			rect_max_y_bits: rect.max.y.to_bits(),
			corner_radius_bits: corner_radius.to_bits(),
			seam_offset_bits: seam_offset.to_bits(),
			sample_count,
		}
	}
}

#[derive(Debug, Default)]
pub(in crate::overlay) struct SelectionFlowGeometryCache {
	pub(in crate::overlay) key: Option<SelectionFlowGeometryCacheKey>,
	pub(in crate::overlay) samples: Vec<(Pos2, f32)>,
	pub(in crate::overlay) normals: Vec<Vec2>,
}
impl SelectionFlowGeometryCache {
	#[cfg(test)]
	pub(in crate::overlay) fn is_empty(&self) -> bool {
		self.key.is_none() && self.samples.is_empty() && self.normals.is_empty()
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::overlay) struct SelectionDashedBorderCacheKey {
	rect_min_x_bits: u32,
	rect_min_y_bits: u32,
	rect_max_x_bits: u32,
	rect_max_y_bits: u32,
	dash_length_bits: u32,
	gap_length_bits: u32,
	corner_keepout_bits: u32,
}
impl SelectionDashedBorderCacheKey {
	pub(in crate::overlay::rendering) const fn new(
		rect: Rect,
		dash_length: f32,
		gap_length: f32,
		corner_keepout: f32,
	) -> Self {
		Self {
			rect_min_x_bits: rect.min.x.to_bits(),
			rect_min_y_bits: rect.min.y.to_bits(),
			rect_max_x_bits: rect.max.x.to_bits(),
			rect_max_y_bits: rect.max.y.to_bits(),
			dash_length_bits: dash_length.to_bits(),
			gap_length_bits: gap_length.to_bits(),
			corner_keepout_bits: corner_keepout.to_bits(),
		}
	}
}

#[derive(Debug, Default)]
pub(in crate::overlay) struct SelectionDashedBorderCache {
	pub(in crate::overlay) key: Option<SelectionDashedBorderCacheKey>,
	pub(in crate::overlay) segments: Vec<[Pos2; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::overlay) struct SelectionDashedBorderMetrics {
	pub(in crate::overlay) stroke_width: f32,
	pub(in crate::overlay) dash_length: f32,
	pub(in crate::overlay) gap_length: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::overlay) struct SelectionSizeBadgePadding {
	pub(in crate::overlay::rendering) left: f32,
	pub(in crate::overlay::rendering) right: f32,
	pub(in crate::overlay::rendering) top: f32,
	pub(in crate::overlay::rendering) bottom: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::overlay) struct SelectionSizeBadgeLayout {
	pub(in crate::overlay) text_size: Vec2,
	pub(in crate::overlay) badge_size: Vec2,
	pub(in crate::overlay::rendering) padding: SelectionSizeBadgePadding,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::overlay) struct SelectionSizeBadgeTarget {
	pub(in crate::overlay) rect: Rect,
	pub(in crate::overlay) size_points: RectPoints,
}
