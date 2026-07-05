use std::sync::mpsc::Sender;

use image::RgbaImage;

use crate::backend::CaptureBackend;
use crate::state::{GlobalPoint, MonitorRect, RectPoints};
use crate::worker::{
	CapturedMonitorRegionResponse, FreezeCaptureTarget, OverlayWorker, WorkerRequest,
	WorkerResponse,
};

#[derive(Default)]
pub(super) struct PendingWorkerRequests {
	last_hit_test: Option<(MonitorRect, GlobalPoint, u64)>,
	#[cfg(not(target_os = "macos"))]
	last_sample_cursor: Option<(MonitorRect, GlobalPoint, u64, bool, u32, u32)>,
	last_refresh_window_list: bool,
	last_freeze: Option<(MonitorRect, FreezeCaptureTarget)>,
	last_capture_region: Option<(MonitorRect, RectPoints, u64)>,
	last_encode: Option<RgbaImage>,
}
impl PendingWorkerRequests {
	pub(super) fn record(&mut self, request: WorkerRequest) {
		match request {
			WorkerRequest::HitTestWindow { monitor, point, request_id } => {
				self.last_hit_test = Some((monitor, point, request_id));
			},
			#[cfg(not(target_os = "macos"))]
			WorkerRequest::SampleLiveCursor {
				monitor,
				point,
				request_id,
				want_patch,
				patch_width_px,
				patch_height_px,
			} => {
				self.last_sample_cursor =
					Some((monitor, point, request_id, want_patch, patch_width_px, patch_height_px));
			},
			WorkerRequest::RefreshWindowList => {
				self.last_refresh_window_list = true;
			},
			WorkerRequest::FreezeCapture { monitor, target } => {
				self.last_freeze = Some((monitor, target));
			},
			WorkerRequest::CaptureMonitorRegion { monitor, rect_px, request_id } => {
				self.last_capture_region = Some((monitor, rect_px, request_id));
			},
			WorkerRequest::EncodePng { image } => {
				self.last_encode = Some(image);
			},
		}
	}

	pub(super) fn dispatch(
		self,
		backend: &mut dyn CaptureBackend,
		resp_tx: &Sender<WorkerResponse>,
		region_capture_resp_tx: &Sender<CapturedMonitorRegionResponse>,
		response_waker: Option<&(dyn Fn() + Send + Sync)>,
	) {
		let mut handled_high_priority = false;

		if let Some(image) = self.last_encode {
			OverlayWorker::handle_encode_request(resp_tx, response_waker, image);

			handled_high_priority = true;
		}

		if handled_high_priority {
			return;
		}

		if let Some((monitor, target)) = self.last_freeze {
			OverlayWorker::handle_freeze_request(backend, resp_tx, response_waker, monitor, target);

			return;
		}
		if let Some((monitor, rect_px, request_id)) = self.last_capture_region {
			OverlayWorker::handle_capture_monitor_region_request(
				backend,
				resp_tx,
				region_capture_resp_tx,
				response_waker,
				monitor,
				rect_px,
				request_id,
			);

			return;
		}

		if self.last_refresh_window_list {
			OverlayWorker::handle_refresh_window_list_request(backend, resp_tx, response_waker);
		}

		#[cfg(not(target_os = "macos"))]
		if let Some((monitor, point, request_id, want_patch, patch_width_px, patch_height_px)) =
			self.last_sample_cursor
		{
			OverlayWorker::handle_sample_cursor_request(
				backend,
				resp_tx,
				response_waker,
				(monitor, point, request_id, want_patch, patch_width_px, patch_height_px),
			);
		}

		OverlayWorker::handle_hit_test_request(
			backend,
			resp_tx,
			response_waker,
			self.last_hit_test,
		);
	}
}
