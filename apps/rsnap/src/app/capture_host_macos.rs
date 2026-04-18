use std::sync::Arc;

use crate::app::{App, UserEvent};
use rsnap_overlay::{MacOSCaptureHost, OverlayExit};

impl App {
	pub(super) fn build_overlay_capture_host(&self) -> MacOSCaptureHost {
		let overlay_proxy = self.overlay_proxy.clone();
		let generation = self.overlay_session_generation;

		MacOSCaptureHost::new(Arc::new(move |event| {
			let _ =
				overlay_proxy.send_event(UserEvent::OverlayNativeCaptureInput(generation, event));
		}))
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
