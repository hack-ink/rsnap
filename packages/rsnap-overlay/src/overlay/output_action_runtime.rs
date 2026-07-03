#[cfg(target_os = "macos")]
use crate::overlay::runtime_timing;
#[cfg(target_os = "macos")]
use crate::overlay::{FrozenCaptureSource, RectPoints, WindowCaptureAlphaMode};
use crate::overlay::{OverlayControl, OverlayMode, OverlaySession, PngAction};
#[cfg(target_os = "macos")]
use rsnap_capture_core::DeferredTextRecognitionRequest;
use rsnap_capture_core::PreparedHostEffectRequest;

impl OverlaySession {
	pub(super) fn handle_encoded_png_response(&mut self, png_bytes: Vec<u8>) -> OverlayControl {
		let Some(action) = self.pending_png_action.take() else {
			return OverlayControl::Continue;
		};

		match action {
			PngAction::Copy => {
				OverlayControl::HostEffect(PreparedHostEffectRequest::CopyPng { png_bytes })
			},
			PngAction::Save => OverlayControl::HostEffect(PreparedHostEffectRequest::SavePng {
				png_bytes,
				output_dir: self.config.output_dir.clone(),
				output_filename_prefix: self.config.output_filename_prefix.clone(),
				output_naming: self.config.output_naming,
			}),
		}
	}

	#[cfg(target_os = "macos")]
	fn next_ocr_request_id(&mut self) -> u64 {
		let request_id = self.next_ocr_request_id;

		self.next_ocr_request_id = self.next_ocr_request_id.wrapping_add(1);

		request_id
	}

	#[cfg(target_os = "macos")]
	pub(super) fn maybe_request_redraw_for_pending_output(&mut self) {
		if self.pending_encode_png.is_some() {
			self.request_redraw_all();
		}
	}

	#[cfg(target_os = "macos")]
	fn current_deferred_text_recognition_request(
		&mut self,
		request_id: u64,
	) -> Option<DeferredTextRecognitionRequest> {
		let requested_at_unix_ms = runtime_timing::current_unix_millis();

		if self.scroll_capture.active {
			let image = self.scroll_capture.session.as_ref()?.export_image().clone();

			return Some(DeferredTextRecognitionRequest::prepared(
				request_id,
				requested_at_unix_ms,
				image,
			));
		}
		if self.frozen_capture_source == FrozenCaptureSource::Window {
			match self.config.window_capture_alpha_mode {
				WindowCaptureAlphaMode::Background => {},
				WindowCaptureAlphaMode::MatteLight => {
					if let Some(window_image) = self.frozen_window_image.take() {
						return Some(DeferredTextRecognitionRequest::prepared(
							request_id,
							requested_at_unix_ms,
							window_image,
						));
					}
				},
				WindowCaptureAlphaMode::MatteDark => {
					if let Some(window_image) = self.frozen_window_image.take() {
						return Some(DeferredTextRecognitionRequest::prepared(
							request_id,
							requested_at_unix_ms,
							window_image,
						));
					}
				},
			}
		}

		let crop_rect = self.deferred_text_recognition_crop_rect_pixels()?;
		let export_image = self.state.frozen_export_image.take()?;

		Some(DeferredTextRecognitionRequest::frozen_crop(
			request_id,
			requested_at_unix_ms,
			export_image,
			crop_rect,
		))
	}

	#[cfg(target_os = "macos")]
	fn deferred_text_recognition_crop_rect_pixels(&self) -> Option<Option<RectPoints>> {
		let export_image = self.state.frozen_export_image.as_ref()?;
		let Some(monitor) = self.state.monitor else {
			return Some(None);
		};
		let capture_rect = self
			.state
			.frozen_capture_rect
			.unwrap_or_else(|| RectPoints::new(0, 0, monitor.width, monitor.height));
		let capture_rect = monitor.local_rect_to_pixels(capture_rect);
		let x = capture_rect.x.min(export_image.width());
		let y = capture_rect.y.min(export_image.height());
		let max_width = export_image.width().saturating_sub(x);
		let max_height = export_image.height().saturating_sub(y);
		let width = capture_rect.width.min(max_width);
		let height = capture_rect.height.min(max_height);

		if width == 0 || height == 0 {
			return None;
		}
		if x == 0 && y == 0 && width == export_image.width() && height == export_image.height() {
			return Some(None);
		}

		Some(Some(RectPoints::new(x, y, width, height)))
	}

	pub(super) fn begin_png_action(&mut self, action: PngAction) {
		if !matches!(self.state.mode, OverlayMode::Frozen) {
			return;
		}
		if !self.frozen_final_capture_ready() {
			self.state.set_error("Preparing capture...");
			self.request_redraw_all();

			return;
		}

		self.prepare_active_scroll_capture_output();

		let image = if self.scroll_capture.active {
			self.current_scroll_preview_render_image()
		} else {
			self.current_export_image()
		};
		let Some(export_image) = image else {
			return;
		};

		self.pending_png_action = Some(action);

		match action {
			PngAction::Copy => self.state.set_error("Copying..."),
			PngAction::Save => self.state.set_error("Saving..."),
		}

		self.pending_encode_png = Some(export_image);

		self.request_redraw_all();
	}

	#[cfg(target_os = "macos")]
	pub(super) fn begin_ocr_action(&mut self) -> OverlayControl {
		if !matches!(self.state.mode, OverlayMode::Frozen) {
			return OverlayControl::Continue;
		}
		if !self.frozen_final_capture_ready() {
			self.state.set_error("Preparing capture...");
			self.request_redraw_all();

			return OverlayControl::Continue;
		}

		self.prepare_active_scroll_capture_output();

		let request_id = self.next_ocr_request_id();
		let Some(request) = self.current_deferred_text_recognition_request(request_id) else {
			return OverlayControl::Continue;
		};
		let (image_width_px, image_height_px) = request.image_dimensions();

		self.pending_png_action = None;
		self.pending_encode_png = None;

		self.state.clear_error();

		tracing::info!(
			target: "rsnap",
			op = "overlay.ocr_request_started",
			request_id,
			image_width_px,
			image_height_px,
			image_pixels = u64::from(image_width_px) * u64::from(image_height_px),
			scroll_capture_active = self.scroll_capture.active,
			"Queued OCR request."
		);

		OverlayControl::HostEffect(PreparedHostEffectRequest::DeferredTextRecognition(request))
	}
}
