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

	#[cfg(test)]
	pub(super) fn handle_overlay_native_capture_input_user_event_for_test(&mut self) {
		let control = self.handle_overlay_native_capture_input_ready();

		self.handle_overlay_control(control);
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
	use std::sync::Arc;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use std::time::{Duration, Instant};

	use winit::event::{ElementState, MouseButton};

	use crate::app::capture_host_macos::OverlayNativeCaptureInputBuffer;
	use crate::app::scroll_input_macos::{ScrollInputObserverLifecycle, SharedScrollInputState};
	use crate::app::{App, OverlayEventProxy, UserEvent};
	use crate::settings::AppSettings;
	use rsnap_overlay::{
		GlobalPoint, MacOSNativeCaptureInputEvent, MonitorRect, OverlayConfig, OverlaySession,
		RectPoints, WindowListSnapshot, WindowRect,
	};

	fn test_monitor() -> MonitorRect {
		MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		}
	}

	fn single_window_list_snapshot(
		monitor: MonitorRect,
		capture_rect: RectPoints,
		window_id: u32,
	) -> Arc<WindowListSnapshot> {
		Arc::new(WindowListSnapshot {
			captured_at: Instant::now(),
			windows: Arc::new(vec![WindowRect {
				window_id: Some(window_id),
				x: i64::from(monitor.origin.x) + i64::from(capture_rect.x),
				y: i64::from(monitor.origin.y) + i64::from(capture_rect.y),
				width: i64::from(capture_rect.width),
				height: i64::from(capture_rect.height),
			}]),
		})
	}

	fn test_app() -> (App, Arc<AtomicUsize>) {
		let wake_count = Arc::new(AtomicUsize::new(0));
		let settings = AppSettings::default();
		let capture_hotkey = settings.capture_hotkey();
		let overlay_proxy = OverlayEventProxy::for_test(Arc::new({
			let wake_count = Arc::clone(&wake_count);

			move |event| {
				if matches!(event, UserEvent::OverlayNativeCaptureInput) {
					wake_count.fetch_add(1, Ordering::AcqRel);
				}

				Ok(())
			}
		}));
		let app = App::new(
			capture_hotkey,
			settings,
			None,
			None,
			overlay_proxy,
			Arc::new(ScrollInputObserverLifecycle::default()),
			Arc::new(SharedScrollInputState::default()),
		);

		(app, wake_count)
	}

	fn queued_native_input_count(app: &App) -> usize {
		app.overlay_native_capture_input_buffer
			.queue
			.lock()
			.expect("native input queue lock should be available")
			.len()
	}

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

	#[test]
	fn host_buffered_native_input_user_event_drains_into_active_overlay_session() {
		let (mut app, wake_count) = test_app();
		let monitor = test_monitor();
		let capture_rect = RectPoints::new(100, 120, 240, 320);
		let first_cursor = GlobalPoint::new(180, 220);
		let second_cursor = GlobalPoint::new(260, 310);
		let mut session = OverlaySession::with_config(OverlayConfig::default());

		session.debug_prepare_live_test_session(monitor);
		session.debug_set_window_list_snapshot(single_window_list_snapshot(
			monitor,
			capture_rect,
			42,
		));

		app.overlay_session_generation = 7;
		app.overlay_session = Some(session);

		let host = app.build_overlay_capture_host();

		host.debug_dispatch_native_capture_input(
			MacOSNativeCaptureInputEvent::OverlayPointerMoved { monitor, global: first_cursor },
		);
		host.debug_dispatch_native_capture_input(
			MacOSNativeCaptureInputEvent::OverlayPointerMoved { monitor, global: second_cursor },
		);

		assert_eq!(queued_native_input_count(&app), 2);
		assert!(app.overlay_native_capture_input_buffer.event_pending.load(Ordering::Acquire));
		assert_eq!(wake_count.load(Ordering::Acquire), 1);

		app.handle_overlay_native_capture_input_user_event_for_test();

		assert_eq!(queued_native_input_count(&app), 0);
		assert!(!app.overlay_native_capture_input_buffer.event_pending.load(Ordering::Acquire));

		let session = app.overlay_session.as_ref().expect("overlay session should still be active");

		assert_eq!(session.debug_cursor(), Some(second_cursor));
		assert_eq!(session.debug_hovered_window_rect(), Some((monitor.id, capture_rect)));
	}

	#[test]
	fn host_buffered_native_input_drops_stale_generation_events_for_newer_session() {
		let (mut app, wake_count) = test_app();
		let monitor = test_monitor();
		let stale_cursor = GlobalPoint::new(180, 220);
		let mut session = OverlaySession::with_config(OverlayConfig::default());

		session.debug_prepare_live_test_session(monitor);

		app.overlay_session_generation = 7;
		app.overlay_session = Some(session);

		let host = app.build_overlay_capture_host();

		host.debug_dispatch_native_capture_input(
			MacOSNativeCaptureInputEvent::OverlayPointerMoved { monitor, global: stale_cursor },
		);

		assert_eq!(queued_native_input_count(&app), 1);
		assert!(app.overlay_native_capture_input_buffer.event_pending.load(Ordering::Acquire));
		assert_eq!(wake_count.load(Ordering::Acquire), 1);

		app.overlay_session_generation = 8;

		let mut newer_session = OverlaySession::with_config(OverlayConfig::default());

		newer_session.debug_prepare_live_test_session(monitor);

		app.overlay_session = Some(newer_session);

		app.handle_overlay_native_capture_input_user_event_for_test();

		assert_eq!(queued_native_input_count(&app), 0);
		assert!(!app.overlay_native_capture_input_buffer.event_pending.load(Ordering::Acquire));

		let session =
			app.overlay_session.as_ref().expect("newer overlay session should still be active");

		assert_eq!(session.debug_cursor(), None);
		assert_eq!(session.debug_hovered_window_rect(), None);
	}

	#[test]
	fn host_buffered_native_drag_selection_commits_display_first_frozen_entry() {
		let (mut app, wake_count) = test_app();
		let monitor = test_monitor();
		let press_global = GlobalPoint::new(180, 220);
		let drag_global = GlobalPoint::new(420, 460);
		let capture_rect = monitor
			.local_rect_from_points(press_global, drag_global)
			.expect("drag should produce a capture rect");
		let mut session = OverlaySession::with_config(OverlayConfig::default());

		session.debug_prepare_live_test_session(monitor);
		session.debug_seed_macos_live_stream_snapshot(
			monitor,
			Instant::now() + Duration::from_secs(1),
		);

		app.overlay_session_generation = 11;
		app.overlay_session = Some(session);

		let host = app.build_overlay_capture_host();

		host.debug_dispatch_native_capture_input(MacOSNativeCaptureInputEvent::OverlayMouseInput {
			monitor,
			global: press_global,
			button: MouseButton::Left,
			state: ElementState::Pressed,
		});

		assert_eq!(wake_count.load(Ordering::Acquire), 1);

		app.handle_overlay_native_capture_input_user_event_for_test();
		host.debug_dispatch_native_capture_input(
			MacOSNativeCaptureInputEvent::OverlayPointerMoved { monitor, global: drag_global },
		);

		assert_eq!(wake_count.load(Ordering::Acquire), 2);

		app.handle_overlay_native_capture_input_user_event_for_test();

		let session =
			app.overlay_session.as_ref().expect("overlay session should remain active during drag");

		assert_eq!(session.debug_drag_rect(), Some((monitor.id, capture_rect)));

		host.debug_dispatch_native_capture_input(MacOSNativeCaptureInputEvent::OverlayMouseInput {
			monitor,
			global: drag_global,
			button: MouseButton::Left,
			state: ElementState::Released,
		});

		assert_eq!(wake_count.load(Ordering::Acquire), 3);

		app.handle_overlay_native_capture_input_user_event_for_test();

		let session = app
			.overlay_session
			.as_ref()
			.expect("overlay session should remain active after frozen handoff");

		assert!(session.debug_is_frozen_mode());
		assert_eq!(session.debug_frozen_capture_rect(), Some(capture_rect));
		assert!(session.debug_has_frozen_display_image());
		assert!(session.debug_has_frozen_export_image());
		assert!(!session.debug_capture_windows_hidden());
	}
}
