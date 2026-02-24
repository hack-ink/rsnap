use color_eyre::eyre::{Result, WrapErr};
use image::RgbaImage;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};

pub(crate) fn rgba_image_to_png_bytes(image: &RgbaImage) -> Result<Vec<u8>> {
	let mut bytes = Vec::new();
	// For huge images (e.g. 8K), PNG encoding can otherwise spend noticeable time reallocating
	// and copying the growing output buffer.
	let raw_len = image.as_raw().len();

	if raw_len >= 16 * 1_024 * 1_024 {
		let extra = (image.height() as usize).saturating_add(1_024);
		let _ = bytes.try_reserve_exact(raw_len.saturating_add(extra));
	}

	let encoder = PngEncoder::new_with_quality(
		&mut bytes,
		CompressionType::Uncompressed,
		FilterType::NoFilter,
	);

	encoder
		.write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgba8)
		.wrap_err("failed to encode screenshot as PNG")?;

	Ok(bytes)
}

#[cfg(test)]
mod tests {
	use crate::png::{RgbaImage, rgba_image_to_png_bytes};

	#[test]
	fn png_signature_is_correct() {
		let image = RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]));
		let png = rgba_image_to_png_bytes(&image).unwrap();

		assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
	}
}
