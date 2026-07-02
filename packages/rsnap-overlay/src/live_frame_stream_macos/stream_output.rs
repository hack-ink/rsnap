use std::collections::VecDeque;
use std::sync::{
	Arc, Mutex,
	atomic::{AtomicU64, Ordering},
};
use std::time::Instant;

use objc2::rc::Retained;
use objc2::{AnyThread, DefinedClass};
use objc2_core_media::CMSampleBuffer;
use objc2_foundation::{NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{SCStream, SCStreamDelegate, SCStreamOutput, SCStreamOutputType};

use crate::live_frame_stream_macos::STREAM_FRAME_QUEUE_CAPACITY;
use crate::live_frame_stream_macos::frame_store::SharedLatestFrame;
use crate::live_frame_stream_macos::live_frame_buffer::{
	QueuedPixelBufferFrame, SharedPixelBuffer,
};

objc2::define_class!(
	#[unsafe(super = NSObject)]
	#[thread_kind = objc2::AnyThread]
	#[ivars = StreamOutputIvars]
	pub(super) struct StreamOutput;

	unsafe impl NSObjectProtocol for StreamOutput {}

	unsafe impl SCStreamDelegate for StreamOutput {
		#[unsafe(method(stream:didStopWithError:))]
		fn stream_did_stop_with_error(&self, _stream: &SCStream, error: &NSError) {
			tracing::info!(
				op = "live_frame_stream.stopped_with_error",
				monitor_id = self.ivars().monitor_id,
				error_code = error.code(),
				error_domain = %error.domain(),
				error_description = %error.localizedDescription(),
				"ScreenCaptureKit stopped delivering frames for the live stream."
			);
		}
	}

	unsafe impl SCStreamOutput for StreamOutput {
		#[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
		fn stream_did_output_sample_buffer_of_type(
			&self,
			_stream: &SCStream,
			sample_buffer: &CMSampleBuffer,
			r#type: SCStreamOutputType,
		) {
			if r#type != SCStreamOutputType::Screen {
				return;
			}

			let Some(image_buffer) = (unsafe { sample_buffer.image_buffer() }) else {
				return;
			};
			let frame_seq =
				self.ivars().frame_seq_counter.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
			let frame = QueuedPixelBufferFrame {
				frame_seq,
				stream_generation: self.ivars().stream_generation,
				captured_at: Instant::now(),
				pixel_buffer: SharedPixelBuffer(image_buffer),
			};
			let mut frames = match self.ivars().frames.lock() {
				Ok(guard) => guard,
				Err(poisoned) => poisoned.into_inner(),
			};

			if frames.len() >= STREAM_FRAME_QUEUE_CAPACITY {
				frames.pop_front();
			}
			frames.push_back(frame.clone());
			drop(frames);
			let store_outcome =
				self.ivars().shared_latest_frame.store(self.ivars().monitor_id, &frame);
			if store_outcome.completed_ensure || store_outcome.completed_refresh {
				tracing::info!(
					op = "live_frame_stream.frame_received",
					monitor_id = self.ivars().monitor_id,
					frame_seq,
					completed_ensure = store_outcome.completed_ensure,
					completed_refresh = store_outcome.completed_refresh,
					"Received a ScreenCaptureKit frame that satisfied a pending ensure or refresh request."
				);
			}

			if let Some(frame_waker) = self.ivars().frame_waker.as_ref() {
				frame_waker();
			}
		}
	}
);

pub(super) struct StreamOutputIvars {
	monitor_id: u32,
	stream_generation: u64,
	frames: Mutex<VecDeque<QueuedPixelBufferFrame>>,
	frame_seq_counter: Arc<AtomicU64>,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	shared_latest_frame: Arc<SharedLatestFrame>,
}
impl StreamOutputIvars {
	fn new(
		monitor_id: u32,
		stream_generation: u64,
		frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
		frame_seq_counter: Arc<AtomicU64>,
		shared_latest_frame: Arc<SharedLatestFrame>,
	) -> Self {
		Self {
			monitor_id,
			stream_generation,
			frames: Mutex::new(VecDeque::with_capacity(STREAM_FRAME_QUEUE_CAPACITY)),
			frame_seq_counter,
			frame_waker,
			shared_latest_frame,
		}
	}
}

impl StreamOutput {
	pub(super) fn new(
		monitor_id: u32,
		stream_generation: u64,
		frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
		frame_seq_counter: Arc<AtomicU64>,
		shared_latest_frame: Arc<SharedLatestFrame>,
	) -> Retained<Self> {
		let this = Self::alloc().set_ivars(StreamOutputIvars::new(
			monitor_id,
			stream_generation,
			frame_waker,
			frame_seq_counter,
			shared_latest_frame,
		));

		unsafe { objc2::msg_send![super(this), init] }
	}

	pub(super) fn latest_frame(&self) -> Option<QueuedPixelBufferFrame> {
		match self.ivars().frames.lock() {
			Ok(guard) => guard.back().cloned(),
			Err(poisoned) => poisoned.into_inner().back().cloned(),
		}
	}

	pub(super) fn latest_pixel_buffer(&self) -> Option<SharedPixelBuffer> {
		self.latest_frame().map(|frame| frame.pixel_buffer)
	}

	pub(super) fn queued_frames_after_seq(
		&self,
		after_frame_seq: u64,
	) -> Vec<QueuedPixelBufferFrame> {
		match self.ivars().frames.lock() {
			Ok(guard) => {
				guard.iter().filter(|frame| frame.frame_seq > after_frame_seq).cloned().collect()
			},
			Err(poisoned) => poisoned
				.into_inner()
				.iter()
				.filter(|frame| frame.frame_seq > after_frame_seq)
				.cloned()
				.collect(),
		}
	}
}
