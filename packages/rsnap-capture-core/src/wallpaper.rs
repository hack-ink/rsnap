//! Platform-neutral wallpaper thumbnail decoding for large PNG backgrounds.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use color_eyre::eyre::{self, Result, WrapErr};
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use png::{BitDepth, ColorType, Decoder, Transformations};

use crate::RgbaExportImage;

const MAX_LANCZOS_INTERMEDIATE_PIXEL_SIZE: u32 = 6_000;
const WALLPAPER_THUMBNAIL_CACHE_CAPACITY: usize = 4;

static WALLPAPER_THUMBNAIL_CACHE: OnceLock<Mutex<WallpaperThumbnailCache>> = OnceLock::new();

#[derive(Clone, Copy)]
enum PngRowLayout {
	Rgba,
	GrayscaleAlpha,
}
impl PngRowLayout {
	fn from_output((color_type, bit_depth): (ColorType, BitDepth)) -> Result<Self> {
		if bit_depth != BitDepth::Eight {
			return Err(eyre::eyre!(
				"unsupported PNG bit depth after transformations: {bit_depth:?}"
			));
		}

		match color_type {
			ColorType::Rgba => Ok(Self::Rgba),
			ColorType::GrayscaleAlpha => Ok(Self::GrayscaleAlpha),
			other => {
				Err(eyre::eyre!("unsupported PNG color type after transformations: {other:?}"))
			},
		}
	}

	fn expected_len(self, width: u32) -> Result<usize> {
		(width as usize)
			.checked_mul(self.bytes_per_pixel())
			.ok_or_else(|| eyre::eyre!("PNG row length overflow"))
	}

	fn bytes_per_pixel(self) -> usize {
		match self {
			Self::Rgba => 4,
			Self::GrayscaleAlpha => 2,
		}
	}

	fn add_weighted_rgba(
		self,
		row: &[u8],
		source_x: usize,
		weight: f32,
		accumulator: &mut [f32; 4],
	) {
		let pixel_index = source_x * self.bytes_per_pixel();

		match self {
			Self::Rgba => {
				accumulator[0] += f32::from(row[pixel_index]) * weight;
				accumulator[1] += f32::from(row[pixel_index + 1]) * weight;
				accumulator[2] += f32::from(row[pixel_index + 2]) * weight;
				accumulator[3] += f32::from(row[pixel_index + 3]) * weight;
			},
			Self::GrayscaleAlpha => {
				let luma = f32::from(row[pixel_index]) * weight;

				accumulator[0] += luma;
				accumulator[1] += luma;
				accumulator[2] += luma;
				accumulator[3] += f32::from(row[pixel_index + 1]) * weight;
			},
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct WallpaperThumbnailCacheKey {
	path: PathBuf,
	target_pixel_size: u32,
	file_size: Option<u64>,
	modified_nanos: Option<u128>,
}
impl WallpaperThumbnailCacheKey {
	fn from_path(path: &Path, target_pixel_size: u32) -> Self {
		let metadata = path.metadata().ok();
		let modified_nanos = metadata
			.as_ref()
			.and_then(|metadata| metadata.modified().ok())
			.and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
			.map(|duration| duration.as_nanos());

		Self {
			path: path.to_path_buf(),
			target_pixel_size,
			file_size: metadata.map(|metadata| metadata.len()),
			modified_nanos,
		}
	}
}

#[derive(Debug)]
struct WallpaperThumbnailCache {
	capacity: usize,
	images: Vec<(WallpaperThumbnailCacheKey, RgbaExportImage)>,
}
impl WallpaperThumbnailCache {
	fn new(capacity: usize) -> Self {
		Self { capacity: capacity.max(1), images: Vec::new() }
	}

	fn image(&mut self, key: &WallpaperThumbnailCacheKey) -> Option<RgbaExportImage> {
		let index = self.images.iter().position(|(candidate, _)| candidate == key)?;
		let (key, image) = self.images.remove(index);
		let cloned = image.clone();

		self.images.push((key, image));

		Some(cloned)
	}

	fn store(&mut self, key: WallpaperThumbnailCacheKey, image: RgbaExportImage) {
		if let Some(index) = self.images.iter().position(|(candidate, _)| *candidate == key) {
			self.images.remove(index);
		}

		self.images.push((key, image));

		while self.images.len() > self.capacity {
			self.images.remove(0);
		}
	}
}

/// Decodes a PNG wallpaper and downsamples it into a bounded RGBA thumbnail.
///
/// The implementation streams source rows instead of materializing the full decoded image. That
/// keeps huge wallpapers from allocating hundreds of megabytes just to draw a background preview.
pub fn capture_frame_wallpaper_png_thumbnail(
	path: impl AsRef<Path>,
	target_pixel_size: u32,
) -> Result<Option<RgbaExportImage>> {
	let path = path.as_ref();

	if target_pixel_size == 0 || !is_png_path(path) {
		return Ok(None);
	}

	let intermediate_target = intermediate_target_pixel_size(target_pixel_size);
	let thumbnail =
		decode_png_streaming_area_thumbnail(path, intermediate_target).wrap_err_with(|| {
			format!("failed to decode PNG wallpaper thumbnail: {}", path.display())
		})?;
	let thumbnail = resize_rgba_lanczos_to_fit(thumbnail, target_pixel_size)?;

	Ok(Some(thumbnail))
}

/// Decodes a PNG wallpaper thumbnail through the shared Rust cache.
///
/// Cache invalidation is based on path, target size, file size, and modification time.
pub fn capture_frame_wallpaper_png_thumbnail_cached(
	path: impl AsRef<Path>,
	target_pixel_size: u32,
) -> Result<Option<RgbaExportImage>> {
	let path = path.as_ref();

	if target_pixel_size == 0 || !is_png_path(path) {
		return Ok(None);
	}

	let key = WallpaperThumbnailCacheKey::from_path(path, target_pixel_size);
	let cache = WALLPAPER_THUMBNAIL_CACHE.get_or_init(|| {
		Mutex::new(WallpaperThumbnailCache::new(WALLPAPER_THUMBNAIL_CACHE_CAPACITY))
	});

	if let Some(image) = cache
		.lock()
		.map_err(|_| eyre::eyre!("wallpaper thumbnail cache lock was poisoned"))?
		.image(&key)
	{
		return Ok(Some(image));
	}

	let thumbnail = capture_frame_wallpaper_png_thumbnail(path, target_pixel_size)?;

	if let Some(image) = thumbnail.as_ref() {
		cache
			.lock()
			.map_err(|_| eyre::eyre!("wallpaper thumbnail cache lock was poisoned"))?
			.store(key, image.clone());
	}

	Ok(thumbnail)
}

fn decode_png_streaming_area_thumbnail(
	path: &Path,
	target_pixel_size: u32,
) -> Result<RgbaExportImage> {
	let file = File::open(path).wrap_err_with(|| format!("failed to open {}", path.display()))?;
	let mut decoder = Decoder::new(BufReader::new(file));

	decoder.set_transformations(Transformations::ALPHA | Transformations::STRIP_16);

	let mut reader = decoder.read_info().wrap_err("failed to read PNG metadata")?;
	let source_width = reader.info().width;
	let source_height = reader.info().height;
	let row_layout = PngRowLayout::from_output(reader.output_color_type())?;
	let Some((destination_width, destination_height)) =
		fit_inside(source_width, source_height, target_pixel_size)
	else {
		return Err(eyre::eyre!("PNG wallpaper has invalid dimensions"));
	};
	let expected_row_len = row_layout.expected_len(source_width)?;
	let x_contributions = destination_axis_contributions(
		source_width as usize,
		destination_width as usize,
		source_width as f32 / destination_width as f32,
	);
	let y_contributions = source_axis_contributions(
		source_height as usize,
		destination_height as usize,
		source_height as f32 / destination_height as f32,
	);
	let area = (source_width as f32 / destination_width as f32)
		* (source_height as f32 / destination_height as f32);
	let accumulator_len = expected_rgba_len(destination_width, destination_height)?;
	let mut accumulator = vec![0.0_f32; accumulator_len];
	let destination_width_usize = destination_width as usize;
	let mut source_y = 0_usize;

	while let Some(row) = reader.next_row().wrap_err("failed to decode PNG row")? {
		let row = row.data();

		if row.len() != expected_row_len {
			return Err(eyre::eyre!(
				"unsupported PNG row layout: expected {expected_row_len} RGBA bytes, got {}",
				row.len()
			));
		}

		let y_weights = &y_contributions[source_y];

		for (destination_x, x_weights) in x_contributions.iter().enumerate() {
			let mut horizontal = [0.0_f32; 4];

			for &(source_x, x_weight) in x_weights {
				row_layout.add_weighted_rgba(row, source_x, x_weight, &mut horizontal);
			}
			for &(destination_y, y_weight) in y_weights {
				let accumulator_index =
					(destination_y * destination_width_usize + destination_x) * 4;

				accumulator[accumulator_index] += horizontal[0] * y_weight;
				accumulator[accumulator_index + 1] += horizontal[1] * y_weight;
				accumulator[accumulator_index + 2] += horizontal[2] * y_weight;
				accumulator[accumulator_index + 3] += horizontal[3] * y_weight;
			}
		}

		source_y += 1;
	}

	if source_y != source_height as usize {
		return Err(eyre::eyre!("decoded {source_y} PNG rows, expected {source_height}"));
	}

	let mut rgba = vec![0_u8; accumulator.len()];

	for (index, value) in accumulator.into_iter().enumerate() {
		rgba[index] = (value / area).round().clamp(0.0, 255.0) as u8;
	}

	RgbaExportImage::from_raw(destination_width, destination_height, rgba)
}

fn resize_rgba_lanczos_to_fit(
	image: RgbaExportImage,
	target_pixel_size: u32,
) -> Result<RgbaExportImage> {
	let Some((destination_width, destination_height)) =
		fit_inside(image.width(), image.height(), target_pixel_size)
	else {
		return Err(eyre::eyre!("PNG wallpaper thumbnail has invalid dimensions"));
	};

	if destination_width == image.width() && destination_height == image.height() {
		return Ok(image);
	}

	let source_ref = ImageRef::new(image.width(), image.height(), image.as_raw(), PixelType::U8x4)
		.wrap_err("failed to prepare source wallpaper thumbnail for Lanczos resize")?;
	let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));
	let mut destination_image = Image::new(destination_width, destination_height, PixelType::U8x4);
	let mut resizer = Resizer::new();

	resizer
		.resize(&source_ref, &mut destination_image, &options)
		.wrap_err("failed to Lanczos-resize wallpaper thumbnail")?;

	RgbaExportImage::from_raw(destination_width, destination_height, destination_image.into_vec())
}

fn is_png_path(path: &Path) -> bool {
	path.extension()
		.and_then(|extension| extension.to_str())
		.is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
}

fn fit_inside(width: u32, height: u32, target_pixel_size: u32) -> Option<(u32, u32)> {
	if width == 0 || height == 0 || target_pixel_size == 0 {
		return None;
	}

	let max_side = width.max(height);

	if max_side <= target_pixel_size {
		return Some((width, height));
	}

	let scale = f64::from(target_pixel_size) / f64::from(max_side);

	Some((
		(f64::from(width) * scale).round().max(1.0) as u32,
		(f64::from(height) * scale).round().max(1.0) as u32,
	))
}

fn destination_axis_contributions(
	source_len: usize,
	destination_len: usize,
	scale: f32,
) -> Vec<Vec<(usize, f32)>> {
	(0..destination_len)
		.map(|destination_index| {
			let destination_start = destination_index as f32 * scale;
			let destination_end = destination_start + scale;
			let source_start = destination_start.floor().max(0.0) as usize;
			let source_end = (destination_end.ceil() as usize).min(source_len);

			(source_start..source_end)
				.filter_map(|source_index| {
					let source_start = source_index as f32;
					let source_end = source_start + 1.0;
					let overlap =
						source_end.min(destination_end) - source_start.max(destination_start);

					(overlap > 0.0).then_some((source_index, overlap))
				})
				.collect()
		})
		.collect()
}

fn source_axis_contributions(
	source_len: usize,
	destination_len: usize,
	scale: f32,
) -> Vec<Vec<(usize, f32)>> {
	(0..source_len)
		.map(|source_index| {
			let source_start = source_index as f32;
			let source_end = source_start + 1.0;
			let destination_start =
				((source_start / scale).floor().max(0.0) as usize).min(destination_len);
			let destination_end = ((source_end / scale).ceil() as usize).min(destination_len);

			(destination_start..destination_end)
				.filter_map(|destination_index| {
					let destination_source_start = destination_index as f32 * scale;
					let destination_source_end = destination_source_start + scale;
					let overlap = source_end.min(destination_source_end)
						- source_start.max(destination_source_start);

					(overlap > 0.0).then_some((destination_index, overlap))
				})
				.collect()
		})
		.collect()
}

fn expected_rgba_len(width: u32, height: u32) -> Result<usize> {
	(width as usize)
		.checked_mul(height as usize)
		.and_then(|pixels| pixels.checked_mul(4))
		.ok_or_else(|| eyre::eyre!("RGBA image dimensions overflow"))
}

fn intermediate_target_pixel_size(target_pixel_size: u32) -> u32 {
	target_pixel_size
		.saturating_mul(2)
		.min(MAX_LANCZOS_INTERMEDIATE_PIXEL_SIZE)
		.max(target_pixel_size)
}

#[cfg(test)]
mod tests {
	use std::env;
	use std::fs;
	use std::process;

	use image::codecs::png::PngEncoder;
	use image::{ExtendedColorType, ImageEncoder, RgbaImage};

	use crate::wallpaper::{self, Path};

	#[test]
	fn png_thumbnail_downsamples_without_full_frame_buffer() {
		let path = env::temp_dir().join(format!(
			"rsnap-wallpaper-thumb-{}-{}.png",
			process::id(),
			"downsample"
		));

		write_test_png(&path, 4, 2);

		let thumbnail = wallpaper::capture_frame_wallpaper_png_thumbnail(&path, 2)
			.expect("test PNG should decode")
			.expect("PNG thumbnail should be produced");
		let _ = fs::remove_file(path);

		assert_eq!(thumbnail.width(), 2);
		assert_eq!(thumbnail.height(), 1);
		assert_eq!(thumbnail.as_raw().len(), 8);
	}

	#[test]
	fn png_thumbnail_skips_non_png_paths() {
		let thumbnail =
			wallpaper::capture_frame_wallpaper_png_thumbnail("/tmp/not-a-wallpaper.jpg", 128)
				.expect("non-PNG extension should not be an error");

		assert!(thumbnail.is_none());
	}

	#[test]
	fn png_thumbnail_decodes_rgb_and_grayscale_inputs() {
		let rgb_path =
			env::temp_dir().join(format!("rsnap-wallpaper-thumb-{}-{}.png", process::id(), "rgb"));
		let grayscale_path = env::temp_dir().join(format!(
			"rsnap-wallpaper-thumb-{}-{}.png",
			process::id(),
			"grayscale"
		));

		write_rgb_test_png(&rgb_path, 3, 2);
		write_grayscale_test_png(&grayscale_path, 3, 2);

		let rgb_thumbnail = wallpaper::capture_frame_wallpaper_png_thumbnail(&rgb_path, 3)
			.expect("RGB test PNG should decode")
			.expect("RGB PNG thumbnail should be produced");
		let grayscale_thumbnail =
			wallpaper::capture_frame_wallpaper_png_thumbnail(&grayscale_path, 3)
				.expect("grayscale test PNG should decode")
				.expect("grayscale PNG thumbnail should be produced");
		let _ = fs::remove_file(rgb_path);
		let _ = fs::remove_file(grayscale_path);

		assert_eq!(rgb_thumbnail.as_raw().len(), 24);
		assert_eq!(grayscale_thumbnail.as_raw().len(), 24);
	}

	#[test]
	fn intermediate_target_keeps_common_exports_oversampled_without_unbounded_growth() {
		assert_eq!(wallpaper::intermediate_target_pixel_size(1_536), 3_072);
		assert_eq!(wallpaper::intermediate_target_pixel_size(3_000), 6_000);
		assert_eq!(wallpaper::intermediate_target_pixel_size(6_000), 6_000);
	}

	#[test]
	fn png_thumbnail_cached_reuses_valid_cached_thumbnail() {
		let path = env::temp_dir().join(format!(
			"rsnap-wallpaper-thumb-{}-{}.png",
			process::id(),
			"cache"
		));

		write_test_png(&path, 8, 4);

		let first = wallpaper::capture_frame_wallpaper_png_thumbnail_cached(&path, 4)
			.expect("test PNG should decode")
			.expect("cached PNG thumbnail should be produced");
		let second = wallpaper::capture_frame_wallpaper_png_thumbnail_cached(&path, 4)
			.expect("test PNG should decode through cache")
			.expect("cached PNG thumbnail should be produced");
		let _ = fs::remove_file(path);

		assert_eq!(first, second);
	}

	fn write_test_png(path: &Path, width: u32, height: u32) {
		let mut image = RgbaImage::new(width, height);

		for y in 0..height {
			for x in 0..width {
				image.put_pixel(
					x,
					y,
					image::Rgba([(x * 40) as u8, (y * 80) as u8, ((x + y) * 24) as u8, 255]),
				);
			}
		}

		let mut bytes = Vec::new();

		PngEncoder::new(&mut bytes)
			.write_image(image.as_raw(), width, height, ExtendedColorType::Rgba8)
			.expect("test PNG should encode");
		fs::write(path, bytes).expect("test PNG should be written");
	}

	fn write_rgb_test_png(path: &Path, width: u32, height: u32) {
		let mut bytes = Vec::new();
		let mut rgb = Vec::new();

		for y in 0..height {
			for x in 0..width {
				rgb.extend_from_slice(&[(x * 40) as u8, (y * 80) as u8, ((x + y) * 24) as u8]);
			}
		}

		PngEncoder::new(&mut bytes)
			.write_image(&rgb, width, height, ExtendedColorType::Rgb8)
			.expect("test RGB PNG should encode");
		fs::write(path, bytes).expect("test RGB PNG should be written");
	}

	fn write_grayscale_test_png(path: &Path, width: u32, height: u32) {
		let mut bytes = Vec::new();
		let mut grayscale = Vec::new();

		for y in 0..height {
			for x in 0..width {
				grayscale.push(((x * 40) + (y * 20)) as u8);
			}
		}

		PngEncoder::new(&mut bytes)
			.write_image(&grayscale, width, height, ExtendedColorType::L8)
			.expect("test grayscale PNG should encode");
		fs::write(path, bytes).expect("test grayscale PNG should be written");
	}
}
