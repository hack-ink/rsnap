use std::sync::{
	Arc,
	mpsc::{Receiver, Sender, SyncSender, TryRecvError, TrySendError},
};

use image::RgbaImage;

use crate::backend::CaptureBackend;
use crate::state::{
	GlobalPoint, MonitorImageSnapshot, MonitorRect, RectPoints, WindowListSnapshot,
};

#[derive(Debug)]
pub(crate) enum WorkerRequest {
	HitTestWindow { monitor: MonitorRect, point: GlobalPoint, request_id: u64 },
	RefreshMonitorImage { monitor: MonitorRect },
	RefreshWindowList,
	FreezeCapture { monitor: MonitorRect },
	EncodePng { image: RgbaImage },
}

#[derive(Debug)]
pub(crate) enum WorkerResponse {
	HitTestWindow {
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
		rect: Option<RectPoints>,
	},
	RefreshedMonitorImage {
		snapshot: Arc<MonitorImageSnapshot>,
	},
	RefreshedWindowList {
		snapshot: Arc<WindowListSnapshot>,
	},
	CapturedFreeze {
		monitor: MonitorRect,
		image: RgbaImage,
	},
	EncodedPng {
		png_bytes: Vec<u8>,
	},
	Error(String),
}

pub(crate) struct OverlayWorker {
	req_tx: SyncSender<WorkerRequest>,
	resp_rx: Receiver<WorkerResponse>,
}
impl OverlayWorker {
	pub(crate) fn new(backend: Box<dyn CaptureBackend>) -> Self {
		let (req_tx, req_rx) = std::sync::mpsc::sync_channel(64);
		let (resp_tx, resp_rx) = std::sync::mpsc::channel();

		std::thread::spawn(move || Self::run_worker_loop(backend, req_rx, resp_tx));

		Self { req_tx, resp_rx }
	}

	fn run_worker_loop(
		mut backend: Box<dyn CaptureBackend>,
		req_rx: Receiver<WorkerRequest>,
		resp_tx: Sender<WorkerResponse>,
	) {
		while let Ok(first) = req_rx.recv() {
			let mut last_hit_test: Option<(MonitorRect, GlobalPoint, u64)> = None;
			let mut last_refresh_monitor: Option<MonitorRect> = None;
			let mut last_refresh_window_list: bool = false;
			let mut last_freeze: Option<MonitorRect> = None;
			let mut last_encode: Option<RgbaImage> = None;

			match first {
				WorkerRequest::HitTestWindow { monitor, point, request_id } => {
					last_hit_test = Some((monitor, point, request_id))
				},
				WorkerRequest::RefreshMonitorImage { monitor } => {
					last_refresh_monitor = Some(monitor)
				},
				WorkerRequest::RefreshWindowList => {
					last_refresh_window_list = true;
				},
				WorkerRequest::FreezeCapture { monitor } => last_freeze = Some(monitor),
				WorkerRequest::EncodePng { image } => last_encode = Some(image),
			}

			while let Ok(next) = req_rx.try_recv() {
				match next {
					WorkerRequest::HitTestWindow { monitor, point, request_id } => {
						last_hit_test = Some((monitor, point, request_id))
					},
					WorkerRequest::RefreshMonitorImage { monitor } => {
						last_refresh_monitor = Some(monitor)
					},
					WorkerRequest::RefreshWindowList => {
						last_refresh_window_list = true;
					},
					WorkerRequest::FreezeCapture { monitor } => last_freeze = Some(monitor),
					WorkerRequest::EncodePng { image } => last_encode = Some(image),
				}
			}

			if let Some(image) = last_encode {
				Self::handle_encode_request(&resp_tx, image);

				continue;
			}
			if let Some(monitor) = last_freeze {
				Self::handle_freeze_request(&mut *backend, &resp_tx, monitor);

				continue;
			}
			if let Some(monitor) = last_refresh_monitor {
				Self::handle_refresh_monitor_request(&mut *backend, &resp_tx, monitor);
			}

			if last_refresh_window_list {
				Self::handle_refresh_window_list_request(&mut *backend, &resp_tx);
			}

			Self::handle_hit_test_request(&mut *backend, &resp_tx, last_hit_test);
		}
	}

	fn handle_encode_request(resp_tx: &Sender<WorkerResponse>, image: RgbaImage) {
		match crate::png::rgba_image_to_png_bytes(&image) {
			Ok(png_bytes) => {
				let _ = resp_tx.send(WorkerResponse::EncodedPng { png_bytes });
			},
			Err(err) => {
				let _ = resp_tx.send(WorkerResponse::Error(format!("{err:#}")));
			},
		}
	}

	fn handle_freeze_request(
		backend: &mut dyn CaptureBackend,
		resp_tx: &Sender<WorkerResponse>,
		monitor: MonitorRect,
	) {
		match backend.capture_monitor(monitor) {
			Ok(image) => {
				let _ = resp_tx.send(WorkerResponse::CapturedFreeze { monitor, image });
			},
			Err(err) => {
				let _ = resp_tx.send(WorkerResponse::Error(format!("{err:#}")));
			},
		}
	}

	fn handle_refresh_monitor_request(
		backend: &mut dyn CaptureBackend,
		resp_tx: &Sender<WorkerResponse>,
		monitor: MonitorRect,
	) {
		let refresh_result = backend.refresh_monitor_cache(monitor);

		match refresh_result {
			Ok(snapshot) => {
				let _ = resp_tx.send(WorkerResponse::RefreshedMonitorImage { snapshot });
			},
			Err(err) => {
				let _ = resp_tx.send(WorkerResponse::Error(format!("{err:#}")));
			},
		}
	}

	fn handle_refresh_window_list_request(
		backend: &mut dyn CaptureBackend,
		resp_tx: &Sender<WorkerResponse>,
	) {
		match backend.refresh_window_cache() {
			Ok(snapshot) => {
				let _ = resp_tx.send(WorkerResponse::RefreshedWindowList { snapshot });
			},
			Err(err) => {
				let _ = resp_tx.send(WorkerResponse::Error(format!("{err:#}")));
			},
		}
	}

	fn handle_hit_test_request(
		backend: &mut dyn CaptureBackend,
		resp_tx: &Sender<WorkerResponse>,
		last_hit_test: Option<(MonitorRect, GlobalPoint, u64)>,
	) {
		if let Some((monitor, point, request_id)) = last_hit_test {
			let rect = backend.hit_test_window_in_monitor(monitor, point).unwrap_or_default();
			let _ =
				resp_tx.send(WorkerResponse::HitTestWindow { monitor, point, request_id, rect });
		}
	}

	fn map_try_send_error(err: TrySendError<WorkerRequest>) -> WorkerRequestSendError {
		match err {
			TrySendError::Full(_) => WorkerRequestSendError::Full,
			TrySendError::Disconnected(_) => WorkerRequestSendError::Disconnected,
		}
	}

	pub(crate) fn request_refresh_monitor_image(&self, monitor: MonitorRect) -> bool {
		self.req_tx.try_send(WorkerRequest::RefreshMonitorImage { monitor }).is_ok()
	}

	pub(crate) fn request_refresh_window_list(&self) -> bool {
		self.req_tx.try_send(WorkerRequest::RefreshWindowList).is_ok()
	}

	pub(crate) fn request_freeze_capture(&self, monitor: MonitorRect) -> bool {
		self.req_tx.try_send(WorkerRequest::FreezeCapture { monitor }).is_ok()
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

	pub(crate) fn try_recv(&self) -> Option<WorkerResponse> {
		match self.resp_rx.try_recv() {
			Ok(msg) => Some(msg),
			Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
		}
	}
}

#[derive(Debug)]
pub(crate) enum WorkerRequestSendError {
	Full,
	Disconnected,
}
