use std::{
	ptr,
	time::{Duration, Instant},
};

use color_eyre::eyre::{self, Result, WrapErr};
use image::RgbaImage;
use objc2::rc::{self, Retained};
use objc2::runtime::AnyObject;
use objc2::{AnyThread, ClassType};
use objc2_core_foundation::{CFData, CFRetained};
use objc2_core_graphics::{
	CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
	CGImageByteOrderInfo,
};
use objc2_foundation::{NSArray, NSDictionary};
use objc2_vision::{
	VNImageOption, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest,
	VNRequestTextRecognitionLevel,
};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OcrRecognizeTimings {
	pub(crate) cg_image: Duration,
	pub(crate) vision_request: Duration,
	pub(crate) extract_results: Duration,
	pub(crate) total: Duration,
}

#[derive(Debug)]
pub(crate) struct RecognizedTextOutput {
	pub(crate) text: String,
	pub(crate) line_count: usize,
	pub(crate) timings: OcrRecognizeTimings,
}

pub(crate) fn recognize_text_from_image(image: &RgbaImage) -> Result<RecognizedTextOutput> {
	let recognize_started_at = Instant::now();

	rc::autoreleasepool(|_| {
		let cg_image_started_at = Instant::now();
		let cg_image = cg_image_from_rgba_image(image)?;
		let cg_image_elapsed = cg_image_started_at.elapsed();
		let options: Retained<NSDictionary<VNImageOption, AnyObject>> = NSDictionary::new();
		let request_handler = unsafe {
			VNImageRequestHandler::initWithCGImage_options(
				VNImageRequestHandler::alloc(),
				cg_image.as_ref(),
				&options,
			)
		};
		let request = VNRecognizeTextRequest::new();

		request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
		request.setUsesLanguageCorrection(true);
		request.setAutomaticallyDetectsLanguage(true);

		let requests: Retained<NSArray<VNRequest>> =
			NSArray::from_slice(&[request.as_super().as_super()]);

		let vision_request_started_at = Instant::now();
		request_handler
			.performRequests_error(&requests)
			.wrap_err("Vision text recognition request failed")?;
		let vision_request_elapsed = vision_request_started_at.elapsed();

		let mut lines = Vec::new();
		let extract_results_started_at = Instant::now();

		if let Some(results) = request.results() {
			for index in 0..results.count() {
				let observation = results.objectAtIndex(index);
				let candidates = observation.topCandidates(1);
				let Some(candidate) = candidates.firstObject() else {
					continue;
				};
				let line = candidate.string().to_string();

				if !line.trim().is_empty() {
					lines.push(line);
				}
			}
		}

		let line_count = lines.len();
		let extract_results_elapsed = extract_results_started_at.elapsed();

		Ok(RecognizedTextOutput {
			text: lines.join("\n"),
			line_count,
			timings: OcrRecognizeTimings {
				cg_image: cg_image_elapsed,
				vision_request: vision_request_elapsed,
				extract_results: extract_results_elapsed,
				total: recognize_started_at.elapsed(),
			},
		})
	})
}

fn cg_image_from_rgba_image(image: &RgbaImage) -> Result<CFRetained<CGImage>> {
	let width = image.width() as usize;
	let height = image.height() as usize;

	if width == 0 || height == 0 {
		return Err(eyre::eyre!("OCR source image has zero dimensions"));
	}

	let bytes = CFData::from_bytes(image.as_raw());
	let provider = CGDataProvider::with_cf_data(Some(bytes.as_ref()))
		.ok_or_else(|| eyre::eyre!("failed to create CGDataProvider for OCR image"))?;
	let color_space = CGColorSpace::new_device_rgb()
		.ok_or_else(|| eyre::eyre!("failed to create RGB colorspace for OCR image"))?;
	let bitmap_info = CGBitmapInfo(CGImageAlphaInfo::Last.0 | CGImageByteOrderInfo::Order32Big.0);

	unsafe {
		CGImage::new(
			width,
			height,
			8,
			32,
			width.saturating_mul(4),
			Some(color_space.as_ref()),
			bitmap_info,
			Some(provider.as_ref()),
			ptr::null(),
			false,
			CGColorRenderingIntent::RenderingIntentDefault,
		)
	}
	.ok_or_else(|| eyre::eyre!("failed to create CGImage for OCR image"))
}

#[cfg(test)]
mod tests {
	use image::Rgba;
	use objc2_core_graphics::CGImage;

	use super::cg_image_from_rgba_image;
	use crate::ocr_macos::RgbaImage;

	#[test]
	fn cg_image_bridge_preserves_dimensions() {
		let image = RgbaImage::from_pixel(7, 5, Rgba([1, 2, 3, 255]));
		let cg_image = cg_image_from_rgba_image(&image).expect("cg image");

		assert_eq!(CGImage::width(Some(cg_image.as_ref())), 7);
		assert_eq!(CGImage::height(Some(cg_image.as_ref())), 5);
	}
}
