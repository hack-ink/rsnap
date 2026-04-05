#[cfg(target_os = "macos")]
use std::time::Instant;

#[cfg(target_os = "macos")]
use image::{Rgba, RgbaImage, imageops};

#[cfg(target_os = "macos")]
use crate::state::RectPoints;

#[cfg(target_os = "macos")]
const WINDOW_CAPTURE_MATTE_LIGHT_RGBA: Rgba<u8> = Rgba([246, 246, 246, 255]);
#[cfg(target_os = "macos")]
const WINDOW_CAPTURE_MATTE_DARK_RGBA: Rgba<u8> = Rgba([24, 24, 24, 255]);

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeferredTextRecognitionWindowMatte {
	Light,
	Dark,
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub(crate) enum DeferredTextRecognitionImageSource {
	Prepared { image: RgbaImage },
	FrozenCrop { frozen_image: RgbaImage, crop_rect: Option<RectPoints> },
	WindowImageWithMatte { window_image: RgbaImage, matte: DeferredTextRecognitionWindowMatte },
}

use crate::{ocr_macos, overlay::output};

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub struct DeferredTextRecognitionRequest {
	pub request_id: u64,
	pub requested_at: Instant,
	pub(crate) image_source: DeferredTextRecognitionImageSource,
}

#[cfg(target_os = "macos")]
impl DeferredTextRecognitionRequest {
	pub(crate) fn prepared(request_id: u64, requested_at: Instant, image: RgbaImage) -> Self {
		Self {
			request_id,
			requested_at,
			image_source: DeferredTextRecognitionImageSource::Prepared { image },
		}
	}

	pub(crate) fn frozen_crop(
		request_id: u64,
		requested_at: Instant,
		frozen_image: RgbaImage,
		crop_rect: Option<RectPoints>,
	) -> Self {
		Self {
			request_id,
			requested_at,
			image_source: DeferredTextRecognitionImageSource::FrozenCrop {
				frozen_image,
				crop_rect,
			},
		}
	}

	pub(crate) fn window_image_with_matte(
		request_id: u64,
		requested_at: Instant,
		window_image: RgbaImage,
		matte: DeferredTextRecognitionWindowMatte,
	) -> Self {
		Self {
			request_id,
			requested_at,
			image_source: DeferredTextRecognitionImageSource::WindowImageWithMatte {
				window_image,
				matte,
			},
		}
	}

	pub(crate) fn image_dimensions(&self) -> (u32, u32) {
		self.image_source.image_dimensions()
	}

	#[cfg(test)]
	pub(crate) fn export_image(&self) -> Option<RgbaImage> {
		self.image_source.export_image()
	}
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferredTextRecognitionOutcomeKind {
	TextCopied,
	NoText,
	ClipboardError,
	RecognizeError,
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub struct DeferredTextRecognitionOutcome {
	pub request_id: u64,
	pub kind: DeferredTextRecognitionOutcomeKind,
	pub recognized_lines: usize,
	pub recognized_chars: usize,
}

#[cfg(target_os = "macos")]
pub fn process_deferred_text_recognition(
	request: DeferredTextRecognitionRequest,
) -> DeferredTextRecognitionOutcome {
	let request_id = request.request_id;
	let requested_at = request.requested_at;
	let worker_started_at = Instant::now();
	let queue_delay = worker_started_at.saturating_duration_since(requested_at);
	let export_prepare_started_at = Instant::now();
	let Some(export_image) = request.image_source.into_export_image() else {
		let error = String::from("OCR export source resolved to an empty image");

		tracing::warn!(
			target: "rsnap",
			op = "overlay.ocr_phase_timing",
			request_id,
			queue_delay_ms = queue_delay.as_millis(),
			export_prepare_ms = export_prepare_started_at.elapsed().as_millis(),
			total_ms = worker_started_at.elapsed().as_millis(),
			error = %error,
			"OCR request failed before Vision processing."
		);

		log_ocr_request_completed(
			request_id,
			requested_at,
			"recognize_error",
			0,
			0,
			None,
			Some(error.as_str()),
		);

		return DeferredTextRecognitionOutcome {
			request_id,
			kind: DeferredTextRecognitionOutcomeKind::RecognizeError,
			recognized_lines: 0,
			recognized_chars: 0,
		};
	};
	let export_prepare_elapsed = export_prepare_started_at.elapsed();
	let image_width_px = export_image.width();
	let image_height_px = export_image.height();

	match ocr_macos::recognize_text_from_image(&export_image) {
		Ok(output) => {
			let recognized_lines = output.line_count;
			let recognized_chars = output.text.chars().count();

			tracing::info!(
				target: "rsnap",
				op = "overlay.ocr_phase_timing",
				request_id,
				image_width_px,
				image_height_px,
				image_pixels = u64::from(image_width_px) * u64::from(image_height_px),
				queue_delay_ms = queue_delay.as_millis(),
				export_prepare_ms = export_prepare_elapsed.as_millis(),
				cg_image_ms = output.timings.cg_image.as_millis(),
				vision_request_ms = output.timings.vision_request.as_millis(),
				extract_results_ms = output.timings.extract_results.as_millis(),
				total_ms = output.timings.total.as_millis(),
				recognized_lines,
				recognized_chars,
				"OCR phase timing."
			);

			if output.text.trim().is_empty() {
				log_ocr_request_completed(
					request_id,
					requested_at,
					"no_text",
					recognized_lines,
					recognized_chars,
					None,
					None,
				);

				return DeferredTextRecognitionOutcome {
					request_id,
					kind: DeferredTextRecognitionOutcomeKind::NoText,
					recognized_lines,
					recognized_chars,
				};
			}

			let clipboard_write_started_at = Instant::now();

			match output::write_text_to_clipboard(&output.text) {
				Ok(()) => {
					log_ocr_request_completed(
						request_id,
						requested_at,
						"text_copied",
						recognized_lines,
						recognized_chars,
						Some(clipboard_write_started_at.elapsed().as_millis()),
						None,
					);

					DeferredTextRecognitionOutcome {
						request_id,
						kind: DeferredTextRecognitionOutcomeKind::TextCopied,
						recognized_lines,
						recognized_chars,
					}
				},
				Err(err) => {
					let error = format!("{err:#}");

					log_ocr_request_completed(
						request_id,
						requested_at,
						"clipboard_error",
						recognized_lines,
						recognized_chars,
						Some(clipboard_write_started_at.elapsed().as_millis()),
						Some(error.as_str()),
					);

					DeferredTextRecognitionOutcome {
						request_id,
						kind: DeferredTextRecognitionOutcomeKind::ClipboardError,
						recognized_lines,
						recognized_chars,
					}
				},
			}
		},
		Err(err) => {
			let error = format!("{err:#}");

			tracing::warn!(
				target: "rsnap",
				op = "overlay.ocr_phase_timing",
				request_id,
				image_width_px,
				image_height_px,
				image_pixels = u64::from(image_width_px) * u64::from(image_height_px),
				queue_delay_ms = queue_delay.as_millis(),
				export_prepare_ms = export_prepare_elapsed.as_millis(),
				total_ms = worker_started_at.elapsed().as_millis(),
				error = %error,
				"OCR request failed."
			);

			log_ocr_request_completed(
				request_id,
				requested_at,
				"recognize_error",
				0,
				0,
				None,
				Some(error.as_str()),
			);

			DeferredTextRecognitionOutcome {
				request_id,
				kind: DeferredTextRecognitionOutcomeKind::RecognizeError,
				recognized_lines: 0,
				recognized_chars: 0,
			}
		},
	}
}

#[cfg(target_os = "macos")]
impl DeferredTextRecognitionImageSource {
	fn image_dimensions(&self) -> (u32, u32) {
		match self {
			Self::Prepared { image } => image.dimensions(),
			Self::FrozenCrop { frozen_image, crop_rect } => crop_rect
				.map(|crop_rect| (crop_rect.width, crop_rect.height))
				.unwrap_or_else(|| frozen_image.dimensions()),
			Self::WindowImageWithMatte { window_image, .. } => window_image.dimensions(),
		}
	}

	#[cfg(test)]
	fn export_image(&self) -> Option<RgbaImage> {
		match self {
			Self::Prepared { image } => Some(image.clone()),
			Self::FrozenCrop { frozen_image, crop_rect } => {
				export_image_from_frozen_crop(frozen_image, *crop_rect)
			},
			Self::WindowImageWithMatte { window_image, matte } => {
				Some(flatten_window_image_with_matte(window_image, *matte))
			},
		}
	}

	fn into_export_image(self) -> Option<RgbaImage> {
		match self {
			Self::Prepared { image } => Some(image),
			Self::FrozenCrop { frozen_image, crop_rect } => {
				export_image_from_frozen_crop(&frozen_image, crop_rect)
			},
			Self::WindowImageWithMatte { window_image, matte } => {
				Some(flatten_window_image_with_matte(&window_image, matte))
			},
		}
	}
}

#[cfg(target_os = "macos")]
fn export_image_from_frozen_crop(
	frozen_image: &RgbaImage,
	crop_rect: Option<RectPoints>,
) -> Option<RgbaImage> {
	match crop_rect {
		Some(crop_rect) => {
			if crop_rect.width == 0 || crop_rect.height == 0 {
				return None;
			}

			Some(
				imageops::crop_imm(
					frozen_image,
					crop_rect.x,
					crop_rect.y,
					crop_rect.width,
					crop_rect.height,
				)
				.to_image(),
			)
		},
		None => Some(frozen_image.clone()),
	}
}

#[cfg(target_os = "macos")]
fn flatten_window_image_with_matte(
	window_image: &RgbaImage,
	matte: DeferredTextRecognitionWindowMatte,
) -> RgbaImage {
	let matte = match matte {
		DeferredTextRecognitionWindowMatte::Light => WINDOW_CAPTURE_MATTE_LIGHT_RGBA,
		DeferredTextRecognitionWindowMatte::Dark => WINDOW_CAPTURE_MATTE_DARK_RGBA,
	};
	let mut flattened = window_image.clone();

	for pixel in flattened.pixels_mut() {
		let alpha = u16::from(pixel[3]);
		let inv_alpha = 255_u16.saturating_sub(alpha);

		for channel in 0..3 {
			let src = u16::from(pixel[channel]);
			let bg = u16::from(matte[channel]);
			let blended = (src.saturating_mul(alpha) + bg.saturating_mul(inv_alpha) + 127) / 255;

			pixel[channel] = blended as u8;
		}

		pixel[3] = 255;
	}

	flattened
}

#[cfg(target_os = "macos")]
fn log_ocr_request_completed(
	request_id: u64,
	requested_at: Instant,
	outcome: &'static str,
	recognized_lines: usize,
	recognized_chars: usize,
	clipboard_write_ms: Option<u128>,
	error: Option<&str>,
) {
	tracing::info!(
		target: "rsnap",
		op = "overlay.ocr_request_completed",
		request_id,
		outcome,
		total_ms = requested_at.elapsed().as_millis(),
		recognized_lines,
		recognized_chars,
		clipboard_write_ms,
		error,
		"OCR request completed."
	);
}

#[cfg(test)]
mod tests {
	use std::time::Instant;

	use image::Rgba;
	use image::RgbaImage;

	use super::{
		DeferredTextRecognitionImageSource, DeferredTextRecognitionRequest,
		DeferredTextRecognitionWindowMatte,
	};
	use crate::state::RectPoints;

	#[test]
	fn frozen_crop_source_exports_clipped_region() {
		let mut image = RgbaImage::from_pixel(5, 4, Rgba([0, 0, 0, 255]));
		*image.get_pixel_mut(2, 1) = Rgba([10, 20, 30, 255]);
		*image.get_pixel_mut(3, 2) = Rgba([40, 50, 60, 255]);
		let request = DeferredTextRecognitionRequest {
			request_id: 7,
			requested_at: Instant::now(),
			image_source: DeferredTextRecognitionImageSource::FrozenCrop {
				frozen_image: image,
				crop_rect: Some(RectPoints::new(2, 1, 2, 2)),
			},
		};
		let export = request.export_image().expect("export image");

		assert_eq!(export.dimensions(), (2, 2));
		assert_eq!(*export.get_pixel(0, 0), Rgba([10, 20, 30, 255]));
		assert_eq!(*export.get_pixel(1, 1), Rgba([40, 50, 60, 255]));
	}

	#[test]
	fn window_matte_source_flattens_alpha_before_ocr() {
		let request = DeferredTextRecognitionRequest {
			request_id: 11,
			requested_at: Instant::now(),
			image_source: DeferredTextRecognitionImageSource::WindowImageWithMatte {
				window_image: RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 128])),
				matte: DeferredTextRecognitionWindowMatte::Light,
			},
		};
		let export = request.export_image().expect("export image");

		assert_eq!(*export.get_pixel(0, 0), Rgba([123, 123, 123, 255]));
	}
}
