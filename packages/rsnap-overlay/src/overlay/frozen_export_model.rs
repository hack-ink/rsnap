use egui::Pos2;
use image::RgbaImage;

use crate::state::RectPoints;

#[derive(Clone, Debug)]
pub(super) struct FrozenImagePatch {
	pub(super) rect: RectPoints,
	pub(super) before: RgbaImage,
	pub(super) after: RgbaImage,
}

#[derive(Clone, Debug)]
pub(super) struct FrozenMosaicEdit {
	pub(super) preview_patch: FrozenImagePatch,
	pub(super) export_patch: FrozenImagePatch,
	pub(super) window_patch: Option<FrozenImagePatch>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FrozenExportTransform {
	capture_rect: RectPoints,
	scale_x: f32,
	scale_y: f32,
}
impl FrozenExportTransform {
	pub(super) fn new(
		capture_rect: RectPoints,
		export_width: u32,
		export_height: u32,
	) -> Option<Self> {
		if capture_rect.width == 0
			|| capture_rect.height == 0
			|| export_width == 0
			|| export_height == 0
		{
			return None;
		}

		Some(Self {
			capture_rect,
			scale_x: export_width as f32 / capture_rect.width as f32,
			scale_y: export_height as f32 / capture_rect.height as f32,
		})
	}

	pub(super) fn point_to_pixels(self, point: Pos2) -> Pos2 {
		Pos2::new(
			(point.x - self.capture_rect.x as f32) * self.scale_x,
			(point.y - self.capture_rect.y as f32) * self.scale_y,
		)
	}

	pub(super) fn scalar_scale(self) -> f32 {
		(self.scale_x + self.scale_y) * 0.5
	}
}
