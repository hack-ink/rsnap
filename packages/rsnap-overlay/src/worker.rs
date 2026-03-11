use std::sync::{
	Arc,
	mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError},
};
use std::thread;
#[cfg(not(target_os = "macos"))]
use std::time::Instant;

use image::RgbaImage;

use crate::backend::CaptureBackend;
use crate::png;
use crate::state::{
	GlobalPoint, LiveCursorSample, MonitorRect, RectPoints, WindowHit, WindowListSnapshot,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FreezeCaptureTarget {
	Monitor,
	Window { window_id: u32 },
}

#[derive(Debug)]
pub(crate) enum WorkerRequest {
	HitTestWindow {
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
	},
	#[cfg(not(target_os = "macos"))]
	SampleLiveCursor {
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
		want_patch: bool,
		patch_width_px: u32,
		patch_height_px: u32,
	},
	RefreshWindowList,
	FreezeCapture {
		monitor: MonitorRect,
		target: FreezeCaptureTarget,
	},
	CaptureMonitorRegion {
		monitor: MonitorRect,
		rect_px: RectPoints,
		request_id: u64,
	},
	EncodePng {
		image: RgbaImage,
	},
}

#[derive(Debug)]
pub(crate) enum WorkerResponse {
	#[cfg_attr(target_os = "macos", allow(dead_code))]
	SampledLiveCursor {
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
		sample: LiveCursorSample,
	},
	HitTestWindow {
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
		hit: Option<WindowHit>,
	},
	RefreshedWindowList {
		snapshot: Arc<WindowListSnapshot>,
	},
	CapturedFreeze {
		monitor: MonitorRect,
		image: RgbaImage,
		window_image: Option<RgbaImage>,
		captured_window_id: Option<u32>,
	},
	EncodedPng {
		png_bytes: Vec<u8>,
	},
	Error(String),
}

#[derive(Debug)]
pub(crate) enum CapturedMonitorRegionResult {
	Image(RgbaImage),
	NoNewFrame,
}

#[derive(Debug)]
pub(crate) enum WorkerRequestSendError {
	Full,
	Disconnected,
}

#[derive(Debug)]
pub(crate) struct CapturedMonitorRegionResponse {
	pub(crate) monitor: MonitorRect,
	pub(crate) rect_px: RectPoints,
	pub(crate) request_id: u64,
	pub(crate) result: CapturedMonitorRegionResult,
}

pub(crate) struct OverlayWorker {
	req_tx: SyncSender<WorkerRequest>,
	resp_rx: Receiver<WorkerResponse>,
	region_capture_resp_rx: Receiver<CapturedMonitorRegionResponse>,
}
impl OverlayWorker {
	pub(crate) fn new(
		backend: Box<dyn CaptureBackend>,
		response_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	) -> Self {
		let (req_tx, req_rx) = mpsc::sync_channel(64);
		let (resp_tx, resp_rx) = mpsc::channel();
		let (region_capture_resp_tx, region_capture_resp_rx) = mpsc::channel();

		thread::spawn(move || {
			Self::run_worker_loop(backend, req_rx, resp_tx, region_capture_resp_tx, response_waker)
		});

		Self { req_tx, resp_rx, region_capture_resp_rx }
	}

	fn run_worker_loop(
		mut backend: Box<dyn CaptureBackend>,
		req_rx: Receiver<WorkerRequest>,
		resp_tx: Sender<WorkerResponse>,
		region_capture_resp_tx: Sender<CapturedMonitorRegionResponse>,
		response_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	) {
		while let Ok(first) = req_rx.recv() {
			let mut pending = PendingWorkerRequests::default();

			pending.record(first);

			while let Ok(next) = req_rx.try_recv() {
				pending.record(next);
			}

			pending.dispatch(
				&mut *backend,
				&resp_tx,
				&region_capture_resp_tx,
				response_waker.as_deref(),
			);
		}
	}

	fn handle_encode_request(
		resp_tx: &Sender<WorkerResponse>,
		response_waker: Option<&(dyn Fn() + Send + Sync)>,
		image: RgbaImage,
	) {
		match png::rgba_image_to_png_bytes(&image) {
			Ok(png_bytes) => {
				Self::send_response(
					resp_tx,
					response_waker,
					WorkerResponse::EncodedPng { png_bytes },
				);
			},
			Err(err) => {
				Self::send_response(
					resp_tx,
					response_waker,
					WorkerResponse::Error(format!("{err:#}")),
				);
			},
		}
	}

	fn handle_freeze_request(
		backend: &mut dyn CaptureBackend,
		resp_tx: &Sender<WorkerResponse>,
		response_waker: Option<&(dyn Fn() + Send + Sync)>,
		monitor: MonitorRect,
		target: FreezeCaptureTarget,
	) {
		let mut captured_window_id = None;
		let mut window_image = None;

		if let FreezeCaptureTarget::Window { window_id } = target
			&& let Ok(image) = backend.capture_window(window_id)
		{
			captured_window_id = Some(window_id);
			window_image = Some(image);
		}

		match backend.capture_monitor(monitor) {
			Ok(image) => {
				Self::send_response(
					resp_tx,
					response_waker,
					WorkerResponse::CapturedFreeze {
						monitor,
						image,
						window_image,
						captured_window_id,
					},
				);
			},
			Err(err) => {
				Self::send_response(
					resp_tx,
					response_waker,
					WorkerResponse::Error(format!("{err:#}")),
				);
			},
		}
	}

	fn handle_refresh_window_list_request(
		backend: &mut dyn CaptureBackend,
		resp_tx: &Sender<WorkerResponse>,
		response_waker: Option<&(dyn Fn() + Send + Sync)>,
	) {
		match backend.refresh_window_cache() {
			Ok(snapshot) => {
				Self::send_response(
					resp_tx,
					response_waker,
					WorkerResponse::RefreshedWindowList { snapshot },
				);
			},
			Err(err) => {
				Self::send_response(
					resp_tx,
					response_waker,
					WorkerResponse::Error(format!("{err:#}")),
				);
			},
		}
	}

	fn handle_capture_monitor_region_request(
		backend: &mut dyn CaptureBackend,
		resp_tx: &Sender<WorkerResponse>,
		region_capture_resp_tx: &Sender<CapturedMonitorRegionResponse>,
		response_waker: Option<&(dyn Fn() + Send + Sync)>,
		monitor: MonitorRect,
		rect_px: RectPoints,
		request_id: u64,
	) {
		match backend.capture_monitor_region_for_scroll_capture(monitor, rect_px) {
			Ok(Some(image)) => {
				Self::send_region_capture_response(
					region_capture_resp_tx,
					response_waker,
					CapturedMonitorRegionResponse {
						monitor,
						rect_px,
						request_id,
						result: CapturedMonitorRegionResult::Image(image),
					},
				);
			},
			Ok(None) => {
				Self::send_region_capture_response(
					region_capture_resp_tx,
					response_waker,
					CapturedMonitorRegionResponse {
						monitor,
						rect_px,
						request_id,
						result: CapturedMonitorRegionResult::NoNewFrame,
					},
				);
			},
			Err(err) => {
				Self::send_response(
					resp_tx,
					response_waker,
					WorkerResponse::Error(format!("{err:#}")),
				);
			},
		}
	}

	#[cfg(not(target_os = "macos"))]
	fn handle_sample_cursor_request(
		backend: &mut dyn CaptureBackend,
		resp_tx: &Sender<WorkerResponse>,
		response_waker: Option<&(dyn Fn() + Send + Sync)>,
		sample_req: (MonitorRect, GlobalPoint, u64, bool, u32, u32),
	) {
		let (monitor, point, request_id, want_patch, patch_width_px, patch_height_px) = sample_req;
		let started_at = Instant::now();
		let sample = backend
			.live_sample_cursor(monitor, point, want_patch, patch_width_px, patch_height_px)
			.unwrap_or(LiveCursorSample { rgb: None, patch: None });
		let elapsed = started_at.elapsed();

		if elapsed >= std::time::Duration::from_millis(8) {
			tracing::debug!(
				op = "overlay.live_sample_backend_latency",
				request_id,
				monitor_id = monitor.id,
				point = ?point,
				want_patch,
				elapsed_ms = elapsed.as_millis(),
				"Live cursor sample backend handling exceeded the target frame budget."
			);
		}

		Self::send_response(
			resp_tx,
			response_waker,
			WorkerResponse::SampledLiveCursor { monitor, point, request_id, sample },
		);
	}

	fn handle_hit_test_request(
		backend: &mut dyn CaptureBackend,
		resp_tx: &Sender<WorkerResponse>,
		response_waker: Option<&(dyn Fn() + Send + Sync)>,
		last_hit_test: Option<(MonitorRect, GlobalPoint, u64)>,
	) {
		if let Some((monitor, point, request_id)) = last_hit_test {
			let hit = backend.hit_test_window_in_monitor(monitor, point).unwrap_or_default();

			Self::send_response(
				resp_tx,
				response_waker,
				WorkerResponse::HitTestWindow { monitor, point, request_id, hit },
			);
		}
	}

	fn send_response(
		resp_tx: &Sender<WorkerResponse>,
		response_waker: Option<&(dyn Fn() + Send + Sync)>,
		resp: WorkerResponse,
	) {
		if resp_tx.send(resp).is_ok()
			&& let Some(wake) = response_waker
		{
			wake();
		}
	}

	fn send_region_capture_response(
		resp_tx: &Sender<CapturedMonitorRegionResponse>,
		response_waker: Option<&(dyn Fn() + Send + Sync)>,
		resp: CapturedMonitorRegionResponse,
	) {
		if resp_tx.send(resp).is_ok()
			&& let Some(wake) = response_waker
		{
			wake();
		}
	}

	fn map_try_send_error(err: TrySendError<WorkerRequest>) -> WorkerRequestSendError {
		match err {
			TrySendError::Full(_) => WorkerRequestSendError::Full,
			TrySendError::Disconnected(_) => WorkerRequestSendError::Disconnected,
		}
	}

	pub(crate) fn request_refresh_window_list(&self) -> bool {
		self.req_tx.try_send(WorkerRequest::RefreshWindowList).is_ok()
	}

	pub(crate) fn request_freeze_capture(
		&self,
		monitor: MonitorRect,
		target: FreezeCaptureTarget,
	) -> bool {
		self.req_tx.try_send(WorkerRequest::FreezeCapture { monitor, target }).is_ok()
	}

	pub(crate) fn request_hit_test_window(
		&self,
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
	) -> Result<(), WorkerRequestSendError> {
		let request = WorkerRequest::HitTestWindow { monitor, point, request_id };

		self.req_tx.try_send(request).map_err(Self::map_try_send_error)
	}

	#[cfg(not(target_os = "macos"))]
	pub(crate) fn request_sample_live_cursor(
		&self,
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
		want_patch: bool,
		patch_width_px: u32,
		patch_height_px: u32,
	) -> Result<(), WorkerRequestSendError> {
		let request = WorkerRequest::SampleLiveCursor {
			monitor,
			point,
			request_id,
			want_patch,
			patch_width_px,
			patch_height_px,
		};

		self.req_tx.try_send(request).map_err(Self::map_try_send_error)
	}

	pub(crate) fn request_encode_png(&self, image: RgbaImage) -> Result<(), RgbaImage> {
		match self.req_tx.try_send(WorkerRequest::EncodePng { image }) {
			Ok(()) => Ok(()),
			Err(TrySendError::Full(WorkerRequest::EncodePng { image })) => Err(image),
			Err(TrySendError::Disconnected(WorkerRequest::EncodePng { image })) => Err(image),
			Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
				unreachable!("request_encode_png only sends WorkerRequest::EncodePng")
			},
		}
	}

	pub(crate) fn request_capture_monitor_region(
		&self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		request_id: u64,
	) -> Result<(), WorkerRequestSendError> {
		let request = WorkerRequest::CaptureMonitorRegion { monitor, rect_px, request_id };

		self.req_tx.try_send(request).map_err(Self::map_try_send_error)
	}

	pub(crate) fn try_recv(&self) -> Option<WorkerResponse> {
		match self.resp_rx.try_recv() {
			Ok(msg) => Some(msg),
			Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
		}
	}

	pub(crate) fn try_recv_captured_monitor_region(&self) -> Option<CapturedMonitorRegionResponse> {
		match self.region_capture_resp_rx.try_recv() {
			Ok(msg) => Some(msg),
			Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
		}
	}
}

#[derive(Default)]
struct PendingWorkerRequests {
	last_hit_test: Option<(MonitorRect, GlobalPoint, u64)>,
	#[cfg(not(target_os = "macos"))]
	last_sample_cursor: Option<(MonitorRect, GlobalPoint, u64, bool, u32, u32)>,
	last_refresh_window_list: bool,
	last_freeze: Option<(MonitorRect, FreezeCaptureTarget)>,
	last_capture_region: Option<(MonitorRect, RectPoints, u64)>,
	last_encode: Option<RgbaImage>,
}
impl PendingWorkerRequests {
	fn record(&mut self, request: WorkerRequest) {
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

	fn dispatch(
		self,
		backend: &mut dyn CaptureBackend,
		resp_tx: &Sender<WorkerResponse>,
		region_capture_resp_tx: &Sender<CapturedMonitorRegionResponse>,
		response_waker: Option<&(dyn Fn() + Send + Sync)>,
	) {
		if let Some(image) = self.last_encode {
			OverlayWorker::handle_encode_request(resp_tx, response_waker, image);

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

#[cfg(test)]
mod tests {
	use std::sync::mpsc;
	use std::sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	};

	use color_eyre::eyre::{self, Result};
	use image::{Rgba, RgbaImage};

	use crate::backend::CaptureBackend;
	use crate::state::{
		GlobalPoint, LiveCursorSample, MonitorImageSnapshot, MonitorRect, RectPoints, Rgb,
		WindowHit, WindowListSnapshot,
	};
	use crate::worker::{
		CapturedMonitorRegionResponse, CapturedMonitorRegionResult, OverlayWorker, WorkerResponse,
	};

	enum MockScrollCaptureResult {
		Image(RgbaImage),
		NoNewFrame,
		Error(String),
	}

	struct MockScrollCaptureBackend {
		scroll_capture_result: MockScrollCaptureResult,
	}

	impl CaptureBackend for MockScrollCaptureBackend {
		fn capture_monitor(&mut self, _monitor: MonitorRect) -> Result<RgbaImage> {
			Err(eyre::eyre!("unused in this test"))
		}

		fn capture_monitor_region_for_scroll_capture(
			&mut self,
			_monitor: MonitorRect,
			_rect_px: RectPoints,
		) -> Result<Option<RgbaImage>> {
			match &self.scroll_capture_result {
				MockScrollCaptureResult::Image(image) => Ok(Some(image.clone())),
				MockScrollCaptureResult::NoNewFrame => Ok(None),
				MockScrollCaptureResult::Error(message) => Err(eyre::eyre!("{message}")),
			}
		}

		fn pixel_rgb_in_monitor(
			&mut self,
			_monitor: MonitorRect,
			_point: GlobalPoint,
		) -> Result<Option<Rgb>> {
			Ok(None)
		}

		fn live_sample_cursor(
			&mut self,
			_monitor: MonitorRect,
			_point: GlobalPoint,
			_want_patch: bool,
			_patch_width_px: u32,
			_patch_height_px: u32,
		) -> Result<LiveCursorSample> {
			Ok(LiveCursorSample { rgb: None, patch: None })
		}

		fn hit_test_window_in_monitor(
			&mut self,
			_monitor: MonitorRect,
			_point: GlobalPoint,
		) -> Result<Option<WindowHit>> {
			Ok(None)
		}

		fn rgba_patch_in_monitor(
			&mut self,
			_monitor: MonitorRect,
			_point: GlobalPoint,
			_width_px: u32,
			_height_px: u32,
		) -> Result<Option<RgbaImage>> {
			Ok(None)
		}

		fn refresh_monitor_cache(
			&mut self,
			_monitor: MonitorRect,
		) -> Result<std::sync::Arc<MonitorImageSnapshot>> {
			Err(eyre::eyre!("unused in this test"))
		}

		fn refresh_window_cache(&mut self) -> Result<std::sync::Arc<WindowListSnapshot>> {
			Err(eyre::eyre!("unused in this test"))
		}
	}

	fn sample_monitor() -> MonitorRect {
		MonitorRect {
			id: 7,
			origin: GlobalPoint::new(0, 0),
			width: 640,
			height: 480,
			scale_factor_x1000: 2_000,
		}
	}

	fn sample_rect() -> RectPoints {
		RectPoints::new(10, 20, 100, 80)
	}

	fn sample_image() -> RgbaImage {
		RgbaImage::from_pixel(3, 2, Rgba([12, 34, 56, 255]))
	}

	#[test]
	fn send_response_wakes_after_regular_worker_response() {
		let (resp_tx, resp_rx) = mpsc::channel::<WorkerResponse>();
		let wake_count = Arc::new(AtomicUsize::new(0));
		let wake = {
			let wake_count = Arc::clone(&wake_count);

			move || {
				wake_count.fetch_add(1, Ordering::AcqRel);
			}
		};

		OverlayWorker::send_response(
			&resp_tx,
			Some(&wake),
			WorkerResponse::Error(String::from("wake me")),
		);

		let response = resp_rx.try_recv().expect("worker response");

		assert!(matches!(response, WorkerResponse::Error(message) if message == "wake me"));
		assert_eq!(wake_count.load(Ordering::Acquire), 1);
	}

	#[test]
	fn send_region_capture_response_wakes_after_scroll_region_result() {
		let (region_tx, region_rx) = mpsc::channel::<CapturedMonitorRegionResponse>();
		let wake_count = Arc::new(AtomicUsize::new(0));
		let wake = {
			let wake_count = Arc::clone(&wake_count);

			move || {
				wake_count.fetch_add(1, Ordering::AcqRel);
			}
		};
		let monitor = sample_monitor();
		let rect_px = sample_rect();

		OverlayWorker::send_region_capture_response(
			&region_tx,
			Some(&wake),
			CapturedMonitorRegionResponse {
				monitor,
				rect_px,
				request_id: 42,
				result: CapturedMonitorRegionResult::NoNewFrame,
			},
		);

		let response = region_rx.try_recv().expect("region result");

		assert_eq!(response.monitor, monitor);
		assert_eq!(response.rect_px, rect_px);
		assert_eq!(response.request_id, 42);
		assert!(matches!(response.result, CapturedMonitorRegionResult::NoNewFrame));
		assert_eq!(wake_count.load(Ordering::Acquire), 1);
	}

	#[test]
	fn capture_monitor_region_request_emits_image_result() {
		let (resp_tx, resp_rx) = mpsc::channel::<WorkerResponse>();
		let (region_tx, region_rx) = mpsc::channel::<CapturedMonitorRegionResponse>();
		let monitor = sample_monitor();
		let rect_px = sample_rect();
		let mut backend = MockScrollCaptureBackend {
			scroll_capture_result: MockScrollCaptureResult::Image(sample_image()),
		};

		OverlayWorker::handle_capture_monitor_region_request(
			&mut backend,
			&resp_tx,
			&region_tx,
			None,
			monitor,
			rect_px,
			99,
		);

		assert!(resp_rx.try_recv().is_err());

		let response = region_rx.try_recv().expect("region result");

		assert_eq!(response.monitor, monitor);
		assert_eq!(response.rect_px, rect_px);
		assert_eq!(response.request_id, 99);

		match response.result {
			CapturedMonitorRegionResult::Image(image) => assert_eq!(image, sample_image()),
			CapturedMonitorRegionResult::NoNewFrame => {
				panic!("expected an image result for a fresh scroll-capture frame")
			},
		}
	}

	#[test]
	fn capture_monitor_region_request_emits_no_new_frame_result() {
		let (resp_tx, resp_rx) = mpsc::channel::<WorkerResponse>();
		let (region_tx, region_rx) = mpsc::channel::<CapturedMonitorRegionResponse>();
		let mut backend =
			MockScrollCaptureBackend { scroll_capture_result: MockScrollCaptureResult::NoNewFrame };

		OverlayWorker::handle_capture_monitor_region_request(
			&mut backend,
			&resp_tx,
			&region_tx,
			None,
			sample_monitor(),
			sample_rect(),
			100,
		);

		assert!(resp_rx.try_recv().is_err());

		let response = region_rx.try_recv().expect("region result");

		assert!(matches!(response.result, CapturedMonitorRegionResult::NoNewFrame));
	}

	#[test]
	fn capture_monitor_region_request_routes_errors_to_worker_responses() {
		let (resp_tx, resp_rx) = mpsc::channel::<WorkerResponse>();
		let (region_tx, region_rx) = mpsc::channel::<CapturedMonitorRegionResponse>();
		let mut backend = MockScrollCaptureBackend {
			scroll_capture_result: MockScrollCaptureResult::Error(
				"fresh frame unavailable".to_owned(),
			),
		};

		OverlayWorker::handle_capture_monitor_region_request(
			&mut backend,
			&resp_tx,
			&region_tx,
			None,
			sample_monitor(),
			sample_rect(),
			101,
		);

		assert!(region_rx.try_recv().is_err());

		let response = resp_rx.try_recv().expect("worker response");

		match response {
			WorkerResponse::Error(message) => {
				assert!(message.contains("fresh frame unavailable"));
			},
			other => panic!("expected worker error, got {other:?}"),
		}
	}
}
