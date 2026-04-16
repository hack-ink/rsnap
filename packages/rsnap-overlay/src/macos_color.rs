#[cfg(target_os = "macos")]
use std::ffi::c_void;
#[cfg(target_os = "macos")]
use std::ptr;
#[cfg(target_os = "macos")]
use std::slice;

#[cfg(target_os = "macos")]
use image::RgbaImage;
#[cfg(target_os = "macos")]
use objc2_core_foundation::{CFData, CFRetained, CGPoint, CGRect, CGSize};
#[cfg(target_os = "macos")]
use objc2_core_graphics::{
	CGBitmapContextCreate, CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGContext,
	CGDataProvider, CGDirectDisplayID, CGDisplayCopyColorSpace, CGImage, CGImageAlphaInfo,
	CGImageByteOrderInfo, kCGColorSpaceSRGB,
};
#[cfg(target_os = "macos")]
use objc2_core_video::{
	CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
	CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
	kCVImageBufferCGColorSpaceKey, kCVReturnSuccess,
};

#[cfg(target_os = "macos")]
fn srgb_color_space() -> Option<CFRetained<CGColorSpace>> {
	CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
		.or_else(CGColorSpace::new_device_rgb)
}

#[cfg(target_os = "macos")]
fn monitor_color_space(display_id: Option<u32>) -> Option<CFRetained<CGColorSpace>> {
	display_id.map(|display_id| CGDisplayCopyColorSpace(display_id as CGDirectDisplayID))
}

#[cfg(target_os = "macos")]
fn pixel_buffer_color_space(
	pixel_buffer: &CFRetained<CVPixelBuffer>,
) -> Option<CFRetained<CGColorSpace>> {
	let attachment =
		unsafe { pixel_buffer.attachment(kCVImageBufferCGColorSpaceKey, ptr::null_mut()) }?;

	attachment.downcast::<CGColorSpace>().ok()
}

#[cfg(target_os = "macos")]
fn color_managed_rgba_from_bgra_bytes(
	width: usize,
	height: usize,
	bytes_per_row: usize,
	data: &[u8],
	src_color_space: Option<&CGColorSpace>,
	display_id: Option<u32>,
	_src_bitmap_info: CGBitmapInfo,
) -> Option<RgbaImage> {
	if width == 0 || height == 0 {
		return None;
	}

	let expected_row_bytes = width.checked_mul(4)?;
	if bytes_per_row < expected_row_bytes {
		return None;
	}

	let required_len = height.checked_mul(bytes_per_row)?;
	if data.len() < required_len {
		return None;
	}

	let fallback_monitor_space = monitor_color_space(display_id);
	let fallback_srgb_space = srgb_color_space();
	let src_space =
		src_color_space.or(fallback_monitor_space.as_deref()).or(fallback_srgb_space.as_deref())?;
	let dst_space = srgb_color_space()?;
	let normalized_src_bitmap_info =
		CGBitmapInfo(CGImageAlphaInfo::NoneSkipFirst.0 | CGImageByteOrderInfo::Order32Little.0);
	let provider =
		CGDataProvider::with_cf_data(Some(CFData::from_bytes(&data[..required_len]).as_ref()))?;
	let source = unsafe {
		CGImage::new(
			width,
			height,
			8,
			32,
			bytes_per_row,
			Some(src_space),
			normalized_src_bitmap_info,
			Some(provider.as_ref()),
			ptr::null(),
			false,
			CGColorRenderingIntent::RenderingIntentDefault,
		)
	}?;
	let mut dst = vec![0_u8; width.checked_mul(height)?.checked_mul(4)?];
	let dst_bytes_per_row = width.checked_mul(4)?;
	let context = unsafe {
		CGBitmapContextCreate(
			dst.as_mut_ptr().cast::<c_void>(),
			width,
			height,
			8,
			dst_bytes_per_row,
			Some(dst_space.as_ref()),
			(CGBitmapInfo(
				CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0,
			))
			.0,
		)
	}?;
	let draw_rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width as f64, height as f64));

	CGContext::draw_image(Some(context.as_ref()), draw_rect, Some(source.as_ref()));

	for bgra in dst.chunks_exact_mut(4) {
		bgra.swap(0, 2);
	}

	RgbaImage::from_raw(width as u32, height as u32, dst)
}

#[cfg(target_os = "macos")]
pub(crate) fn rgba_image_from_pixel_buffer_color_managed(
	pixel_buffer: &CFRetained<CVPixelBuffer>,
	width_px: u32,
	height_px: u32,
	display_id: Option<u32>,
) -> Option<RgbaImage> {
	let lock_result =
		unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	if lock_result != kCVReturnSuccess {
		return None;
	}

	let out = (|| {
		let base = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;
		if base.is_null() {
			return None;
		}

		let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
		let height = height_px.max(1) as usize;
		let byte_len = height.checked_mul(bytes_per_row)?;
		let bytes = unsafe { slice::from_raw_parts(base, byte_len) };
		let src_color_space = pixel_buffer_color_space(pixel_buffer);
		let src_bitmap_info = CGBitmapInfo(
			CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0,
		);

		color_managed_rgba_from_bgra_bytes(
			width_px.max(1) as usize,
			height,
			bytes_per_row,
			bytes,
			src_color_space.as_deref(),
			display_id,
			src_bitmap_info,
		)
	})();
	let _ =
		unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	out
}

#[cfg(target_os = "macos")]
pub(crate) fn rgba_image_from_cg_image_color_managed(
	cg_image: &CGImage,
	display_id: Option<u32>,
) -> Option<RgbaImage> {
	let width = CGImage::width(Some(cg_image));
	let height = CGImage::height(Some(cg_image));

	if width == 0 || height == 0 {
		return None;
	}

	let data_provider = CGImage::data_provider(Some(cg_image))?;
	let data = objc2_core_graphics::CGDataProvider::data(Some(data_provider.as_ref()))?;
	let bytes = data.to_vec();
	let bytes_per_row = CGImage::bytes_per_row(Some(cg_image));
	let src_color_space = CGImage::color_space(Some(cg_image));
	let src_bitmap_info = CGImage::bitmap_info(Some(cg_image));

	color_managed_rgba_from_bgra_bytes(
		width,
		height,
		bytes_per_row,
		&bytes,
		src_color_space.as_deref(),
		display_id,
		src_bitmap_info,
	)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
	use image::{Rgba, RgbaImage};
	use objc2_core_foundation::{CFData, CGPoint, CGRect, CGSize};
	use objc2_core_graphics::{
		CGBitmapContextCreate, CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGContext,
		CGDataProvider, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo,
	};

	use crate::macos_color::color_managed_rgba_from_bgra_bytes;

	#[test]
	fn color_managed_bgra_to_rgba_preserves_srgb_identity() {
		let src_space =
			CGColorSpace::with_name(Some(unsafe { objc2_core_graphics::kCGColorSpaceSRGB }))
				.expect("sRGB");
		let src = [
			10_u8, 20, 30, 255, // BGRA for RGBA(30,20,10,255)
			90, 80, 70, 255, // BGRA for RGBA(70,80,90,255)
		];
		let image = color_managed_rgba_from_bgra_bytes(
			2,
			1,
			8,
			&src,
			Some(src_space.as_ref()),
			None,
			objc2_core_graphics::CGBitmapInfo(
				CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0,
			),
		)
		.expect("converted image");

		let expected = RgbaImage::from_vec(
			2,
			1,
			vec![
				Rgba([30, 20, 10, 255]).0[0],
				Rgba([30, 20, 10, 255]).0[1],
				Rgba([30, 20, 10, 255]).0[2],
				Rgba([30, 20, 10, 255]).0[3],
				Rgba([70, 80, 90, 255]).0[0],
				Rgba([70, 80, 90, 255]).0[1],
				Rgba([70, 80, 90, 255]).0[2],
				Rgba([70, 80, 90, 255]).0[3],
			],
		)
		.expect("expected image");

		assert_eq!(image, expected);
	}

	#[test]
	fn quartz_color_managed_path_constructs_source_and_context() {
		let src_space =
			CGColorSpace::with_name(Some(unsafe { objc2_core_graphics::kCGColorSpaceSRGB }))
				.expect("sRGB");
		let src = [
			10_u8, 20, 30, 255, // BGRA for RGBA(30,20,10,255)
			90, 80, 70, 255, // BGRA for RGBA(70,80,90,255)
		];
		let provider = CGDataProvider::with_cf_data(Some(CFData::from_bytes(&src).as_ref()))
			.expect("provider");
		let source = unsafe {
			CGImage::new(
				2,
				1,
				8,
				32,
				8,
				Some(src_space.as_ref()),
				CGBitmapInfo(
					CGImageAlphaInfo::NoneSkipFirst.0 | CGImageByteOrderInfo::Order32Little.0,
				),
				Some(provider.as_ref()),
				std::ptr::null(),
				false,
				CGColorRenderingIntent::RenderingIntentDefault,
			)
		}
		.expect("source image");
		let dst_space =
			CGColorSpace::with_name(Some(unsafe { objc2_core_graphics::kCGColorSpaceSRGB }))
				.expect("dst sRGB");
		let mut dst = vec![0_u8; 8];
		let context = unsafe {
			CGBitmapContextCreate(
				dst.as_mut_ptr().cast(),
				2,
				1,
				8,
				8,
				Some(dst_space.as_ref()),
				(CGBitmapInfo(
					CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0,
				))
				.0,
			)
		}
		.expect("bitmap context");
		let draw_rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(2.0, 1.0));

		CGContext::save_g_state(Some(context.as_ref()));
		CGContext::translate_ctm(Some(context.as_ref()), 0.0, 1.0);
		CGContext::scale_ctm(Some(context.as_ref()), 1.0, -1.0);
		CGContext::draw_image(Some(context.as_ref()), draw_rect, Some(source.as_ref()));
		CGContext::restore_g_state(Some(context.as_ref()));

		for bgra in dst.chunks_exact_mut(4) {
			bgra.swap(0, 2);
		}

		assert_eq!(dst, vec![30, 20, 10, 255, 70, 80, 90, 255]);
	}

	#[test]
	fn color_managed_path_preserves_row_order_without_vertical_flip() {
		let src_space =
			CGColorSpace::with_name(Some(unsafe { objc2_core_graphics::kCGColorSpaceSRGB }))
				.expect("sRGB");
		let src = [
			10_u8, 20, 30, 255, 40, 50, 60, 255, // top row, BGRA
			70, 80, 90, 255, 100, 110, 120, 255, // bottom row, BGRA
		];
		let image = color_managed_rgba_from_bgra_bytes(
			2,
			2,
			8,
			&src,
			Some(src_space.as_ref()),
			None,
			objc2_core_graphics::CGBitmapInfo(
				CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0,
			),
		)
		.expect("converted image");

		let expected = RgbaImage::from_vec(
			2,
			2,
			vec![
				30, 20, 10, 255, 60, 50, 40, 255, // top row
				90, 80, 70, 255, 120, 110, 100, 255, // bottom row
			],
		)
		.expect("expected image");

		assert_eq!(image, expected);
	}
}
