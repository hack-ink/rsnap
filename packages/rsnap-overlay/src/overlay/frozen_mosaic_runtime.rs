use image::{Rgba, RgbaImage, imageops};

use crate::overlay::{
	FROZEN_MOSAIC_BLOCK_SIZE_PX, FrozenEditKind, FrozenImagePatch, FrozenMosaicEdit, MonitorRect,
	OverlaySession, RectPoints,
};

impl OverlaySession {
	pub(super) fn intersect_rect_points(left: RectPoints, right: RectPoints) -> Option<RectPoints> {
		let x = left.x.max(right.x);
		let y = left.y.max(right.y);
		let right_edge = left.x.saturating_add(left.width).min(right.x.saturating_add(right.width));
		let bottom_edge =
			left.y.saturating_add(left.height).min(right.y.saturating_add(right.height));

		(right_edge > x && bottom_edge > y).then(|| {
			RectPoints::new(x, y, right_edge.saturating_sub(x), bottom_edge.saturating_sub(y))
		})
	}

	fn build_frozen_image_patch(image: &RgbaImage, rect: RectPoints) -> Option<FrozenImagePatch> {
		let x = rect.x.min(image.width());
		let y = rect.y.min(image.height());
		let max_width = image.width().saturating_sub(x);
		let max_height = image.height().saturating_sub(y);
		let width = rect.width.min(max_width);
		let height = rect.height.min(max_height);

		if width == 0 || height == 0 {
			return None;
		}

		let rect = RectPoints::new(x, y, width, height);
		let before = imageops::crop_imm(image, x, y, width, height).to_image();
		let after = Self::mosaic_patch(&before);

		Some(FrozenImagePatch { rect, before, after })
	}

	fn mosaic_patch(region: &RgbaImage) -> RgbaImage {
		let mut out = region.clone();

		for block_y in (0..region.height()).step_by(FROZEN_MOSAIC_BLOCK_SIZE_PX as usize) {
			for block_x in (0..region.width()).step_by(FROZEN_MOSAIC_BLOCK_SIZE_PX as usize) {
				let block_width = FROZEN_MOSAIC_BLOCK_SIZE_PX.min(region.width() - block_x);
				let block_height = FROZEN_MOSAIC_BLOCK_SIZE_PX.min(region.height() - block_y);
				let mut sum = [0_u64; 4];
				let mut samples = 0_u64;

				for y in block_y..block_y.saturating_add(block_height) {
					for x in block_x..block_x.saturating_add(block_width) {
						let pixel = region.get_pixel(x, y);

						sum[0] = sum[0].saturating_add(u64::from(pixel[0]));
						sum[1] = sum[1].saturating_add(u64::from(pixel[1]));
						sum[2] = sum[2].saturating_add(u64::from(pixel[2]));
						sum[3] = sum[3].saturating_add(u64::from(pixel[3]));
						samples = samples.saturating_add(1);
					}
				}

				if samples == 0 {
					continue;
				}

				let fill = Rgba([
					(sum[0] / samples) as u8,
					(sum[1] / samples) as u8,
					(sum[2] / samples) as u8,
					(sum[3] / samples) as u8,
				]);

				for y in block_y..block_y.saturating_add(block_height) {
					for x in block_x..block_x.saturating_add(block_width) {
						out.put_pixel(x, y, fill);
					}
				}
			}
		}

		out
	}

	fn apply_frozen_image_patch(image: &mut RgbaImage, patch: &FrozenImagePatch, use_after: bool) {
		let source = if use_after { &patch.after } else { &patch.before };

		imageops::replace(image, source, i64::from(patch.rect.x), i64::from(patch.rect.y));
	}

	fn map_rect_into_window_image(
		monitor: MonitorRect,
		capture_rect_points: RectPoints,
		window_image: &RgbaImage,
		selection_rect_points: RectPoints,
	) -> Option<RectPoints> {
		let capture_rect_px = monitor.local_rect_to_pixels(capture_rect_points);
		let selection_rect_px = monitor.local_rect_to_pixels(selection_rect_points);
		let selection_rect_px = Self::intersect_rect_points(selection_rect_px, capture_rect_px)?;

		if capture_rect_px.is_empty() || window_image.width() == 0 || window_image.height() == 0 {
			return None;
		}

		let rel_left = selection_rect_px.x.saturating_sub(capture_rect_px.x);
		let rel_top = selection_rect_px.y.saturating_sub(capture_rect_px.y);
		let rel_right = rel_left.saturating_add(selection_rect_px.width);
		let rel_bottom = rel_top.saturating_add(selection_rect_px.height);
		let capture_width = u64::from(capture_rect_px.width.max(1));
		let capture_height = u64::from(capture_rect_px.height.max(1));
		let target_width = u64::from(window_image.width());
		let target_height = u64::from(window_image.height());
		let left = (u64::from(rel_left) * target_width) / capture_width;
		let top = (u64::from(rel_top) * target_height) / capture_height;
		let right = (u64::from(rel_right) * target_width).div_ceil(capture_width);
		let bottom = (u64::from(rel_bottom) * target_height).div_ceil(capture_height);
		let width = right.saturating_sub(left) as u32;
		let height = bottom.saturating_sub(top) as u32;

		(width > 0 && height > 0).then(|| {
			RectPoints::new(
				left.min(target_width) as u32,
				top.min(target_height) as u32,
				width,
				height,
			)
		})
	}

	pub(super) fn push_frozen_mosaic_edit(&mut self, edit: FrozenMosaicEdit) {
		self.frozen_mosaic_undo_stack.push(edit);
	}

	pub(super) fn apply_frozen_mosaic_edit(&mut self, rect_points: RectPoints) -> bool {
		let Some(monitor) = self.state.monitor else {
			return false;
		};

		if !self.frozen_final_capture_ready() {
			return false;
		}

		let preview_rect_px = monitor.local_rect_to_pixels(rect_points);
		let Some(preview_patch) = self
			.state
			.frozen_display_image
			.as_ref()
			.and_then(|image| Self::build_frozen_image_patch(image, preview_rect_px))
		else {
			return false;
		};
		let Some(export_patch) = self
			.state
			.frozen_export_image
			.as_ref()
			.and_then(|image| Self::build_frozen_image_patch(image, preview_rect_px))
		else {
			return false;
		};
		let window_patch = match (self.frozen_window_image.as_ref(), self.state.frozen_capture_rect)
		{
			(Some(window_image), Some(capture_rect_points)) => Self::map_rect_into_window_image(
				monitor,
				capture_rect_points,
				window_image,
				rect_points,
			)
			.and_then(|rect| Self::build_frozen_image_patch(window_image, rect)),
			_ => None,
		};

		if let Some(image) = self.state.frozen_display_image.as_mut() {
			Self::apply_frozen_image_patch(image, &preview_patch, true);
		}
		if let Some(image) = self.state.frozen_export_image.as_mut() {
			Self::apply_frozen_image_patch(image, &export_patch, true);
		}
		if let (Some(window_image), Some(window_patch)) =
			(self.frozen_window_image.as_mut(), window_patch.as_ref())
		{
			Self::apply_frozen_image_patch(window_image, window_patch, true);
		}

		self.push_frozen_mosaic_edit(FrozenMosaicEdit {
			preview_patch,
			export_patch,
			window_patch,
		});
		self.push_frozen_edit_to_undo_history(FrozenEditKind::MosaicEdit);
		self.note_frozen_image_mutated(monitor);

		true
	}

	fn replay_frozen_mosaic_edit(&mut self, edit: &FrozenMosaicEdit, use_after: bool) -> bool {
		let Some(monitor) = self.state.monitor else {
			return false;
		};

		if let Some(image) = self.state.frozen_display_image.as_mut() {
			Self::apply_frozen_image_patch(image, &edit.preview_patch, use_after);
		}
		if let Some(image) = self.state.frozen_export_image.as_mut() {
			Self::apply_frozen_image_patch(image, &edit.export_patch, use_after);
		}
		if let (Some(window_image), Some(window_patch)) =
			(self.frozen_window_image.as_mut(), edit.window_patch.as_ref())
		{
			Self::apply_frozen_image_patch(window_image, window_patch, use_after);
		}

		self.note_frozen_image_mutated(monitor);

		true
	}

	pub(super) fn undo_frozen_mosaic_edit(&mut self) -> bool {
		if !self.frozen_final_capture_ready() {
			return false;
		}

		let Some(edit) = self.frozen_mosaic_undo_stack.pop() else {
			return false;
		};
		let reapplied = self.replay_frozen_mosaic_edit(&edit, false);

		self.frozen_mosaic_redo_stack.push(edit);
		self.sync_frozen_toolbar_state();

		reapplied
	}

	pub(super) fn redo_frozen_mosaic_edit(&mut self) -> bool {
		if !self.frozen_final_capture_ready() {
			return false;
		}

		let Some(edit) = self.frozen_mosaic_redo_stack.pop() else {
			return false;
		};
		let reapplied = self.replay_frozen_mosaic_edit(&edit, true);

		self.frozen_mosaic_undo_stack.push(edit);
		self.sync_frozen_toolbar_state();

		reapplied
	}

	pub(super) fn commit_frozen_mosaic_drag(&mut self) -> bool {
		let preview_rect = self.state.frozen_mosaic_preview_rect;

		self.stop_frozen_mosaic_drag();

		let Some(preview_rect) = preview_rect else {
			return false;
		};

		if preview_rect.width <= 1 && preview_rect.height <= 1 {
			return false;
		}

		self.apply_frozen_mosaic_edit(preview_rect)
	}
}
