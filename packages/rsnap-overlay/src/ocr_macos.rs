use color_eyre::eyre::{Result, WrapErr};
use image::RgbaImage;
use objc2::rc::{self, Retained};
use objc2::runtime::AnyObject;
use objc2::{AnyThread, ClassType};
use objc2_foundation::{NSArray, NSData, NSDictionary};
use objc2_vision::{
	VNImageOption, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest,
	VNRequestTextRecognitionLevel,
};

use crate::png;

pub(crate) fn recognize_text_from_image(image: &RgbaImage) -> Result<String> {
	rc::autoreleasepool(|_| {
		let image_data = NSData::with_bytes(
			&png::rgba_image_to_png_bytes(image).wrap_err("failed to encode OCR source image")?,
		);
		let options: Retained<NSDictionary<VNImageOption, AnyObject>> = NSDictionary::new();
		let request_handler = VNImageRequestHandler::initWithData_options(
			VNImageRequestHandler::alloc(),
			&image_data,
			&options,
		);
		let request = VNRecognizeTextRequest::new();

		request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
		request.setUsesLanguageCorrection(true);
		request.setAutomaticallyDetectsLanguage(true);

		let requests: Retained<NSArray<VNRequest>> =
			NSArray::from_slice(&[request.as_super().as_super()]);

		request_handler
			.performRequests_error(&requests)
			.wrap_err("Vision text recognition request failed")?;

		let mut lines = Vec::new();

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

		Ok(lines.join("\n"))
	})
}
