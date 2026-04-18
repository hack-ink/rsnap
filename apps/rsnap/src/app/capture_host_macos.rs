use std::collections::VecDeque;
use std::sync::{
	Arc, Mutex,
	atomic::{AtomicBool, Ordering},
};

use crate::app::{App, UserEvent};
use rsnap_overlay::{MacOSCaptureHost, MacOSNativeCaptureInputEvent, OverlayControl, OverlayExit};

#[derive(Clone)]
pub(super) struct OverlayNativeCaptureInputBuffer {
	queue: Arc<Mutex<VecDeque<(u64, MacOSNativeCaptureInputEvent)>>>,
	event_pending: Arc<AtomicBool>,
}
impl OverlayNativeCaptureInputBuffer {
	pub(super) fn new() -> Self {
		Self {
			queue: Arc::new(Mutex::new(VecDeque::new())),
			event_pending: Arc::new(AtomicBool::new(false)),
		}
	}

	fn enqueue(&self, generation: u64, event: MacOSNativeCaptureInputEvent) -> bool {
		match self.queue.lock() {
			Ok(mut queue) => queue.push_back((generation, event)),
			Err(poisoned) => {
				tracing::warn!(
					op = "capture.native_input_queue_poisoned",
					"Dropping native capture input event because the queue lock was poisoned."
				);

				poisoned.into_inner().clear();

				return false;
			},
		}

		self.event_pending
			.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
			.is_ok()
	}

	fn finish_send(&self) {
		self.event_pending.store(false, Ordering::Release);
	}

	fn drain(&self) -> Vec<(u64, MacOSNativeCaptureInputEvent)> {
		match self.queue.lock() {
			Ok(mut queue) => queue.drain(..).collect(),
			Err(poisoned) => {
				tracing::warn!(
					op = "capture.native_input_queue_poisoned",
					"Draining native capture input from a poisoned queue."
				);

				poisoned.into_inner().drain(..).collect()
			},
		}
	}

	fn reset(&self) {
		self.finish_send();

		match self.queue.lock() {
			Ok(mut queue) => queue.clear(),
			Err(poisoned) => {
				tracing::warn!(
					op = "capture.native_input_queue_poisoned",
					"Resetting native capture input from a poisoned queue."
				);

				poisoned.into_inner().clear();
			},
		}
	}
}

impl App {
	pub(super) fn build_overlay_capture_host(&self) -> MacOSCaptureHost {
		let overlay_proxy = self.overlay_proxy.clone();
		let native_input_buffer = self.overlay_native_capture_input_buffer.clone();
		let generation = self.overlay_session_generation;

		MacOSCaptureHost::new(Arc::new(move |event| {
			if native_input_buffer.enqueue(generation, event)
				&& overlay_proxy.send_event(UserEvent::OverlayNativeCaptureInput).is_err()
			{
				native_input_buffer.finish_send();
			}
		}))
	}

	pub(super) fn finish_coalesced_overlay_native_capture_input_send(&self) {
		self.overlay_native_capture_input_buffer.finish_send();
	}

	pub(super) fn drain_overlay_native_capture_input_events(
		&self,
	) -> Vec<(u64, MacOSNativeCaptureInputEvent)> {
		self.overlay_native_capture_input_buffer.drain()
	}

	pub(super) fn handle_overlay_native_capture_input_ready(&mut self) -> OverlayControl {
		self.finish_coalesced_overlay_native_capture_input_send();

		for (generation, event) in self.drain_overlay_native_capture_input_events() {
			if generation != self.overlay_session_generation {
				continue;
			}

			let Some(session) = self.overlay_session.as_mut() else {
				break;
			};
			let control = session.handle_native_capture_input_event(event);

			if !matches!(control, OverlayControl::Continue) {
				return control;
			}
		}

		OverlayControl::Continue
	}

	pub(super) fn reset_overlay_native_capture_input_dispatch(&self) {
		self.overlay_native_capture_input_buffer.reset();
	}

	pub(super) fn sync_overlay_capture_host(&mut self) {
		let sync_result = match (self.overlay_session.as_mut(), self.overlay_capture_host.as_mut())
		{
			(Some(session), Some(host)) => session.sync_macos_capture_host(host),
			_ => return,
		};

		if let Err(err) = sync_result {
			self.end_overlay_session(OverlayExit::Error(err));
		}
	}

	pub(super) fn teardown_overlay_capture_host(&mut self) {
		if let (Some(session), Some(host)) =
			(self.overlay_session.as_mut(), self.overlay_capture_host.as_mut())
		{
			session.teardown_macos_capture_host(host);
		}

		self.overlay_capture_host = None;
	}
}

#[cfg(test)]
mod tests {
	use crate::app::capture_host_macos::OverlayNativeCaptureInputBuffer;
	use rsnap_overlay::MacOSNativeCaptureInputEvent;

	#[test]
	fn native_capture_input_buffer_coalesces_multiple_events_behind_one_wakeup() {
		let buffer = OverlayNativeCaptureInputBuffer::new();

		assert!(buffer.enqueue(7, MacOSNativeCaptureInputEvent::ToolbarPointerLeft));
		assert!(!buffer.enqueue(7, MacOSNativeCaptureInputEvent::ToolbarPointerLeft));

		buffer.finish_send();

		assert_eq!(
			buffer.drain(),
			vec![
				(7, MacOSNativeCaptureInputEvent::ToolbarPointerLeft),
				(7, MacOSNativeCaptureInputEvent::ToolbarPointerLeft),
			]
		);
	}

	#[test]
	fn native_capture_input_buffer_reset_clears_pending_and_buffered_events() {
		let buffer = OverlayNativeCaptureInputBuffer::new();

		assert!(buffer.enqueue(5, MacOSNativeCaptureInputEvent::ToolbarPointerLeft));

		buffer.reset();

		assert!(buffer.drain().is_empty());
		assert!(buffer.enqueue(6, MacOSNativeCaptureInputEvent::ToolbarPointerLeft));
	}
}
