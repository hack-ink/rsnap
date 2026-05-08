//! Lossless export-image primitives owned by the Rust product core.

use color_eyre::eyre::{Result, WrapErr, eyre};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, RgbaImage, imageops};

use crate::RectPoints;

/// RGBA export image prepared by the product core.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RgbaExportImage {
	image: RgbaImage,
}

impl RgbaExportImage {
	/// Wraps an existing RGBA image as a product-core export image.
	#[must_use]
	pub fn from_image(image: RgbaImage) -> Self {
		Self { image }
	}

	/// Creates a product-core export image from raw RGBA bytes.
	///
	/// The byte payload must be exactly `width * height * 4` bytes and dimensions
	/// must be non-zero.
	pub fn from_raw(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self> {
		let expected = expected_rgba_len(width, height)?;
		let actual = rgba.len();

		if actual != expected {
			return Err(eyre!(
				"RGBA export image byte length mismatch: expected {expected}, got {actual}"
			));
		}

		let image = RgbaImage::from_raw(width, height, rgba)
			.ok_or_else(|| eyre!("failed to create RGBA export image from raw bytes"))?;

		Ok(Self { image })
	}

	/// Returns the image width in pixels.
	#[must_use]
	pub fn width(&self) -> u32 {
		self.image.width()
	}

	/// Returns the image height in pixels.
	#[must_use]
	pub fn height(&self) -> u32 {
		self.image.height()
	}

	/// Returns the underlying RGBA byte buffer.
	#[must_use]
	pub fn as_raw(&self) -> &[u8] {
		self.image.as_raw()
	}

	/// Returns the underlying image.
	#[must_use]
	pub fn as_image(&self) -> &RgbaImage {
		&self.image
	}

	/// Consumes the wrapper and returns the underlying image.
	#[must_use]
	pub fn into_image(self) -> RgbaImage {
		self.image
	}

	/// Returns a pixel-exact crop of this export image.
	#[must_use]
	pub fn crop(&self, rect: RectPoints) -> Option<Self> {
		crop_rgba_image(&self.image, rect).map(Self::from_image)
	}

	/// Encodes this export image as a lossless PNG with the fast export settings.
	pub fn to_png_bytes(&self) -> Result<Vec<u8>> {
		encode_png_lossless_fast(&self.image)
	}
}

/// Returns a pixel-exact crop when the requested rectangle lies inside the image.
#[must_use]
pub fn crop_rgba_image(image: &RgbaImage, rect: RectPoints) -> Option<RgbaImage> {
	if rect.is_empty() {
		return None;
	}

	let max_x = rect.x.checked_add(rect.width)?;
	let max_y = rect.y.checked_add(rect.height)?;

	if max_x > image.width() || max_y > image.height() {
		return None;
	}

	Some(imageops::crop_imm(image, rect.x, rect.y, rect.width, rect.height).to_image())
}

/// Crops from a frozen export image, or clones the full export when no crop is requested.
#[must_use]
pub fn crop_export_image(
	export_image: &RgbaImage,
	crop_rect: Option<RectPoints>,
) -> Option<RgbaImage> {
	crop_rect.map_or_else(
		|| Some(export_image.clone()),
		|crop_rect| crop_rgba_image(export_image, crop_rect),
	)
}

/// Encodes an RGBA export image as lossless PNG using the fast capture-output profile.
///
/// The encoder uses PNG's uncompressed mode and disables filtering. That keeps
/// the image byte-exact after decoding while avoiding expensive deflate work on
/// the capture hot path.
pub fn encode_png_lossless_fast(image: &RgbaImage) -> Result<Vec<u8>> {
	let mut bytes = Vec::new();
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

fn expected_rgba_len(width: u32, height: u32) -> Result<usize> {
	if width == 0 || height == 0 {
		return Err(eyre!(
			"RGBA export image dimensions must be non-zero: width={width}, height={height}"
		));
	}

	let width = usize::try_from(width).wrap_err("failed to convert RGBA image width")?;
	let height = usize::try_from(height).wrap_err("failed to convert RGBA image height")?;

	width.checked_mul(height).and_then(|pixel_count| pixel_count.checked_mul(4)).ok_or_else(|| {
		eyre!("RGBA export image byte length overflow: width={width}, height={height}")
	})
}

#[cfg(test)]
mod tests {
	use image::{Rgba, RgbaImage};

	use crate::{
		RectPoints, RgbaExportImage, crop_export_image, crop_rgba_image, encode_png_lossless_fast,
	};

	#[test]
	fn raw_export_image_validates_byte_length() {
		let error = RgbaExportImage::from_raw(2, 2, vec![0; 15])
			.expect_err("invalid RGBA length should fail")
			.to_string();

		assert!(error.contains("byte length mismatch"));
	}

	#[test]
	fn raw_export_image_rejects_empty_dimensions() {
		let error = RgbaExportImage::from_raw(0, 2, Vec::new())
			.expect_err("empty dimensions should fail")
			.to_string();

		assert!(error.contains("dimensions must be non-zero"));
	}

	#[test]
	fn crop_rgba_image_copies_exact_pixels() {
		let image = RgbaImage::from_fn(4, 4, |x, y| Rgba([x as u8, y as u8, (x + y) as u8, 255]));
		let crop = crop_rgba_image(&image, RectPoints::new(1, 1, 2, 2)).expect("valid crop");

		assert_eq!(crop.dimensions(), (2, 2));
		assert_eq!(crop.get_pixel(0, 0), image.get_pixel(1, 1));
		assert_eq!(crop.get_pixel(1, 1), image.get_pixel(2, 2));
	}

	#[test]
	fn crop_rgba_image_rejects_out_of_bounds_rect() {
		let image = RgbaImage::new(4, 4);

		assert!(crop_rgba_image(&image, RectPoints::new(3, 3, 2, 2)).is_none());
	}

	#[test]
	fn crop_export_image_clones_full_image_without_rect() {
		let image = RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]));

		assert_eq!(crop_export_image(&image, None), Some(image));
	}

	#[test]
	fn encode_png_lossless_fast_writes_png_payload() {
		let image = RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]));
		let png = encode_png_lossless_fast(&image).expect("PNG encode should succeed");

		assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
	}

	#[test]
	fn export_image_wrapper_crops_and_encodes() {
		let image = RgbaExportImage::from_image(RgbaImage::from_fn(4, 4, |x, y| {
			Rgba([x as u8, y as u8, 0, 255])
		}));
		let crop = image.crop(RectPoints::new(1, 1, 2, 2)).expect("valid crop");
		let png = crop.to_png_bytes().expect("PNG encode should succeed");

		assert_eq!(crop.width(), 2);
		assert_eq!(crop.height(), 2);
		assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
	}
}
