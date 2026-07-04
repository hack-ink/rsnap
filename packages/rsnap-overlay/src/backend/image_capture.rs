#[cfg(target_os = "macos")]
use std::sync::{Mutex, mpsc};
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
use block2::RcBlock;
use color_eyre::eyre::{self, Result, WrapErr};
use image::{Rgba, RgbaImage, imageops};
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
#[cfg(target_os = "macos")]
use objc2_core_graphics::{CGDataProvider, CGImage};
#[allow(
	deprecated,
	reason = "Legacy CG capture remains as the macOS fallback while ScreenCaptureKit owns the primary capture path."
)]
#[cfg(target_os = "macos")]
use objc2_core_graphics::{CGDisplayCreateImage, CGDisplayCreateImageForRect};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSError, NSOperatingSystemVersion};
#[cfg(target_os = "macos")]
use objc2_screen_capture_kit::SCScreenshotManager;

#[cfg(not(target_os = "macos"))]
use crate::backend::CaptureBackendError;
#[cfg(target_os = "macos")]
use crate::macos_color;
use crate::state::{MonitorRect, RectPoints};

#[cfg(target_os = "macos")]
const MACOS_SCREENSHOT_CAPTURE_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(target_os = "macos")]
const MACOS_SCREENSHOT_ERROR_TIMEOUT_CODE: isize = 10;
#[cfg(target_os = "macos")]
const MACOS_SCREENSHOT_ERROR_NULL_IMAGE_CODE: isize = 11;
#[cfg(target_os = "macos")]
const MACOS_SCREENSHOT_ERROR_RETAIN_FAILED_CODE: isize = 12;

#[cfg(target_os = "macos")]
pub(super) fn rgba_image_from_cg_image_for_display(
	cg_image: &CGImage,
	display_id: Option<u32>,
) -> Result<RgbaImage> {
	if let Some(image) = macos_color::rgba_image_from_cg_image_color_managed(cg_image, display_id) {
		return Ok(image);
	}

	rgba_image_from_cg_image(cg_image)
}

#[cfg(target_os = "macos")]
#[expect(
	deprecated,
	reason = "CoreGraphics region capture remains the verified macOS fallback until XY-74/XY-75 replace this path."
)]
pub(super) fn capture_monitor_region_with_core_graphics(
	monitor: MonitorRect,
	rect_px: RectPoints,
) -> Result<RgbaImage> {
	let cg_rect = CGRect::new(
		CGPoint::new(rect_px.x as f64, rect_px.y as f64),
		CGSize::new(rect_px.width.max(1) as f64, rect_px.height.max(1) as f64),
	);
	let image = CGDisplayCreateImageForRect(monitor.id, cg_rect)
		.ok_or_else(|| eyre::eyre!("CGDisplayCreateImageForRect returned null"))?;
	let image = rgba_image_from_cg_image_for_display(image.as_ref(), Some(monitor.id))
		.wrap_err("failed to decode CGDisplay rect capture")?;

	if image.width() == rect_px.width.max(1) && image.height() == rect_px.height.max(1) {
		return Ok(image);
	}

	let full_image = CGDisplayCreateImage(monitor.id)
		.ok_or_else(|| eyre::eyre!("CGDisplayCreateImage returned null"))?;
	let full_image = rgba_image_from_cg_image_for_display(full_image.as_ref(), Some(monitor.id))
		.wrap_err("failed to decode CGDisplay full-monitor capture")?;

	crop_monitor_image_region(&full_image, rect_px)
		.wrap_err("failed to crop full-monitor fallback to requested rect")
}

pub(super) fn copy_rgba_patch(
	image: &RgbaImage,
	center_x: u32,
	center_y: u32,
	width_px: u32,
	height_px: u32,
) -> RgbaImage {
	let mut out = RgbaImage::new(width_px.max(1), height_px.max(1));
	let out_w = out.width();
	let out_h = out.height();
	let in_w = image.width() as i32;
	let in_h = image.height() as i32;
	let half_w = (out_w as i32) / 2;
	let half_h = (out_h as i32) / 2;
	let center_x = center_x as i32;
	let center_y = center_y as i32;

	for oy in 0..out_h {
		for ox in 0..out_w {
			let ix = center_x + (ox as i32) - half_w;
			let iy = center_y + (oy as i32) - half_h;

			if ix >= 0 && iy >= 0 && ix < in_w && iy < in_h {
				let pixel = image.get_pixel(ix as u32, iy as u32);

				out.put_pixel(ox, oy, *pixel);
			} else {
				out.put_pixel(ox, oy, Rgba([0, 0, 0, 0]));
			}
		}
	}

	out
}

#[cfg(target_os = "macos")]
pub(super) fn macos_supports_scroll_capture_screenshot_api_with_version(
	version: NSOperatingSystemVersion,
) -> bool {
	version.majorVersion >= 14
}

pub(super) fn normalize_capture_rect(rect_px: RectPoints) -> RectPoints {
	RectPoints::new(rect_px.x, rect_px.y, rect_px.width.max(1), rect_px.height.max(1))
}

#[cfg(target_os = "macos")]
pub(super) fn point_extent_to_pixel_extent(points: u32, scale_factor: f32) -> u32 {
	((points as f32) * scale_factor.max(1.0)).round().max(1.0) as u32
}

pub(super) fn crop_monitor_image_region(
	image: &RgbaImage,
	rect_px: RectPoints,
) -> Result<RgbaImage> {
	let rect_px = normalize_capture_rect(rect_px);
	let x = rect_px.x.min(image.width());
	let y = rect_px.y.min(image.height());
	let width = rect_px.width.min(image.width().saturating_sub(x));
	let height = rect_px.height.min(image.height().saturating_sub(y));

	if width == 0 || height == 0 {
		return Err(eyre::eyre!("capture region is outside the monitor image bounds"));
	}

	Ok(imageops::crop_imm(image, x, y, width, height).to_image())
}

#[cfg(target_os = "macos")]
pub(super) fn capture_monitor_region_image_with_screenshot_manager(
	monitor: MonitorRect,
	rect_px: RectPoints,
) -> Result<RgbaImage> {
	let rect_px = normalize_capture_rect(rect_px);
	let sf = f64::from(monitor.scale_factor()).max(1.0);
	let cg_rect = CGRect::new(
		CGPoint::new(
			f64::from(monitor.origin.x) + f64::from(rect_px.x) / sf,
			f64::from(monitor.origin.y) + f64::from(rect_px.y) / sf,
		),
		CGSize::new(f64::from(rect_px.width) / sf, f64::from(rect_px.height) / sf),
	);
	let cg_image = capture_screenshot_cg_image(cg_rect)?;
	let image = rgba_image_from_cg_image_for_display(cg_image.as_ref(), Some(monitor.id))?;

	// ScreenCaptureKit may round point-space captures by one pixel at non-integer scale edges.
	// Clamp or extend back to the requested region so the stitcher sees stable dimensions.
	if image.dimensions() == (rect_px.width, rect_px.height) {
		Ok(image)
	} else {
		Ok(normalize_capture_image_extent(&image, rect_px.width, rect_px.height))
	}
}

#[cfg(not(target_os = "macos"))]
pub(super) fn capture_monitor_image(monitor: MonitorRect) -> Result<RgbaImage> {
	let xcap_monitor = xcap_find_monitor(monitor)?;
	let image = xcap_monitor.capture_image().wrap_err("xcap capture_image failed")?;

	Ok(image)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn xcap_find_monitor(monitor: MonitorRect) -> Result<xcap::Monitor> {
	let monitors = xcap::Monitor::all().wrap_err("xcap Monitor::all failed")?;

	for m in monitors {
		if m.id().wrap_err("Failed to read xcap monitor id")? == monitor.id {
			return Ok(m);
		}
	}

	Err(CaptureBackendError::MonitorNotFound { monitor }.into())
}

#[cfg(target_os = "macos")]
fn rgba_image_from_cg_image(cg_image: &CGImage) -> Result<RgbaImage> {
	let width = CGImage::width(Some(cg_image));
	let height = CGImage::height(Some(cg_image));

	if width == 0 || height == 0 {
		return Err(eyre::eyre!("CGImage has zero dimensions"));
	}

	let data_provider = CGImage::data_provider(Some(cg_image))
		.ok_or_else(|| eyre::eyre!("Failed to get CGImage data provider"))?;
	let data = CGDataProvider::data(Some(data_provider.as_ref()))
		.ok_or_else(|| eyre::eyre!("Failed to copy CGImage bytes"))?;
	let bytes_per_row = CGImage::bytes_per_row(Some(cg_image));

	rgba_image_from_bgra_rows(width, height, bytes_per_row, &data.to_vec())
}

#[cfg(target_os = "macos")]
fn rgba_image_from_bgra_rows(
	width: usize,
	height: usize,
	bytes_per_row: usize,
	data: &[u8],
) -> Result<RgbaImage> {
	let expected_row_bytes =
		width.checked_mul(4).ok_or_else(|| eyre::eyre!("row byte count overflowed"))?;

	if bytes_per_row < expected_row_bytes {
		return Err(eyre::eyre!(
			"CGImage bytes_per_row {bytes_per_row} is smaller than the required RGBA row width {expected_row_bytes}"
		));
	}

	let required_len = height
		.checked_mul(bytes_per_row)
		.ok_or_else(|| eyre::eyre!("CGImage backing store length overflowed"))?;

	if data.len() < required_len {
		return Err(eyre::eyre!(
			"CGImage backing store is shorter than the declared image size: expected at least {required_len} bytes, got {}",
			data.len()
		));
	}

	let mut buffer = Vec::with_capacity(width * height * 4);

	for row in data[..required_len].chunks_exact(bytes_per_row) {
		buffer.extend_from_slice(&row[..expected_row_bytes]);
	}
	for bgra in buffer.chunks_exact_mut(4) {
		bgra.swap(0, 2);
	}

	RgbaImage::from_raw(width as u32, height as u32, buffer)
		.ok_or_else(|| eyre::eyre!("RgbaImage::from_raw failed"))
}

#[cfg(target_os = "macos")]
fn normalize_capture_image_extent(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
	let width = width.max(1);
	let height = height.max(1);

	if image.dimensions() == (width, height) {
		return image.clone();
	}

	let source_max_x = image.width().saturating_sub(1);
	let source_max_y = image.height().saturating_sub(1);
	let mut out = RgbaImage::new(width, height);

	for out_y in 0..height {
		let sample_y = out_y.min(source_max_y);

		for out_x in 0..width {
			let sample_x = out_x.min(source_max_x);

			out.put_pixel(out_x, out_y, *image.get_pixel(sample_x, sample_y));
		}
	}

	out
}

#[cfg(target_os = "macos")]
fn capture_screenshot_cg_image(rect: CGRect) -> Result<Retained<CGImage>> {
	let (tx, rx) = mpsc::sync_channel::<Result<Retained<CGImage>, Retained<NSError>>>(1);
	let tx = Mutex::new(Some(tx));
	let block = RcBlock::new(move |image: *mut CGImage, err: *mut NSError| {
		let mut maybe_tx = match tx.lock() {
			Ok(guard) => guard,
			Err(poisoned) => poisoned.into_inner(),
		};
		let Some(tx) = maybe_tx.take() else {
			return;
		};

		if !err.is_null() {
			let Some(err) = (unsafe { Retained::retain(err) }) else {
				let _ = tx.send(Err(screenshot_error(MACOS_SCREENSHOT_ERROR_RETAIN_FAILED_CODE)));

				return;
			};
			let _ = tx.send(Err(err));

			return;
		}

		let Some(image) = (unsafe { Retained::retain(image) }) else {
			let _ = tx.send(Err(screenshot_error(MACOS_SCREENSHOT_ERROR_NULL_IMAGE_CODE)));

			return;
		};
		let _ = tx.send(Ok(image));
	});

	unsafe { SCScreenshotManager::captureImageInRect_completionHandler(rect, Some(&block)) };

	rx.recv_timeout(MACOS_SCREENSHOT_CAPTURE_TIMEOUT)
		.map_err(|_| screenshot_error(MACOS_SCREENSHOT_ERROR_TIMEOUT_CODE))?
		.map_err(|err| eyre::eyre!("{}", err.localizedDescription()))
}

#[cfg(target_os = "macos")]
fn screenshot_error(code: isize) -> Retained<NSError> {
	NSError::new(code, objc2_foundation::ns_string!("io.hackink.rsnap.screenshot_capture"))
}

#[cfg(test)]
mod tests {
	#[cfg(target_os = "macos")]
	use image::RgbaImage;
	#[cfg(target_os = "macos")]
	use objc2_foundation::NSOperatingSystemVersion;

	#[cfg(target_os = "macos")]
	#[test]
	fn rgba_image_from_bgra_rows_truncates_trailing_rows_beyond_declared_height() {
		let width = 2_usize;
		let height = 2_usize;
		let bytes_per_row = width * 4;
		let data = vec![
			10, 20, 30, 255, 40, 50, 60, 255, // row 0
			70, 80, 90, 255, 100, 110, 120, 255, // row 1
			130, 140, 150, 255, 160, 170, 180, 255, // extra row 2
			190, 200, 210, 255, 220, 230, 240, 255, // extra row 3
		];
		let image = super::rgba_image_from_bgra_rows(width, height, bytes_per_row, &data)
			.expect("image should decode");

		assert_eq!(image.dimensions(), (2, 2));
		assert_eq!(image.as_raw().len(), width * height * 4);
		assert_eq!(image.get_pixel(0, 0).0, [30, 20, 10, 255]);
		assert_eq!(image.get_pixel(1, 1).0, [120, 110, 100, 255]);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn rgba_image_from_bgra_rows_rejects_short_backing_store() {
		let err = super::rgba_image_from_bgra_rows(
			2,
			2,
			8,
			&[
				10, 20, 30, 255, 40, 50, 60, 255, // row 0
			],
		)
		.expect_err("short backing store should fail");

		assert!(format!("{err:#}").contains("shorter than the declared image size"));
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn screenshot_api_scroll_capture_gate_requires_macos_14_or_newer() {
		assert!(!super::macos_supports_scroll_capture_screenshot_api_with_version(
			NSOperatingSystemVersion { majorVersion: 13, minorVersion: 6, patchVersion: 0 }
		));
		assert!(super::macos_supports_scroll_capture_screenshot_api_with_version(
			NSOperatingSystemVersion { majorVersion: 14, minorVersion: 0, patchVersion: 0 }
		));
		assert!(super::macos_supports_scroll_capture_screenshot_api_with_version(
			NSOperatingSystemVersion { majorVersion: 15, minorVersion: 0, patchVersion: 0 }
		));
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn normalize_capture_image_extent_pads_inward_rounded_edges_with_border_pixels() {
		let image = RgbaImage::from_vec(
			2,
			2,
			vec![
				10, 0, 0, 255, 20, 0, 0, 255, //
				30, 0, 0, 255, 40, 0, 0, 255,
			],
		)
		.expect("valid rgba image");
		let normalized = super::normalize_capture_image_extent(&image, 3, 3);

		assert_eq!(normalized.dimensions(), (3, 3));
		assert_eq!(normalized.get_pixel(0, 0).0, [10, 0, 0, 255]);
		assert_eq!(normalized.get_pixel(2, 0).0, [20, 0, 0, 255]);
		assert_eq!(normalized.get_pixel(0, 2).0, [30, 0, 0, 255]);
		assert_eq!(normalized.get_pixel(2, 2).0, [40, 0, 0, 255]);
	}
}
