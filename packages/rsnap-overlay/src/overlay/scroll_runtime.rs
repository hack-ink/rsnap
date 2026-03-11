use super::*;

impl OverlaySession {
	pub(super) fn maybe_tick_scroll_capture(&mut self) {
		if !self.scroll_capture.active || self.scroll_capture.paused {
			return;
		}

		#[cfg(target_os = "macos")]
		{
			self.keep_scroll_capture_worker_region_symbols_referenced();
			self.sync_scroll_overlay_mouse_passthrough_window(Instant::now());

			let _ = self.try_consume_scroll_stream_frame();
		}
		#[cfg(not(target_os = "macos"))]
		{
			if self.scroll_capture.inflight_request_id.is_some() {
				return;
			}

			let now = Instant::now();
			let Some(next_sample_at) = self.scroll_capture.next_sample_at else {
				self.scroll_capture.next_sample_at = Some(now + SCROLL_CAPTURE_SAMPLE_INTERVAL);

				return;
			};

			if now < next_sample_at {
				return;
			}

			let Some(monitor) = self.scroll_capture.monitor else {
				self.scroll_capture_set_error("Scroll capture lost its monitor.");

				return;
			};
			let Some(capture_rect) = self.scroll_capture.capture_rect_pixels else {
				self.scroll_capture_set_error("Scroll capture lost its region.");

				return;
			};
			let Some(worker) = self.worker.as_ref() else {
				self.scroll_capture_set_error("Scroll capture worker is unavailable.");

				return;
			};
			let request_id = self.scroll_capture.next_request_id.wrapping_add(1);

			match worker.request_capture_monitor_region(monitor, capture_rect, request_id) {
				Ok(()) => {
					self.scroll_capture.next_request_id = request_id;
					self.scroll_capture.inflight_request_id = Some(request_id);
					#[cfg(target_os = "macos")]
					{
						self.scroll_capture.inflight_request_observation =
							Some(InflightScrollCaptureObservation {
								input_direction: self.scroll_capture.input_direction,
								was_observable: self.scroll_capture_input_allows_observation(),
								external_input_seq: self
									.scroll_capture
									.last_external_scroll_input_seq,
							});
					}
					self.scroll_capture.next_sample_at = Some(now + SCROLL_CAPTURE_SAMPLE_INTERVAL);
				},
				Err(WorkerRequestSendError::Full) => {
					self.scroll_capture.next_sample_at =
						Some(now + SCROLL_CAPTURE_SAMPLE_INTERVAL.saturating_mul(2));
				},
				Err(WorkerRequestSendError::Disconnected) => {
					self.scroll_capture_set_error("Scroll capture worker disconnected.");
				},
			}
		}
	}

	#[cfg(target_os = "macos")]
	pub(super) fn try_consume_scroll_stream_frame(&mut self) -> bool {
		let Some(monitor) = self.scroll_capture.monitor else {
			self.scroll_capture_set_error("Scroll capture lost its monitor.");

			return true;
		};
		let Some(capture_rect) = self.scroll_capture.capture_rect_pixels else {
			self.scroll_capture_set_error("Scroll capture lost its region.");

			return true;
		};
		let Some(live_stream) = self.scroll_capture.live_stream.as_mut() else {
			return false;
		};
		let last_frame_seq = self.scroll_capture.last_stream_frame_seq;
		let Some(frames) =
			live_stream.ordered_rgba_regions_after_seq(monitor, capture_rect, last_frame_seq)
		else {
			tracing::info!(
				op = "scroll_capture.stream_frame_empty",
				last_frame_seq,
				"Did not receive a newer live-stream frame for scroll-capture observation."
			);

			return false;
		};
		let Some(newest_frame_seq) = frames.last().map(|frame| frame.frame_seq) else {
			tracing::info!(
				op = "scroll_capture.stream_frame_empty",
				last_frame_seq,
				"Did not receive a newer live-stream frame for scroll-capture observation."
			);

			return false;
		};

		tracing::info!(
			op = "scroll_capture.stream_frame_ready",
			prior_frame_seq = last_frame_seq,
			frame_seq = newest_frame_seq,
			frame_gap = newest_frame_seq.saturating_sub(last_frame_seq),
			frame_count = frames.len(),
			"Pulled live-stream frame for scroll-capture observation."
		);

		for frame in frames {
			self.drain_external_scroll_input_events_through(frame.captured_at);

			self.scroll_capture.last_stream_frame_seq = frame.frame_seq;

			self.handle_scroll_capture_frame(
				frame.image,
				ScrollCaptureFrameSource::LiveStream { frame_seq: frame.frame_seq },
				false,
				frame.captured_at,
			);
		}

		true
	}

	#[cfg(target_os = "macos")]
	pub fn handle_scroll_stream_frame_ready(&mut self) -> OverlayControl {
		if self.scroll_capture.active && !self.scroll_capture.paused {
			let _ = self.try_consume_scroll_stream_frame();
		}

		OverlayControl::Continue
	}

	pub fn handle_worker_response_ready(&mut self) -> OverlayControl {
		self.drain_worker_responses()
	}

	#[cfg(target_os = "macos")]
	pub(super) fn drain_external_scroll_input_events_through(&mut self, through: Instant) {
		let Some(reader) = self.scroll_capture.external_scroll_input_drain_reader.clone() else {
			return;
		};

		for (seq, recorded_at, global_x, global_y, delta_y, gesture_active, gesture_ended) in
			reader(self.scroll_capture.last_external_scroll_input_seq, through)
		{
			if seq <= self.scroll_capture.last_external_scroll_input_seq {
				continue;
			}

			let inferred_direction = Self::scroll_capture_direction_from_delta_y(delta_y);
			let input_age_ms =
				u64::try_from(through.saturating_duration_since(recorded_at).as_millis())
					.unwrap_or(u64::MAX);
			let prior_direction = self.scroll_capture.input_direction;
			let prior_gesture_active = self.scroll_capture.input_gesture_active;

			tracing::info!(
				op = "scroll_capture.replayed_input",
				seq,
				prior_seq = self.scroll_capture.last_external_scroll_input_seq,
				delta_y,
				gesture_active,
				gesture_ended,
				direction = ?inferred_direction,
				input_age_ms,
				prior_direction = ?prior_direction,
				prior_gesture_active,
				"Replayed external scroll input event into scroll capture."
			);

			self.scroll_capture.last_external_scroll_input_seq = seq;

			self.apply_external_scroll_input_delta_y(
				global_x,
				global_y,
				delta_y,
				gesture_active,
				gesture_ended,
				through,
			);
			self.refresh_live_stream_stale_grace_for_external_input(seq);

			tracing::info!(
				op = "scroll_capture.replayed_input_result",
				seq,
				recorded_at_ms_behind_pairing = u64::try_from(
					through.saturating_duration_since(recorded_at).as_millis()
				)
				.unwrap_or(u64::MAX),
				paired_at_age_ms = self.scroll_capture_input_age_ms(),
				after_direction = ?self.scroll_capture.input_direction,
				after_gesture_active = self.scroll_capture.input_gesture_active,
				"Applied replayed external scroll input event to scroll-capture state."
			);
		}
	}

	pub(super) fn handle_captured_scroll_region(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		request_id: u64,
		image: RgbaImage,
	) {
		let frame_px = image.dimensions();

		if !self.scroll_capture.active {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "inactive",
				request_id,
				paused = self.scroll_capture.paused,
				frame_px = ?frame_px,
				"Dropped worker-fed scroll-capture frame before observation."
			);

			return;
		}
		if self.scroll_capture.monitor != Some(monitor) {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "monitor_mismatch",
				request_id,
				expected_monitor_id = ?self.scroll_capture.monitor.map(|current_monitor| current_monitor.id),
				received_monitor_id = monitor.id,
				frame_px = ?frame_px,
				"Dropped worker-fed scroll-capture frame before observation."
			);

			return;
		}
		if self.scroll_capture.capture_rect_pixels != Some(rect_px) {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "rect_mismatch",
				request_id,
				expected_rect_px = ?self.scroll_capture.capture_rect_pixels,
				received_rect_px = ?rect_px,
				frame_px = ?frame_px,
				"Dropped worker-fed scroll-capture frame before observation."
			);

			return;
		}
		if self.scroll_capture.inflight_request_id != Some(request_id) {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "inflight_request_mismatch",
				request_id,
				expected_request_id = ?self.scroll_capture.inflight_request_id,
				frame_px = ?frame_px,
				"Dropped worker-fed scroll-capture frame before observation."
			);

			return;
		}

		#[cfg(target_os = "macos")]
		self.drain_external_scroll_input_events_through(Instant::now());

		#[cfg(target_os = "macos")]
		let allow_stale_input_for_request =
			self.allow_worker_frame_with_latched_request_input(request_id);
		#[cfg(not(target_os = "macos"))]
		let allow_stale_input_for_request = false;

		self.clear_scroll_capture_inflight_request();
		self.handle_scroll_capture_frame(
			image,
			ScrollCaptureFrameSource::Worker { request_id },
			allow_stale_input_for_request,
			Instant::now(),
		);
	}

	pub(super) fn handle_missing_scroll_region(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		request_id: u64,
	) {
		if !self.scroll_capture.active {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "inactive",
				request_id,
				paused = self.scroll_capture.paused,
				"Dropped worker scroll-capture no-frame notification before observation."
			);

			return;
		}
		if self.scroll_capture.monitor != Some(monitor) {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "monitor_mismatch",
				request_id,
				expected_monitor_id = ?self.scroll_capture.monitor.map(|current_monitor| current_monitor.id),
				received_monitor_id = monitor.id,
				"Dropped worker scroll-capture no-frame notification before observation."
			);

			return;
		}
		if self.scroll_capture.capture_rect_pixels != Some(rect_px) {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "rect_mismatch",
				request_id,
				expected_rect_px = ?self.scroll_capture.capture_rect_pixels,
				received_rect_px = ?rect_px,
				"Dropped worker scroll-capture no-frame notification before observation."
			);

			return;
		}
		if self.scroll_capture.inflight_request_id != Some(request_id) {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "inflight_request_mismatch",
				request_id,
				expected_request_id = ?self.scroll_capture.inflight_request_id,
				"Dropped worker scroll-capture no-frame notification before observation."
			);

			return;
		}

		self.clear_scroll_capture_inflight_request();

		tracing::info!(
			op = "scroll_capture.worker_frame_unavailable",
			request_id,
			reason = "no_new_frame",
			input_direction = ?self.scroll_capture.input_direction,
			"Worker scroll-capture request completed without a fresh frame."
		);
	}

	pub(super) fn handle_scroll_capture_frame(
		&mut self,
		frame: RgbaImage,
		source: ScrollCaptureFrameSource,
		allow_stale_input: bool,
		observation_at: Instant,
	) {
		let frame_px = frame.dimensions();

		if let Some(reason) = self.scroll_capture_observation_block_reason_at(observation_at) {
			#[cfg(target_os = "macos")]
			let allow_live_stream_stale_grace = !allow_stale_input
				&& reason == "stale_input"
				&& matches!(source, ScrollCaptureFrameSource::LiveStream { .. })
				&& self.consume_live_stream_stale_grace_if_current();
			#[cfg(not(target_os = "macos"))]
			let allow_live_stream_stale_grace = false;

			if (allow_stale_input || allow_live_stream_stale_grace) && reason == "stale_input" {
				let Some(outcome) =
					self.observe_scroll_capture_frame_with_gate(frame, true, observation_at)
				else {
					return;
				};

				self.handle_scroll_capture_frame_outcome(outcome, source, frame_px);

				return;
			}

			let input_age_ms = self.scroll_capture_input_age_ms_at(observation_at);

			tracing::info!(
				op = "scroll_capture.observation_blocked",
				frame_source = source.as_str(),
				worker_request_id = ?source.worker_request_id(),
				reason,
				frame_px = ?frame_px,
				input_direction = ?self.scroll_capture.input_direction,
				input_gesture_active = self.scroll_capture.input_gesture_active,
				input_age_ms = ?input_age_ms,
				"Skipped scroll-capture frame observation because input was not currently usable."
			);

			return;
		}

		let Some(outcome) = self.observe_scroll_capture_frame_at(frame, observation_at) else {
			return;
		};

		self.handle_scroll_capture_frame_outcome(outcome, source, frame_px);
	}

	fn handle_scroll_capture_frame_outcome(
		&mut self,
		outcome: color_eyre::Result<ScrollObserveOutcome>,
		source: ScrollCaptureFrameSource,
		frame_px: (u32, u32),
	) {
		match outcome {
			Ok(ScrollObserveOutcome::NoChange) => {
				if let Some(request_id) = source.worker_request_id() {
					tracing::info!(
						op = "scroll_capture.worker_frame_processed",
						request_id,
						outcome = "no_change",
						frame_px = ?frame_px,
						input_direction = ?self.scroll_capture.input_direction,
						"Worker-fed scroll-capture frame reached the session without changing preview or export state."
					);
				}
			},
			Ok(ScrollObserveOutcome::PreviewUpdated) => {
				if let Some(request_id) = source.worker_request_id() {
					tracing::info!(
						op = "scroll_capture.worker_frame_processed",
						request_id,
						outcome = "preview_updated",
						frame_px = ?frame_px,
						input_direction = ?self.scroll_capture.input_direction,
						"Worker-fed scroll-capture frame refreshed preview state without committing stitched growth."
					);
				}
			},
			Ok(ScrollObserveOutcome::UnsupportedDirection { direction }) => {
				let export_size = self
					.scroll_capture
					.session
					.as_ref()
					.map_or((0, 0), ScrollSession::export_dimensions);

				tracing::info!(
					op = "scroll_capture.unsupported_direction",
					frame_source = source.as_str(),
					worker_request_id = ?source.worker_request_id(),
					direction = ?direction,
					frame_px = ?frame_px,
					export_px = ?export_size,
					"Scroll-capture sample moved in an unsupported direction."
				);
			},
			Ok(ScrollObserveOutcome::Committed { direction, growth_rows }) => {
				let export_size = self
					.scroll_capture
					.session
					.as_ref()
					.map_or((0, 0), ScrollSession::export_dimensions);

				tracing::info!(
					op = "scroll_capture.committed",
					frame_source = source.as_str(),
					worker_request_id = ?source.worker_request_id(),
					direction = ?direction,
					growth_rows,
					frame_px = ?frame_px,
					export_px = ?export_size,
					"Scroll sample committed stitched growth."
				);

				self.sync_scroll_preview_segments();
				self.request_redraw_scroll_preview_window();
			},
			Err(err) => {
				self.scroll_capture_set_error(format!("{err:#}"));
			},
		}
	}

	pub(super) fn clear_scroll_capture_inflight_request(&mut self) {
		self.scroll_capture.inflight_request_id = None;
		#[cfg(target_os = "macos")]
		{
			self.scroll_capture.inflight_request_observation = None;
		}
	}

	#[cfg(target_os = "macos")]
	pub(super) fn keep_scroll_capture_worker_region_symbols_referenced(&self) {
		let _ = SCROLL_CAPTURE_SAMPLE_INTERVAL;
		let _ = OverlayWorker::request_capture_monitor_region;
	}

	#[cfg(target_os = "macos")]
	pub(super) fn allow_worker_frame_with_latched_request_input(&self, request_id: u64) -> bool {
		if self.scroll_capture.inflight_request_id != Some(request_id) {
			return false;
		}

		let Some(observation) = self.scroll_capture.inflight_request_observation else {
			return false;
		};

		if !observation.was_observable {
			return false;
		}
		if observation.external_input_seq != self.scroll_capture.last_external_scroll_input_seq {
			return false;
		}

		observation.input_direction == self.scroll_capture.input_direction
	}

	#[cfg(target_os = "macos")]
	pub(super) fn clear_incompatible_live_stream_stale_grace(&mut self) {
		let Some(grace) = self.scroll_capture.live_stream_stale_grace else {
			return;
		};
		let grace_is_current =
			grace.external_input_seq == self.scroll_capture.last_external_scroll_input_seq;
		let grace_is_compatible = self.scroll_capture.input_direction
			== Some(grace.input_direction)
			&& !self.scroll_capture.input_gesture_active
			&& grace.input_direction == ScrollDirection::Down;

		if !(grace_is_current && grace_is_compatible) {
			self.scroll_capture.live_stream_stale_grace = None;
		}
	}

	#[cfg(target_os = "macos")]
	pub(super) fn refresh_live_stream_stale_grace_for_external_input(
		&mut self,
		external_input_seq: u64,
	) {
		self.scroll_capture.live_stream_stale_grace =
			match (self.scroll_capture.input_direction, self.scroll_capture.input_gesture_active) {
				(Some(ScrollDirection::Down), false) => Some(LiveStreamStaleGrace {
					external_input_seq,
					input_direction: ScrollDirection::Down,
					remaining_stale_frames: SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES,
				}),
				_ => None,
			};
	}

	#[cfg(target_os = "macos")]
	pub(super) fn consume_live_stream_stale_grace_if_current(&mut self) -> bool {
		let Some(grace) = self.scroll_capture.live_stream_stale_grace else {
			return false;
		};

		if grace.external_input_seq != self.scroll_capture.last_external_scroll_input_seq
			|| self.scroll_capture.input_direction != Some(grace.input_direction)
			|| self.scroll_capture.input_gesture_active
			|| grace.input_direction != ScrollDirection::Down
			|| grace.remaining_stale_frames == 0
		{
			self.scroll_capture.live_stream_stale_grace = None;

			return false;
		}
		if grace.remaining_stale_frames == 1 {
			self.scroll_capture.live_stream_stale_grace = None;
		} else {
			self.scroll_capture.live_stream_stale_grace = Some(LiveStreamStaleGrace {
				remaining_stale_frames: grace.remaining_stale_frames - 1,
				..grace
			});
		}

		true
	}
}
