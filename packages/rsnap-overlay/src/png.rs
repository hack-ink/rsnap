use color_eyre::eyre::Result;
use image::RgbaImage;

pub(crate) fn rgba_image_to_png_bytes(image: &RgbaImage) -> Result<Vec<u8>> {
	rsnap_capture_core::encode_png_lossless_fast(image)
}

#[cfg(test)]
mod tests {
	use crate::png::{self, RgbaImage};

	#[test]
	fn png_signature_is_correct() {
		let image = RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]));
		let png = png::rgba_image_to_png_bytes(&image).unwrap();

		assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
	}
}
