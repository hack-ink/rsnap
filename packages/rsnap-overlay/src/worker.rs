use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError};

use image::RgbaImage;

use crate::backend::CaptureBackend;
use crate::state::{GlobalPoint, MonitorRect, Rgb};

#[derive(Debug)]
pub(crate) enum WorkerRequest {
	SampleRgb { monitor: MonitorRect, point: GlobalPoint },
	SampleLoupe { monitor: MonitorRect, point: GlobalPoint, width_px: u32, height_px: u32 },
	FreezeCapture { monitor: MonitorRect },
	EncodePng { image: RgbaImage },
}

#[derive(Debug)]
pub(crate) enum WorkerResponse {
	SampledRgb {
		monitor: MonitorRect,
		point: GlobalPoint,
		rgb: Option<Rgb>,
	},
	SampledLoupe {
		monitor: MonitorRect,
		point: GlobalPoint,
		rgb: Option<Rgb>,
		patch: Option<RgbaImage>,
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
	pub(crate) fn new(mut backend: Box<dyn CaptureBackend>) -> Self {
		let (req_tx, req_rx) = std::sync::mpsc::sync_channel(64);
		let (resp_tx, resp_rx) = std::sync::mpsc::channel();

		std::thread::spawn(move || {
			while let Ok(first) = req_rx.recv() {
				let mut last_sample: Option<(MonitorRect, GlobalPoint)> = None;
				let mut last_loupe: Option<(MonitorRect, GlobalPoint, u32, u32)> = None;
				let mut last_freeze: Option<MonitorRect> = None;
				let mut last_encode: Option<RgbaImage> = None;

				match first {
					WorkerRequest::SampleRgb { monitor, point } => {
						last_sample = Some((monitor, point))
					},
					WorkerRequest::SampleLoupe { monitor, point, width_px, height_px } => {
						last_loupe = Some((monitor, point, width_px, height_px));
					},
					WorkerRequest::FreezeCapture { monitor } => last_freeze = Some(monitor),
					WorkerRequest::EncodePng { image } => last_encode = Some(image),
				}

				while let Ok(next) = req_rx.try_recv() {
					match next {
						WorkerRequest::SampleRgb { monitor, point } => {
							last_sample = Some((monitor, point))
						},
						WorkerRequest::SampleLoupe { monitor, point, width_px, height_px } => {
							last_loupe = Some((monitor, point, width_px, height_px));
						},
						WorkerRequest::FreezeCapture { monitor } => last_freeze = Some(monitor),
						WorkerRequest::EncodePng { image } => last_encode = Some(image),
					}
				}

				if let Some(image) = last_encode {
					match crate::png::rgba_image_to_png_bytes(&image) {
						Ok(png_bytes) => {
							let _ = resp_tx.send(WorkerResponse::EncodedPng { png_bytes });
						},
						Err(err) => {
							let _ = resp_tx.send(WorkerResponse::Error(format!("{err:#}")));
						},
					}

					continue;
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
				if let Some((monitor, point, width_px, height_px)) = last_loupe {
					let rgb = match backend.pixel_rgb_in_monitor(monitor, point) {
						Ok(rgb) => rgb,
						Err(err) => {
							let _ = resp_tx.send(WorkerResponse::Error(format!("{err:#}")));

							continue;
						},
					};
					let patch =
						match backend.rgba_patch_in_monitor(monitor, point, width_px, height_px) {
							Ok(patch) => patch,
							Err(err) => {
								let _ = resp_tx.send(WorkerResponse::Error(format!("{err:#}")));

								continue;
							},
						};
					let _ =
						resp_tx.send(WorkerResponse::SampledLoupe { monitor, point, rgb, patch });

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

	fn map_try_send_error(err: TrySendError<WorkerRequest>) -> WorkerRequestSendError {
		match err {
			TrySendError::Full(_) => WorkerRequestSendError::Full,
			TrySendError::Disconnected(_) => WorkerRequestSendError::Disconnected,
		}
	}

	pub(crate) fn try_sample_rgb(
		&self,
		monitor: MonitorRect,
		point: GlobalPoint,
	) -> Result<(), WorkerRequestSendError> {
		let request = WorkerRequest::SampleRgb { monitor, point };

		self.req_tx.try_send(request).map_err(Self::map_try_send_error)
	}

	pub(crate) fn try_sample_loupe(
		&self,
		monitor: MonitorRect,
		point: GlobalPoint,
		width_px: u32,
		height_px: u32,
	) -> Result<(), WorkerRequestSendError> {
		let request = WorkerRequest::SampleLoupe { monitor, point, width_px, height_px };

		self.req_tx.try_send(request).map_err(Self::map_try_send_error)
	}

	pub(crate) fn request_freeze_capture(&self, monitor: MonitorRect) -> bool {
		self.req_tx.try_send(WorkerRequest::FreezeCapture { monitor }).is_ok()
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
