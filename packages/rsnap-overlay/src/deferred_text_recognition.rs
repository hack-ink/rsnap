//! Deferred OCR processing that runs after the overlay has already exited.

#[cfg(target_os = "macos")]
use std::{
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use image::{Rgba, RgbaImage, imageops};

#[cfg(target_os = "macos")]
use crate::state::RectPoints;
use crate::{
	ocr_macos::{self, RecognizedTextOutput},
	overlay::output,
};

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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Final background OCR outcome reported for structured logging and telemetry.
pub enum DeferredTextRecognitionOutcomeKind {
	/// OCR produced non-empty text and the clipboard write succeeded.
	TextCopied,
	/// OCR completed successfully but did not return any non-whitespace text.
	NoText,
	/// OCR finished, but a newer capture superseded this request before publish.
	StaleRequestSuppressed,
	/// OCR completed successfully but writing the recognized text to the clipboard failed.
	ClipboardError,
	/// OCR could not prepare the export image or Vision failed to recognize text.
	RecognizeError,
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
/// A deferred OCR job emitted by the overlay and executed by the app shell.
pub struct DeferredTextRecognitionRequest {
	/// Monotonic request identifier used to correlate logs across threads.
	pub request_id: u64,
	/// Timestamp captured when the overlay scheduled the background OCR request.
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
#[derive(Debug)]
/// Structured result returned after a deferred OCR request finishes.
pub struct DeferredTextRecognitionOutcome {
	/// Monotonic request identifier used to correlate logs across threads.
	pub request_id: u64,
	/// Final high-level outcome for the deferred OCR request.
	pub kind: DeferredTextRecognitionOutcomeKind,
	/// Number of non-empty lines returned by Vision.
	pub recognized_lines: usize,
	/// Number of characters returned by Vision after line joining.
	pub recognized_chars: usize,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug)]
struct DeferredTextRecognitionContext {
	request_id: u64,
	requested_at: Instant,
	worker_started_at: Instant,
	queue_delay: Duration,
}
#[cfg(target_os = "macos")]
impl DeferredTextRecognitionContext {
	fn new(request_id: u64, requested_at: Instant) -> Self {
		let worker_started_at = Instant::now();

		Self {
			request_id,
			requested_at,
			worker_started_at,
			queue_delay: worker_started_at.saturating_duration_since(requested_at),
		}
	}
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct DeferredTextRecognitionPublishGate {
	latest_generation: Arc<AtomicU64>,
	request_generation: u64,
}
#[cfg(target_os = "macos")]
impl DeferredTextRecognitionPublishGate {
	fn allows_publish(&self) -> bool {
		self.latest_generation.load(Ordering::Acquire) == self.request_generation
	}
}

#[cfg(target_os = "macos")]
/// Runs a deferred OCR request and records structured timing logs for each
/// phase.
pub fn process_deferred_text_recognition(
	request: DeferredTextRecognitionRequest,
) -> DeferredTextRecognitionOutcome {
	process_deferred_text_recognition_with_gate(request, None)
}

#[cfg(target_os = "macos")]
/// Runs a deferred OCR request and only publishes recognized text if the
/// associated capture generation is still the latest one when OCR completes.
pub fn process_deferred_text_recognition_for_latest_capture(
	request: DeferredTextRecognitionRequest,
	latest_generation: Arc<AtomicU64>,
	request_generation: u64,
) -> DeferredTextRecognitionOutcome {
	process_deferred_text_recognition_with_gate(
		request,
		Some(DeferredTextRecognitionPublishGate { latest_generation, request_generation }),
	)
}

#[cfg(target_os = "macos")]
fn process_deferred_text_recognition_with_gate(
	request: DeferredTextRecognitionRequest,
	publish_gate: Option<DeferredTextRecognitionPublishGate>,
) -> DeferredTextRecognitionOutcome {
	let context = DeferredTextRecognitionContext::new(request.request_id, request.requested_at);
	let export_prepare_started_at = Instant::now();
	let Some(export_image) = request.image_source.into_export_image() else {
		return empty_export_outcome(&context, export_prepare_started_at.elapsed());
	};
	let export_prepare_elapsed = export_prepare_started_at.elapsed();
	let image_width_px = export_image.width();
	let image_height_px = export_image.height();

	match ocr_macos::recognize_text_from_image(&export_image) {
		Ok(output) => recognized_text_outcome(
			&context,
			image_width_px,
			image_height_px,
			export_prepare_elapsed,
			output,
			publish_gate.as_ref(),
		),
		Err(err) => recognize_error_outcome(
			&context,
			image_width_px,
			image_height_px,
			export_prepare_elapsed,
			format!("{err:#}"),
		),
	}
}

#[cfg(target_os = "macos")]
fn empty_export_outcome(
	context: &DeferredTextRecognitionContext,
	export_prepare_elapsed: Duration,
) -> DeferredTextRecognitionOutcome {
	let error = String::from("OCR export source resolved to an empty image");

	tracing::warn!(
		target: "rsnap",
		op = "overlay.ocr_phase_timing",
		request_id = context.request_id,
		queue_delay_ms = context.queue_delay.as_millis(),
		export_prepare_ms = export_prepare_elapsed.as_millis(),
		total_ms = context.worker_started_at.elapsed().as_millis(),
		error = %error,
		"OCR request failed before Vision processing."
	);

	log_ocr_request_completed(
		context.request_id,
		context.requested_at,
		"recognize_error",
		0,
		0,
		None,
		Some(error.as_str()),
	);

	outcome(context.request_id, DeferredTextRecognitionOutcomeKind::RecognizeError, 0, 0)
}

#[cfg(target_os = "macos")]
fn recognized_text_outcome(
	context: &DeferredTextRecognitionContext,
	image_width_px: u32,
	image_height_px: u32,
	export_prepare_elapsed: Duration,
	output: RecognizedTextOutput,
	publish_gate: Option<&DeferredTextRecognitionPublishGate>,
) -> DeferredTextRecognitionOutcome {
	let recognized_lines = output.line_count;
	let recognized_chars = output.text.chars().count();

	tracing::info!(
		target: "rsnap",
		op = "overlay.ocr_phase_timing",
		request_id = context.request_id,
		image_width_px,
		image_height_px,
		image_pixels = u64::from(image_width_px) * u64::from(image_height_px),
		queue_delay_ms = context.queue_delay.as_millis(),
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
			context.request_id,
			context.requested_at,
			"no_text",
			recognized_lines,
			recognized_chars,
			None,
			None,
		);

		return outcome(
			context.request_id,
			DeferredTextRecognitionOutcomeKind::NoText,
			recognized_lines,
			recognized_chars,
		);
	}
	if !publish_gate_allows_publish(publish_gate) {
		log_ocr_request_completed(
			context.request_id,
			context.requested_at,
			"stale_request_suppressed",
			recognized_lines,
			recognized_chars,
			None,
			None,
		);

		return outcome(
			context.request_id,
			DeferredTextRecognitionOutcomeKind::StaleRequestSuppressed,
			recognized_lines,
			recognized_chars,
		);
	}

	let clipboard_write_started_at = Instant::now();

	match output::write_text_to_clipboard(&output.text) {
		Ok(()) => {
			log_ocr_request_completed(
				context.request_id,
				context.requested_at,
				"text_copied",
				recognized_lines,
				recognized_chars,
				Some(clipboard_write_started_at.elapsed().as_millis()),
				None,
			);

			outcome(
				context.request_id,
				DeferredTextRecognitionOutcomeKind::TextCopied,
				recognized_lines,
				recognized_chars,
			)
		},
		Err(err) => {
			let error = format!("{err:#}");

			log_ocr_request_completed(
				context.request_id,
				context.requested_at,
				"clipboard_error",
				recognized_lines,
				recognized_chars,
				Some(clipboard_write_started_at.elapsed().as_millis()),
				Some(error.as_str()),
			);

			outcome(
				context.request_id,
				DeferredTextRecognitionOutcomeKind::ClipboardError,
				recognized_lines,
				recognized_chars,
			)
		},
	}
}

#[cfg(target_os = "macos")]
fn publish_gate_allows_publish(publish_gate: Option<&DeferredTextRecognitionPublishGate>) -> bool {
	publish_gate.is_none_or(DeferredTextRecognitionPublishGate::allows_publish)
}

#[cfg(target_os = "macos")]
fn recognize_error_outcome(
	context: &DeferredTextRecognitionContext,
	image_width_px: u32,
	image_height_px: u32,
	export_prepare_elapsed: Duration,
	error: String,
) -> DeferredTextRecognitionOutcome {
	tracing::warn!(
		target: "rsnap",
		op = "overlay.ocr_phase_timing",
		request_id = context.request_id,
		image_width_px,
		image_height_px,
		image_pixels = u64::from(image_width_px) * u64::from(image_height_px),
		queue_delay_ms = context.queue_delay.as_millis(),
		export_prepare_ms = export_prepare_elapsed.as_millis(),
		total_ms = context.worker_started_at.elapsed().as_millis(),
		error = %error,
		"OCR request failed."
	);

	log_ocr_request_completed(
		context.request_id,
		context.requested_at,
		"recognize_error",
		0,
		0,
		None,
		Some(error.as_str()),
	);

	outcome(context.request_id, DeferredTextRecognitionOutcomeKind::RecognizeError, 0, 0)
}

#[cfg(target_os = "macos")]
fn outcome(
	request_id: u64,
	kind: DeferredTextRecognitionOutcomeKind,
	recognized_lines: usize,
	recognized_chars: usize,
) -> DeferredTextRecognitionOutcome {
	DeferredTextRecognitionOutcome { request_id, kind, recognized_lines, recognized_chars }
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
	#[cfg(target_os = "macos")]
	use std::sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	};
	use std::time::Instant;

	use image::Rgba;
	use image::RgbaImage;

	use crate::deferred_text_recognition::{
		self, DeferredTextRecognitionImageSource, DeferredTextRecognitionPublishGate,
		DeferredTextRecognitionRequest, DeferredTextRecognitionWindowMatte,
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

	#[cfg(target_os = "macos")]
	#[test]
	fn publish_gate_only_allows_latest_capture_generation() {
		let latest_generation = Arc::new(AtomicU64::new(7));
		let matching_gate = DeferredTextRecognitionPublishGate {
			latest_generation: Arc::clone(&latest_generation),
			request_generation: 7,
		};
		let stale_gate = DeferredTextRecognitionPublishGate {
			latest_generation: Arc::clone(&latest_generation),
			request_generation: 6,
		};

		assert!(deferred_text_recognition::publish_gate_allows_publish(Some(&matching_gate,)));

		latest_generation.store(8, Ordering::Release);

		assert!(!deferred_text_recognition::publish_gate_allows_publish(Some(&stale_gate,)));
		assert!(!deferred_text_recognition::publish_gate_allows_publish(Some(&matching_gate,)));
		assert!(deferred_text_recognition::publish_gate_allows_publish(None));
	}
}
