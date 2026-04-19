use std::sync::{Arc, Mutex};

use image::RgbaImage;

use crate::backend;
use crate::overlay::ScrollCaptureHostFrameRequestError;
use crate::state::{MonitorRect, RectPoints};
use crate::worker::{
	CapturedMonitorRegionResult, OverlayWorker, WorkerErrorSource, WorkerRequestSendError,
	WorkerResponse,
};

#[derive(Debug)]
/// Host-owned scroll-capture capability event emitted back into the Rust core.
pub enum MacOSScrollCaptureCapabilityEvent {
	/// A fresh region sample became available for scroll-capture observation.
	Frame {
		/// Target monitor for the sample.
		monitor: MonitorRect,
		/// Sampled monitor-local pixel rect.
		rect_px: RectPoints,
		/// Request identifier supplied by the core.
		request_id: u64,
		/// Captured RGBA frame.
		image: RgbaImage,
	},
	/// The host completed the request but could not produce a newer frame.
	NoNewFrame {
		/// Target monitor for the sample.
		monitor: MonitorRect,
		/// Sampled monitor-local pixel rect.
		rect_px: RectPoints,
		/// Request identifier supplied by the core.
		request_id: u64,
	},
	/// The host capability failed and the core must fail closed.
	Failure {
		/// User-visible failure message.
		message: String,
	},
}

#[derive(Clone)]
/// App-owned macOS scroll-capture capability runtime for region sampling.
pub struct MacOSScrollCaptureCapability {
	worker: Arc<Mutex<OverlayWorker>>,
}
impl MacOSScrollCaptureCapability {
	#[must_use]
	/// Builds a host-owned capability runtime with the current self-capture exception allowlist.
	pub fn new(
		self_capture_exception_window_ids: Vec<u32>,
		response_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	) -> Self {
		Self {
			worker: Arc::new(Mutex::new(OverlayWorker::new(
				backend::default_capture_backend_with_self_capture_exception_window_ids(
					self_capture_exception_window_ids,
				),
				response_waker,
			))),
		}
	}

	/// Queues one host-owned region sample request for scroll capture.
	pub fn request_frame(
		&self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		request_id: u64,
	) -> std::result::Result<(), ScrollCaptureHostFrameRequestError> {
		let worker = self.worker.lock().map_err(|_| {
			ScrollCaptureHostFrameRequestError::Unavailable(String::from(
				"Scroll capture capability worker lock was poisoned.",
			))
		})?;

		worker.request_capture_monitor_region(monitor, rect_px, request_id).map_err(|err| match err
		{
			WorkerRequestSendError::Full => ScrollCaptureHostFrameRequestError::Busy,
			WorkerRequestSendError::Disconnected => {
				ScrollCaptureHostFrameRequestError::Unavailable(String::from(
					"Scroll capture capability is unavailable.",
				))
			},
		})
	}

	#[must_use]
	/// Drains every pending host-owned region-sample outcome for the core.
	pub fn drain_events(&self) -> Vec<MacOSScrollCaptureCapabilityEvent> {
		let mut events = Vec::new();
		let worker = match self.worker.lock() {
			Ok(worker) => worker,
			Err(poisoned) => {
				events.push(MacOSScrollCaptureCapabilityEvent::Failure {
					message: String::from("Scroll capture capability worker lock was poisoned."),
				});

				poisoned.into_inner()
			},
		};

		while let Some(resp) = worker.try_recv_captured_monitor_region() {
			match resp.result {
				CapturedMonitorRegionResult::Image(image) => {
					events.push(MacOSScrollCaptureCapabilityEvent::Frame {
						monitor: resp.monitor,
						rect_px: resp.rect_px,
						request_id: resp.request_id,
						image,
					});
				},
				CapturedMonitorRegionResult::NoNewFrame => {
					events.push(MacOSScrollCaptureCapabilityEvent::NoNewFrame {
						monitor: resp.monitor,
						rect_px: resp.rect_px,
						request_id: resp.request_id,
					});
				},
			}
		}
		while let Some(resp) = worker.try_recv() {
			if let WorkerResponse::Error {
				source: WorkerErrorSource::CaptureMonitorRegion,
				message,
			} = resp
			{
				events.push(MacOSScrollCaptureCapabilityEvent::Failure { message });
			}
		}

		events
	}
}
