use std::ffi::CStr;
use std::mem;
use std::os::raw::c_char;
use std::ptr;
use std::slice;

use crate::abi::{
	RsnapCaptureFrameBackgroundKind, RsnapCaptureFrameBackgroundPlan, RsnapCaptureFrameColorStop,
	RsnapCaptureFramePlan, RsnapCaptureFrameRenderKind, RsnapCaptureFrameShadow,
	RsnapCaptureFrameSourceKind, RsnapCaptureFrameWallpaperRequest, RsnapFloatRect,
	RsnapOwnedBytes, RsnapOwnedRgbaRegion, RsnapPixelRect, RsnapRgb, RsnapStatus,
};
use rsnap_capture_core::{
	self, BgraFrameView, CaptureFrameBackgroundKind, CaptureFrameBackgroundPlan,
	CaptureFrameColorStop, CaptureFramePlan, CaptureFrameRenderImageRef, CaptureFrameRenderKind,
	CaptureFrameShadow, CaptureFrameSourceKind, CaptureFrameWallpaperRequest, RgbaExportImage,
};

/// Encodes a full RGBA export image as lossless PNG through the Rust product core.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_png` must be writable. The returned buffer must
/// be released with `rsnap_owned_bytes_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_export_rgba_to_png(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	out_png: *mut RsnapOwnedBytes,
) -> RsnapStatus {
	if out_png.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { crate::rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(image) = RgbaExportImage::from_raw(width, height, bytes.to_vec()) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(png) = image.to_png_bytes() else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_png, owned_bytes_from_vec(png));
	}

	RsnapStatus::Ok
}

/// Encodes a full RGBA export image as lossless PNG with physical-pixel density metadata.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_png` must be writable. The returned buffer must
/// be released with `rsnap_owned_bytes_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_export_rgba_to_png_with_screen_scale(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	scale_factor_x1000: u32,
	out_png: *mut RsnapOwnedBytes,
) -> RsnapStatus {
	if out_png.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { crate::rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(image) = RgbaExportImage::from_raw(width, height, bytes.to_vec()) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(png) = image.to_png_bytes_with_screen_scale(scale_factor_x1000) else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_png, owned_bytes_from_vec(png));
	}

	RsnapStatus::Ok
}

/// Encodes a pixel-space RGBA export crop as lossless PNG through the Rust product core.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_png` must be writable. The returned buffer must
/// be released with `rsnap_owned_bytes_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_export_rgba_crop_to_png(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	crop_rect: RsnapPixelRect,
	out_png: *mut RsnapOwnedBytes,
) -> RsnapStatus {
	if out_png.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { crate::rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(image) = RgbaExportImage::from_raw(width, height, bytes.to_vec()) else {
		return RsnapStatus::InvalidInput;
	};
	let Some(cropped) = image.crop(crate::decode_pixel_rect(crop_rect)) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(png) = cropped.to_png_bytes() else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_png, owned_bytes_from_vec(png));
	}

	RsnapStatus::Ok
}

/// Encodes a pixel-space RGBA crop as lossless PNG with physical-pixel density metadata.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_png` must be writable. The returned buffer must
/// be released with `rsnap_owned_bytes_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_export_rgba_crop_to_png_with_screen_scale(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	crop_rect: RsnapPixelRect,
	scale_factor_x1000: u32,
	out_png: *mut RsnapOwnedBytes,
) -> RsnapStatus {
	if out_png.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { crate::rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(image) = RgbaExportImage::from_raw(width, height, bytes.to_vec()) else {
		return RsnapStatus::InvalidInput;
	};
	let Some(cropped) = image.crop(crate::decode_pixel_rect(crop_rect)) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(png) = cropped.to_png_bytes_with_screen_scale(scale_factor_x1000) else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_png, owned_bytes_from_vec(png));
	}

	RsnapStatus::Ok
}

/// Resolves a frozen display selection into an image-local pixel crop rectangle.
///
/// # Safety
///
/// `out_rect` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_display_crop_rect(
	image_width: u32,
	image_height: u32,
	display_frame: RsnapFloatRect,
	selection: RsnapFloatRect,
	out_rect: *mut RsnapPixelRect,
) -> RsnapStatus {
	if out_rect.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(crop_rect) = rsnap_capture_core::frozen_display_crop_rect(
		image_width,
		image_height,
		crate::decode_float_rect(display_frame),
		crate::decode_float_rect(selection),
	) else {
		return RsnapStatus::Empty;
	};

	unsafe {
		ptr::write(out_rect, crate::encode_pixel_rect(crop_rect));
	}

	RsnapStatus::Ok
}

/// Builds a light privacy mosaic patch as row-major RGBA bytes.
///
/// # Safety
///
/// `out_region` must be writable. The returned buffer must be released with
/// `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_mosaic_light_privacy_patch_rgba(
	image_width: u32,
	image_height: u32,
	source_rect: RsnapFloatRect,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(patch) = rsnap_capture_core::frozen_mosaic_light_privacy_patch(
		image_width,
		image_height,
		crate::decode_float_rect(source_rect),
	) else {
		return RsnapStatus::Empty;
	};
	let (width, height) = patch.dimensions();

	unsafe {
		ptr::write(out_region, crate::owned_region_from_raw_rgba(width, height, patch.into_raw()));
	}

	RsnapStatus::Ok
}

/// Samples an RGB value from a borrowed BGRA frame.
///
/// # Safety
///
/// `bgra` must point to `bgra_len` readable bytes while this function runs, and
/// `out_rgb` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_bgra_frame_sample_rgb(
	width: u32,
	height: u32,
	bytes_per_row: usize,
	bgra: *const u8,
	bgra_len: usize,
	display_frame: RsnapFloatRect,
	point_x: f64,
	point_y: f64,
	out_rgb: *mut RsnapRgb,
) -> RsnapStatus {
	if out_rgb.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(frame) = (unsafe { decode_bgra_frame(width, height, bytes_per_row, bgra, bgra_len) })
	else {
		return RsnapStatus::InvalidInput;
	};
	let Some(rgb) = rsnap_capture_core::sample_rgb_from_bgra_frame(
		frame,
		crate::decode_float_rect(display_frame),
		point_x,
		point_y,
	) else {
		return RsnapStatus::Empty;
	};

	unsafe {
		ptr::write(out_rgb, RsnapRgb { r: rgb.r, g: rgb.g, b: rgb.b });
	}

	RsnapStatus::Ok
}

/// Builds a square RGBA loupe patch from a borrowed BGRA frame.
///
/// # Safety
///
/// `bgra` must point to `bgra_len` readable bytes while this function runs. `out_region`
/// must be writable, and the returned buffer must be released with
/// `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_bgra_frame_loupe_patch_rgba(
	width: u32,
	height: u32,
	bytes_per_row: usize,
	bgra: *const u8,
	bgra_len: usize,
	display_frame: RsnapFloatRect,
	point_x: f64,
	point_y: f64,
	side_pixels: u32,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(frame) = (unsafe { decode_bgra_frame(width, height, bytes_per_row, bgra, bgra_len) })
	else {
		return RsnapStatus::InvalidInput;
	};
	let Some(patch) = rsnap_capture_core::loupe_patch_rgba_from_bgra_frame(
		frame,
		crate::decode_float_rect(display_frame),
		point_x,
		point_y,
		side_pixels,
	) else {
		unsafe {
			ptr::write(out_region, RsnapOwnedRgbaRegion::default());
		}

		return RsnapStatus::Empty;
	};
	let (width, height) = patch.dimensions();

	unsafe {
		ptr::write(out_region, crate::owned_region_from_raw_rgba(width, height, patch.into_raw()));
	}

	RsnapStatus::Ok
}

/// Resolves capture-frame layout and shadow parameters.
///
/// # Safety
///
/// `out_plan` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_capture_frame_plan(
	image_width: u32,
	image_height: u32,
	screen_scale_factor: f64,
	source_kind: RsnapCaptureFrameSourceKind,
	out_plan: *mut RsnapCaptureFramePlan,
) -> RsnapStatus {
	if out_plan.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(plan) = rsnap_capture_core::capture_frame_plan(
		image_width,
		image_height,
		screen_scale_factor,
		decode_capture_frame_source_kind(source_kind),
	) else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_plan, encode_capture_frame_plan(plan));
	}

	RsnapStatus::Ok
}

/// Resolves the source crop rect for aspect-fill drawing.
///
/// # Safety
///
/// `out_rect` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_capture_frame_aspect_fill_crop_rect(
	source_width: u32,
	source_height: u32,
	destination_width: f64,
	destination_height: f64,
	out_rect: *mut RsnapFloatRect,
) -> RsnapStatus {
	if out_rect.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(rect) = rsnap_capture_core::capture_frame_aspect_fill_crop_rect(
		source_width,
		source_height,
		destination_width,
		destination_height,
	) else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_rect, crate::encode_float_rect(rect));
	}

	RsnapStatus::Ok
}

/// Resolves capture-frame background colors and wallpaper fallback behavior.
///
/// # Safety
///
/// `out_plan` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_capture_frame_background_plan(
	background_kind: RsnapCaptureFrameBackgroundKind,
	out_plan: *mut RsnapCaptureFrameBackgroundPlan,
) -> RsnapStatus {
	if out_plan.is_null() {
		return RsnapStatus::NullOutput;
	}

	let plan = rsnap_capture_core::capture_frame_background_plan(
		decode_capture_frame_background_kind(background_kind),
	);

	unsafe {
		ptr::write(out_plan, encode_capture_frame_background_plan(plan));
	}

	RsnapStatus::Ok
}

/// Resolves a platform wallpaper thumbnail request for a capture-frame destination.
///
/// # Safety
///
/// `out_request` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_capture_frame_wallpaper_request_plan(
	background_kind: RsnapCaptureFrameBackgroundKind,
	destination_width: f64,
	destination_height: f64,
	out_request: *mut RsnapCaptureFrameWallpaperRequest,
) -> RsnapStatus {
	if out_request.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(request) = rsnap_capture_core::capture_frame_wallpaper_request_plan(
		decode_capture_frame_background_kind(background_kind),
		destination_width,
		destination_height,
	) else {
		return RsnapStatus::Empty;
	};

	unsafe {
		ptr::write(out_request, encode_capture_frame_wallpaper_request(request));
	}

	RsnapStatus::Ok
}

/// Decodes a PNG wallpaper thumbnail through Rust's streaming low-memory cached path.
///
/// Non-PNG paths and decode failures return `Empty` so native hosts can skip wallpaper drawing.
///
/// # Safety
///
/// `path` must be a valid null-terminated UTF-8 string, and `out_region` must be a valid writable
/// pointer. When `Ok` is returned, the caller must release the returned buffer with
/// `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_capture_frame_wallpaper_png_thumbnail(
	path: *const c_char,
	target_pixel_size: u32,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	unsafe {
		ptr::write(out_region, RsnapOwnedRgbaRegion::default());
	}

	if path.is_null() || target_pixel_size == 0 {
		return RsnapStatus::InvalidInput;
	}

	let Ok(path) = (unsafe { CStr::from_ptr(path) }).to_str() else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(Some(thumbnail)) =
		rsnap_capture_core::capture_frame_wallpaper_png_thumbnail_cached(path, target_pixel_size)
	else {
		return RsnapStatus::Empty;
	};
	let image = thumbnail.into_image();
	let out = crate::owned_region_from_raw_rgba(image.width(), image.height(), image.into_raw());

	unsafe {
		ptr::write(out_region, out);
	}

	RsnapStatus::Ok
}

/// Renders the complete capture-frame effect as Rust-owned RGBA bytes.
///
/// Swift/native hosts only pass source pixels and an optional platform wallpaper path. Rust owns
/// wallpaper thumbnail planning/cache/decode, background drawing, shadows, clipping, and final
/// composition.
///
/// # Safety
///
/// `source_rgba` must point to `source_rgba_len` readable bytes containing
/// `source_width * source_height * 4` row-major RGBA data. `wallpaper_path` may be null or a valid
/// null-terminated UTF-8 string. `out_region` must be writable, and the returned buffer must be
/// released with `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_capture_frame_render_rgba(
	source_width: u32,
	source_height: u32,
	source_rgba: *const u8,
	source_rgba_len: usize,
	screen_scale_factor: f64,
	source_kind: RsnapCaptureFrameSourceKind,
	background_kind: RsnapCaptureFrameBackgroundKind,
	render_kind: RsnapCaptureFrameRenderKind,
	wallpaper_path: *const c_char,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	unsafe {
		ptr::write(out_region, RsnapOwnedRgbaRegion::default());
	}

	let Some(source_bytes) = (unsafe { crate::rgba_bytes(source_rgba, source_rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(source) = CaptureFrameRenderImageRef::new(source_width, source_height, source_bytes)
	else {
		return RsnapStatus::InvalidInput;
	};
	let background_kind = decode_capture_frame_background_kind(background_kind);
	let source_kind = decode_capture_frame_source_kind(source_kind);
	let render_kind = decode_capture_frame_render_kind(render_kind);
	let wallpaper = match unsafe {
		capture_frame_wallpaper_for_render(
			source,
			screen_scale_factor,
			source_kind,
			background_kind,
			wallpaper_path,
		)
	} {
		Ok(wallpaper) => wallpaper,
		Err(_err) => return RsnapStatus::InvalidInput,
	};
	let wallpaper_ref = wallpaper.as_ref().map(CaptureFrameRenderImageRef::from_export);
	let Ok(Some(rendered)) = rsnap_capture_core::render_capture_frame_effect(
		source,
		background_kind,
		screen_scale_factor,
		source_kind,
		render_kind,
		wallpaper_ref,
	) else {
		return RsnapStatus::InvalidInput;
	};
	let image = rendered.into_image();

	unsafe {
		ptr::write(
			out_region,
			crate::owned_region_from_raw_rgba(image.width(), image.height(), image.into_raw()),
		);
	}

	RsnapStatus::Ok
}

unsafe fn decode_bgra_frame<'a>(
	width: u32,
	height: u32,
	bytes_per_row: usize,
	bgra: *const u8,
	bgra_len: usize,
) -> Option<BgraFrameView<'a>> {
	if bgra.is_null() {
		return None;
	}

	let bytes = unsafe { slice::from_raw_parts(bgra, bgra_len) };
	let frame = BgraFrameView { width, height, bytes_per_row, bytes };

	frame.is_valid().then_some(frame)
}

fn decode_capture_frame_source_kind(kind: RsnapCaptureFrameSourceKind) -> CaptureFrameSourceKind {
	match kind {
		RsnapCaptureFrameSourceKind::DragRegion => CaptureFrameSourceKind::DragRegion,
		RsnapCaptureFrameSourceKind::Window => CaptureFrameSourceKind::Window,
		RsnapCaptureFrameSourceKind::FullScreen => CaptureFrameSourceKind::FullScreen,
		RsnapCaptureFrameSourceKind::ScrollCapture => CaptureFrameSourceKind::ScrollCapture,
		RsnapCaptureFrameSourceKind::Unknown => CaptureFrameSourceKind::Unknown,
	}
}

fn decode_capture_frame_background_kind(
	kind: RsnapCaptureFrameBackgroundKind,
) -> CaptureFrameBackgroundKind {
	match kind {
		RsnapCaptureFrameBackgroundKind::SystemWallpaper => {
			CaptureFrameBackgroundKind::SystemWallpaper
		},
		RsnapCaptureFrameBackgroundKind::Aurora => CaptureFrameBackgroundKind::Aurora,
		RsnapCaptureFrameBackgroundKind::Graphite => CaptureFrameBackgroundKind::Graphite,
		RsnapCaptureFrameBackgroundKind::Linen => CaptureFrameBackgroundKind::Linen,
	}
}

fn decode_capture_frame_render_kind(kind: RsnapCaptureFrameRenderKind) -> CaptureFrameRenderKind {
	match kind {
		RsnapCaptureFrameRenderKind::FramedCapture => CaptureFrameRenderKind::FramedCapture,
		RsnapCaptureFrameRenderKind::WindowSnapshot => CaptureFrameRenderKind::WindowSnapshot,
	}
}

unsafe fn capture_frame_wallpaper_for_render(
	source: CaptureFrameRenderImageRef<'_>,
	screen_scale_factor: f64,
	source_kind: CaptureFrameSourceKind,
	background_kind: CaptureFrameBackgroundKind,
	wallpaper_path: *const c_char,
) -> Result<Option<RgbaExportImage>, ()> {
	if wallpaper_path.is_null() {
		return Ok(None);
	}

	let Some(plan) = rsnap_capture_core::capture_frame_plan(
		source.width(),
		source.height(),
		screen_scale_factor,
		source_kind,
	) else {
		return Ok(None);
	};
	let Some(request) = rsnap_capture_core::capture_frame_wallpaper_request_plan(
		background_kind,
		plan.canvas_width,
		plan.canvas_height,
	) else {
		return Ok(None);
	};
	let path = unsafe { CStr::from_ptr(wallpaper_path) }.to_str().map_err(|_| ())?;

	match rsnap_capture_core::capture_frame_wallpaper_png_thumbnail_cached(
		path,
		request.target_pixel_size,
	) {
		Ok(thumbnail) => Ok(thumbnail),
		Err(_err) => Ok(None),
	}
}

fn encode_capture_frame_plan(plan: CaptureFramePlan) -> RsnapCaptureFramePlan {
	RsnapCaptureFramePlan {
		canvas_width: plan.canvas_width,
		canvas_height: plan.canvas_height,
		image_rect: crate::encode_float_rect(plan.image_rect),
		corner_radius: plan.corner_radius,
		shadows: plan.shadows.map(encode_capture_frame_shadow),
	}
}

fn encode_capture_frame_background_plan(
	plan: CaptureFrameBackgroundPlan,
) -> RsnapCaptureFrameBackgroundPlan {
	RsnapCaptureFrameBackgroundPlan {
		colors: plan.colors.map(encode_capture_frame_color_stop),
		locations: plan.locations,
		prefers_wallpaper: u8::from(plan.prefers_wallpaper),
		wallpaper_overlay_alpha: plan.wallpaper_overlay_alpha,
	}
}

fn encode_capture_frame_color_stop(color: CaptureFrameColorStop) -> RsnapCaptureFrameColorStop {
	RsnapCaptureFrameColorStop {
		red: color.red,
		green: color.green,
		blue: color.blue,
		alpha: color.alpha,
	}
}

fn encode_capture_frame_shadow(shadow: CaptureFrameShadow) -> RsnapCaptureFrameShadow {
	RsnapCaptureFrameShadow {
		offset_x: shadow.offset_x,
		offset_y: shadow.offset_y,
		blur: shadow.blur,
		alpha: shadow.alpha,
	}
}

fn encode_capture_frame_wallpaper_request(
	request: CaptureFrameWallpaperRequest,
) -> RsnapCaptureFrameWallpaperRequest {
	RsnapCaptureFrameWallpaperRequest {
		target_pixel_size: request.target_pixel_size,
		overlay_alpha: request.overlay_alpha,
	}
}

fn owned_bytes_from_vec(mut bytes: Vec<u8>) -> RsnapOwnedBytes {
	let out =
		RsnapOwnedBytes { len: bytes.len(), capacity: bytes.capacity(), bytes: bytes.as_mut_ptr() };

	mem::forget(bytes);

	out
}
