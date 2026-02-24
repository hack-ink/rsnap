use color_eyre::eyre::{Result, WrapErr};
use image::{ImageFormat, RgbaImage};

pub(crate) fn rgba_image_to_png_bytes(image: &RgbaImage) -> Result<Vec<u8>> {
	let mut bytes = Vec::new();
	image
		.write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
		.wrap_err("failed to encode screenshot as PNG")?;
	Ok(bytes)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn png_signature_is_correct() {
		let image = RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]));
		let png = rgba_image_to_png_bytes(&image).unwrap();
		assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
	}
}
