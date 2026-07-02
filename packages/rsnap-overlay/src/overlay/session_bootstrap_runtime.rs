use std::time::{Duration, Instant};

use crate::overlay::{
	CURSOR_POLL_INTERVAL_MIN, LIVE_WINDOW_LIST_REFRESH_INTERVAL, OverlayConfig, OverlaySession,
	OverlayState,
};

pub(super) struct InitialSessionRuntime {
	live_bg_request_interval: Duration,
	window_list_refresh_interval: Duration,
	now: Instant,
	loupe_sample_side_px: u32,
	state: OverlayState,
}

impl OverlaySession {
	fn initial_timing() -> (Duration, Duration, Instant) {
		(Duration::from_millis(500), LIVE_WINDOW_LIST_REFRESH_INTERVAL, Instant::now())
	}

	pub(super) fn apply_initial_session_runtime(&mut self, runtime: InitialSessionRuntime) {
		self.state = runtime.state;
		self.last_hud_window_move_at = runtime.now;
		self.last_loupe_window_move_at = runtime.now;
		self.last_toolbar_window_move_at = runtime.now;
		self.last_present_at = runtime.now;
		self.last_live_cursor_poll_at = runtime.now - CURSOR_POLL_INTERVAL_MIN;
		self.last_frozen_cursor_poll_at = runtime.now - CURSOR_POLL_INTERVAL_MIN;
		self.last_window_list_refresh_request_at =
			runtime.now - runtime.window_list_refresh_interval;
		self.window_list_refresh_interval = runtime.window_list_refresh_interval;
		self.last_live_bg_request_at = runtime.now - runtime.live_bg_request_interval;
		self.live_bg_request_interval = runtime.live_bg_request_interval;
		self.event_loop_last_progress_at = runtime.now;
		self.loupe_patch_width_px = runtime.loupe_sample_side_px;
		self.loupe_patch_height_px = runtime.loupe_sample_side_px;
	}

	fn overlay_state_with_loupe_patch(loupe_sample_side_px: u32) -> OverlayState {
		let mut state = OverlayState::new();

		state.reset_for_start(loupe_sample_side_px);

		state
	}

	fn overlay_state_with_config(config: &OverlayConfig) -> (u32, OverlayState) {
		let loupe_sample_side_px =
			Self::normalized_loupe_sample_side_px(config.loupe_sample_side_px);

		(loupe_sample_side_px, Self::overlay_state_with_loupe_patch(loupe_sample_side_px))
	}

	pub(super) fn initial_session_runtime(config: &OverlayConfig) -> InitialSessionRuntime {
		let (live_bg_request_interval, window_list_refresh_interval, now) = Self::initial_timing();
		let (loupe_sample_side_px, state) = Self::overlay_state_with_config(config);

		InitialSessionRuntime {
			live_bg_request_interval,
			window_list_refresh_interval,
			now,
			loupe_sample_side_px,
			state,
		}
	}
}
