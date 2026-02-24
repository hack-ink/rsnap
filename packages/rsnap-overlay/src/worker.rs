use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, channel, sync_channel};
use std::thread::spawn;

use image::RgbaImage;

use crate::backend::CaptureBackend;
use crate::state::{GlobalPoint, MonitorRect, Rgb};

#[derive(Debug)]
pub(crate) enum WorkerRequest {
	SampleRgb { monitor: MonitorRect, point: GlobalPoint },
	FreezeCapture { monitor: MonitorRect },
}

#[derive(Debug)]
pub(crate) enum WorkerResponse {
	SampledRgb { monitor: MonitorRect, point: GlobalPoint, rgb: Option<Rgb> },
	CapturedFreeze { monitor: MonitorRect, image: RgbaImage },
	Error(String),
}

pub(crate) struct OverlayWorker {
	req_tx: SyncSender<WorkerRequest>,
	resp_rx: Receiver<WorkerResponse>,
}

impl OverlayWorker {
	pub(crate) fn new(mut backend: Box<dyn CaptureBackend>) -> Self {
		let (req_tx, req_rx) = sync_channel::<WorkerRequest>(8);
		let (resp_tx, resp_rx) = channel::<WorkerResponse>();

		spawn(move || {
			while let Ok(first) = req_rx.recv() {
				let mut last_sample: Option<(MonitorRect, GlobalPoint)> = None;
				let mut last_freeze: Option<MonitorRect> = None;

				match first {
					WorkerRequest::SampleRgb { monitor, point } => {
						last_sample = Some((monitor, point))
					},
					WorkerRequest::FreezeCapture { monitor } => last_freeze = Some(monitor),
				}

				while let Ok(next) = req_rx.try_recv() {
					match next {
						WorkerRequest::SampleRgb { monitor, point } => {
							last_sample = Some((monitor, point))
						},
						WorkerRequest::FreezeCapture { monitor } => last_freeze = Some(monitor),
					}
				}

				if let Some(monitor) = last_freeze {
					match backend.capture_monitor(monitor) {
						Ok(image) => {
							let _ = resp_tx.send(WorkerResponse::CapturedFreeze { monitor, image });
						},
						Err(err) => {
							let _ = resp_tx.send(WorkerResponse::Error(format!("{err:#}")));
						},
					}
					continue;
				}

				if let Some((monitor, point)) = last_sample {
					match backend.pixel_rgb_in_monitor(monitor, point) {
						Ok(rgb) => {
							let _ =
								resp_tx.send(WorkerResponse::SampledRgb { monitor, point, rgb });
						},
						Err(err) => {
							let _ = resp_tx.send(WorkerResponse::Error(format!("{err:#}")));
						},
					}
				}
			}
		});

		Self { req_tx, resp_rx }
	}

	pub(crate) fn try_sample_rgb(&self, monitor: MonitorRect, point: GlobalPoint) {
		let _ = self.req_tx.try_send(WorkerRequest::SampleRgb { monitor, point });
	}

	pub(crate) fn request_freeze_capture(&self, monitor: MonitorRect) -> bool {
		self.req_tx.try_send(WorkerRequest::FreezeCapture { monitor }).is_ok()
	}

	pub(crate) fn try_recv(&self) -> Option<WorkerResponse> {
		match self.resp_rx.try_recv() {
			Ok(msg) => Some(msg),
			Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
		}
	}
}
