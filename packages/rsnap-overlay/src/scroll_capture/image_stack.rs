use color_eyre::eyre::{self, Result};
use image::{
	RgbaImage,
	imageops::{self, FilterType},
};

pub(crate) fn compose_provisional_preview_image(
	base_preview: &RgbaImage,
	latest_frame: Option<&RgbaImage>,
	motion_rows_hint: Option<u32>,
	preview_width_px: u32,
) -> RgbaImage {
	let Some(frame) = latest_frame else {
		return base_preview.clone();
	};
	let Some(motion_rows_hint) = motion_rows_hint else {
		return base_preview.clone();
	};
	let hinted_growth_rows = motion_rows_hint.min(frame.height());

	if hinted_growth_rows == 0 {
		return base_preview.clone();
	}

	let Some(strip) = crop_bottom_rows(frame, hinted_growth_rows) else {
		return base_preview.clone();
	};
	let preview_strip = resize_strip_to_preview_width(&strip, preview_width_px);

	append_vertical_image(base_preview, &preview_strip).unwrap_or_else(|_| base_preview.clone())
}

pub(super) fn resize_strip_to_preview_width(strip: &RgbaImage, preview_width_px: u32) -> RgbaImage {
	if strip.width() <= preview_width_px {
		return strip.clone();
	}

	let preview_height = ((strip.height() as f32 / strip.width() as f32) * preview_width_px as f32)
		.round()
		.max(1.0) as u32;

	imageops::resize(strip, preview_width_px, preview_height, FilterType::Triangle)
}

pub(super) fn crop_bottom_rows(frame: &RgbaImage, rows: u32) -> Option<RgbaImage> {
	let rows = rows.min(frame.height());

	if rows == 0 {
		return None;
	}

	let start_y = frame.height().saturating_sub(rows);

	Some(imageops::crop_imm(frame, 0, start_y, frame.width(), rows).to_image())
}

pub(super) fn stack_vertical_images(images: &[&RgbaImage]) -> Result<RgbaImage> {
	let Some(first) = images.first() else {
		return Err(eyre::eyre!("cannot stack an empty image list"));
	};
	let width = first.width();
	let total_height = images.iter().try_fold(0_u32, |acc, image| {
		if image.width() != width {
			return Err(eyre::eyre!(
				"image width mismatch while stacking: expected {} got {}",
				width,
				image.width()
			));
		}

		acc.checked_add(image.height()).ok_or_else(|| eyre::eyre!("stacked image height overflow"))
	})?;
	let total_bytes = images.iter().try_fold(0_usize, |acc, image| {
		acc.checked_add(image.as_raw().len())
			.ok_or_else(|| eyre::eyre!("stacked image byte length overflow"))
	})?;
	let mut raw = Vec::with_capacity(total_bytes);

	for image in images {
		raw.extend_from_slice(image.as_raw());
	}

	RgbaImage::from_raw(width, total_height, raw)
		.ok_or_else(|| eyre::eyre!("failed to construct stacked image buffer"))
}

pub(super) fn append_vertical_image(base: &RgbaImage, strip: &RgbaImage) -> Result<RgbaImage> {
	if base.width() != strip.width() {
		return Err(eyre::eyre!(
			"image width mismatch while appending: expected {} got {}",
			base.width(),
			strip.width()
		));
	}

	stack_vertical_images(&[base, strip])
}
