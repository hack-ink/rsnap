//! Lossless export-image primitives owned by the Rust product core.

use color_eyre::eyre::{self, Result, WrapErr};
use image::{RgbaImage, imageops};
use png::{BitDepth, ColorType, Compression, Encoder, Filter, PixelDimensions, Unit};

use crate::RectPoints;

const BASE_SCREEN_DPI: f64 = 72.0;
const INCHES_PER_METER: f64 = 39.370_078_740_157_48;

/// Rectangle in display point space used for export geometry decisions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayPointRect {
	/// Left coordinate in display points.
	pub x: f64,
	/// Top coordinate in display points.
	pub y: f64,
	/// Rectangle width in display points.
	pub width: f64,
	/// Rectangle height in display points.
	pub height: f64,
}
impl DisplayPointRect {
	/// Creates a display-space rectangle.
	#[must_use]
	pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
		Self { x, y, width, height }
	}

	fn max_y(self) -> f64 {
		self.y + self.height
	}

	fn is_valid(self) -> bool {
		self.x.is_finite()
			&& self.y.is_finite()
			&& self.width.is_finite()
			&& self.height.is_finite()
			&& self.width > 0.0
			&& self.height > 0.0
	}
}

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
			return Err(eyre::eyre!(
				"RGBA export image byte length mismatch: expected {expected}, got {actual}"
			));
		}

		let image = RgbaImage::from_raw(width, height, rgba)
			.ok_or_else(|| eyre::eyre!("failed to create RGBA export image from raw bytes"))?;

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

	/// Encodes this export image with display-density metadata for the screen scale.
	pub fn to_png_bytes_with_screen_scale(&self, scale_factor_x1000: u32) -> Result<Vec<u8>> {
		encode_png_lossless_fast_with_screen_scale(&self.image, scale_factor_x1000)
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

/// Resolves a frozen display selection into an image-local pixel crop rectangle.
///
/// `display_frame` and `selection` use the same global display point coordinate
/// space. The returned rectangle mirrors CoreGraphics' integral crop semantics:
/// fractional source rectangles are expanded to the smallest containing integer
/// pixel rectangle and then clipped to the display image bounds.
#[must_use]
pub fn frozen_display_crop_rect(
	image_width: u32,
	image_height: u32,
	display_frame: DisplayPointRect,
	selection: DisplayPointRect,
) -> Option<RectPoints> {
	if image_width == 0 || image_height == 0 || !display_frame.is_valid() || !selection.is_valid() {
		return None;
	}

	let image_width_f64 = f64::from(image_width);
	let image_height_f64 = f64::from(image_height);
	let left = ((selection.x - display_frame.x) / display_frame.width) * image_width_f64;
	let top =
		((display_frame.max_y() - selection.max_y()) / display_frame.height) * image_height_f64;
	let width = (selection.width / display_frame.width) * image_width_f64;
	let height = (selection.height / display_frame.height) * image_height_f64;

	integral_image_intersection(left, top, width, height, image_width, image_height)
}

/// Encodes an RGBA export image as lossless PNG using the fast capture-output profile.
///
/// The encoder uses PNG's uncompressed mode and disables filtering. That keeps
/// the image byte-exact after decoding while avoiding expensive deflate work on
/// the capture hot path.
pub fn encode_png_lossless_fast(image: &RgbaImage) -> Result<Vec<u8>> {
	encode_png_lossless_fast_inner(image, None)
}

/// Encodes an RGBA export image with PNG physical-pixel density metadata.
///
/// A 2x Retina capture still stores every physical pixel. The density metadata lets
/// consumers that honor PNG `pHYs` display it as `points @ scale` instead of as a
/// larger 1x image.
pub fn encode_png_lossless_fast_with_screen_scale(
	image: &RgbaImage,
	scale_factor_x1000: u32,
) -> Result<Vec<u8>> {
	let pixel_dims = screen_scale_pixel_dimensions(scale_factor_x1000)?;

	encode_png_lossless_fast_inner(image, Some(pixel_dims))
}

fn encode_png_lossless_fast_inner(
	image: &RgbaImage,
	pixel_dims: Option<PixelDimensions>,
) -> Result<Vec<u8>> {
	let raw_len = image.as_raw().len();
	let mut bytes = Vec::new();

	if raw_len >= 16 * 1_024 * 1_024 {
		let extra = (image.height() as usize).saturating_add(1_024);
		let _ = bytes.try_reserve_exact(raw_len.saturating_add(extra));
	}

	let mut encoder = Encoder::new(&mut bytes, image.width(), image.height());

	encoder.set_color(ColorType::Rgba);
	encoder.set_depth(BitDepth::Eight);
	encoder.set_compression(Compression::NoCompression);
	encoder.set_filter(Filter::NoFilter);
	encoder.set_pixel_dims(pixel_dims);

	let mut writer = encoder.write_header().wrap_err("failed to encode screenshot as PNG")?;

	writer.write_image_data(image.as_raw()).wrap_err("failed to encode screenshot as PNG")?;

	drop(writer);

	Ok(bytes)
}

fn screen_scale_pixel_dimensions(scale_factor_x1000: u32) -> Result<PixelDimensions> {
	if scale_factor_x1000 == 0 {
		return Err(eyre::eyre!("PNG screen scale factor must be non-zero"));
	}

	let scale = f64::from(scale_factor_x1000) / 1_000.0;
	let pixels_per_meter =
		(BASE_SCREEN_DPI * scale * INCHES_PER_METER).round().clamp(1.0, f64::from(u32::MAX)) as u32;

	Ok(PixelDimensions { xppu: pixels_per_meter, yppu: pixels_per_meter, unit: Unit::Meter })
}

fn integral_image_intersection(
	left: f64,
	top: f64,
	width: f64,
	height: f64,
	image_width: u32,
	image_height: u32,
) -> Option<RectPoints> {
	let right = left + width;
	let bottom = top + height;

	if !left.is_finite()
		|| !top.is_finite()
		|| !right.is_finite()
		|| !bottom.is_finite()
		|| width <= 0.0
		|| height <= 0.0
	{
		return None;
	}

	let clipped_left = left.floor().max(0.0);
	let clipped_top = top.floor().max(0.0);
	let clipped_right = right.ceil().min(f64::from(image_width));
	let clipped_bottom = bottom.ceil().min(f64::from(image_height));

	if clipped_left >= clipped_right || clipped_top >= clipped_bottom {
		return None;
	}

	let x = integral_f64_to_u32(clipped_left)?;
	let y = integral_f64_to_u32(clipped_top)?;
	let max_x = integral_f64_to_u32(clipped_right)?;
	let max_y = integral_f64_to_u32(clipped_bottom)?;
	let rect = RectPoints::new(x, y, max_x.checked_sub(x)?, max_y.checked_sub(y)?);

	if rect.is_empty() {
		return None;
	}

	Some(rect)
}

fn integral_f64_to_u32(value: f64) -> Option<u32> {
	if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) {
		return None;
	}

	Some(value as u32)
}

fn expected_rgba_len(width: u32, height: u32) -> Result<usize> {
	if width == 0 || height == 0 {
		return Err(eyre::eyre!(
			"RGBA export image dimensions must be non-zero: width={width}, height={height}"
		));
	}

	let width = usize::try_from(width).wrap_err("failed to convert RGBA image width")?;
	let height = usize::try_from(height).wrap_err("failed to convert RGBA image height")?;

	width.checked_mul(height).and_then(|pixel_count| pixel_count.checked_mul(4)).ok_or_else(|| {
		eyre::eyre!("RGBA export image byte length overflow: width={width}, height={height}")
	})
}

#[cfg(test)]
mod tests {
	use image::{Rgba, RgbaImage};

	use crate::{DisplayPointRect, RectPoints, RgbaExportImage};

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
		let crop = crate::crop_rgba_image(&image, RectPoints::new(1, 1, 2, 2)).expect("valid crop");

		assert_eq!(crop.dimensions(), (2, 2));
		assert_eq!(crop.get_pixel(0, 0), image.get_pixel(1, 1));
		assert_eq!(crop.get_pixel(1, 1), image.get_pixel(2, 2));
	}

	#[test]
	fn crop_rgba_image_rejects_out_of_bounds_rect() {
		let image = RgbaImage::new(4, 4);

		assert!(crate::crop_rgba_image(&image, RectPoints::new(3, 3, 2, 2)).is_none());
	}

	#[test]
	fn crop_export_image_clones_full_image_without_rect() {
		let image = RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]));

		assert_eq!(crate::crop_export_image(&image, None), Some(image));
	}

	#[test]
	fn frozen_display_crop_rect_maps_global_selection_to_image_pixels() {
		let crop = crate::frozen_display_crop_rect(
			2_880,
			1_800,
			DisplayPointRect::new(0.0, 0.0, 1_440.0, 900.0),
			DisplayPointRect::new(100.0, 200.0, 300.0, 150.0),
		);

		assert_eq!(crop, Some(RectPoints::new(200, 1_100, 600, 300)));
	}

	#[test]
	fn frozen_display_crop_rect_integral_expands_and_clips() {
		let crop = crate::frozen_display_crop_rect(
			200,
			200,
			DisplayPointRect::new(0.0, 0.0, 100.0, 100.0),
			DisplayPointRect::new(-1.2, 10.25, 12.5, 20.25),
		);

		assert_eq!(crop, Some(RectPoints::new(0, 139, 23, 41)));
	}

	#[test]
	fn frozen_display_crop_rect_rejects_empty_or_outside_selection() {
		let display_frame = DisplayPointRect::new(0.0, 0.0, 100.0, 100.0);

		assert_eq!(
			crate::frozen_display_crop_rect(
				200,
				200,
				display_frame,
				DisplayPointRect::new(10.0, 10.0, 0.0, 20.0)
			),
			None
		);
		assert_eq!(
			crate::frozen_display_crop_rect(
				200,
				200,
				display_frame,
				DisplayPointRect::new(120.0, 10.0, 10.0, 20.0)
			),
			None
		);
	}

	#[test]
	fn encode_png_lossless_fast_writes_png_payload() {
		let image = RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]));
		let png = crate::encode_png_lossless_fast(&image).expect("PNG encode should succeed");

		assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
		assert_eq!(png_phys_chunk(&png), None);
	}

	#[test]
	fn encode_png_lossless_fast_with_screen_scale_writes_retina_density() {
		let image = RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]));
		let png = crate::encode_png_lossless_fast_with_screen_scale(&image, 2_000)
			.expect("PNG encode should succeed");

		assert_eq!(png_phys_chunk(&png), Some((5_669, 5_669, 1)));
	}

	#[test]
	fn encode_png_lossless_fast_with_screen_scale_rejects_zero_scale() {
		let image = RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]));

		assert!(crate::encode_png_lossless_fast_with_screen_scale(&image, 0).is_err());
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

	fn png_phys_chunk(bytes: &[u8]) -> Option<(u32, u32, u8)> {
		if bytes.len() < 8 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
			return None;
		}

		let mut offset = 8_usize;

		while offset.checked_add(12)? <= bytes.len() {
			let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
			let chunk_type = &bytes[offset + 4..offset + 8];
			let data_start = offset.checked_add(8)?;
			let data_end = data_start.checked_add(length)?;
			let chunk_end = data_end.checked_add(4)?;

			if chunk_end > bytes.len() {
				return None;
			}
			if chunk_type == b"pHYs" && length == 9 {
				let xppu = u32::from_be_bytes(bytes[data_start..data_start + 4].try_into().ok()?);
				let yppu =
					u32::from_be_bytes(bytes[data_start + 4..data_start + 8].try_into().ok()?);

				return Some((xppu, yppu, bytes[data_start + 8]));
			}

			offset = chunk_end;
		}

		None
	}
}
