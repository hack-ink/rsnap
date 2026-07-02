use std::process;
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{AnyThread, Message};
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{
	SCContentFilter, SCDisplay, SCRunningApplication, SCShareableContent, SCWindow,
};

use crate::live_frame_stream_macos::{
	STREAM_ERROR_NULL_CONTENT_CODE, STREAM_ERROR_RETAIN_FAILED_CODE, STREAM_ERROR_TIMEOUT_CODE,
};
use crate::state::MonitorRect;

#[derive(Clone, Debug, Default)]
pub(super) struct StreamFilterConfig {
	pub(super) self_capture_exception_window_ids: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StreamFilterMode {
	ExcludeCurrentProcess,
	ExcludeCurrentProcessShareableWindows,
}

pub(super) struct PreparedStreamFilter {
	pub(super) filter_mode: StreamFilterMode,
	pub(super) filter: Retained<SCContentFilter>,
	pub(super) self_capture_filter_complete: bool,
	pub(super) self_capture_exception_window_ids_complete: bool,
	pub(super) excepting_window_count: usize,
	pub(super) fallback_excluded_window_count: usize,
	pub(super) missing_window_ids: Vec<u32>,
	pub(super) shareable_content_ms: u128,
	pub(super) find_display_ms: u128,
	pub(super) exception_windows_ms: u128,
	pub(super) filter_build_ms: u128,
}

struct CurrentProcessExceptionWindows {
	windows: Vec<Retained<SCWindow>>,
	fallback_excluded_windows: Vec<Retained<SCWindow>>,
	missing_window_ids: Vec<u32>,
}
impl CurrentProcessExceptionWindows {
	fn complete(&self) -> bool {
		self.missing_window_ids.is_empty()
	}
}

pub(super) fn prepare_stream_filter_for_monitor(
	monitor: MonitorRect,
	self_capture_exception_window_ids: &[u32],
) -> Option<PreparedStreamFilter> {
	let shareable_content_started_at = Instant::now();
	let content = load_shareable_content_for_monitor(monitor.id)?;
	let shareable_content_ms = shareable_content_started_at.elapsed().as_millis();
	let find_display_started_at = Instant::now();
	let display = find_display_for_monitor(&content, monitor.id)?;
	let find_display_ms = find_display_started_at.elapsed().as_millis();
	let exception_windows_started_at = Instant::now();
	let excepting_windows =
		find_current_process_exception_windows(&content, self_capture_exception_window_ids);
	let exception_windows_ms = exception_windows_started_at.elapsed().as_millis();
	let self_capture_exception_window_ids_complete = excepting_windows.complete();
	let filter_build_started_at = Instant::now();
	let prepared_filter =
		build_stream_content_filter(monitor.id, &display, &content, excepting_windows);
	let filter_build_ms = filter_build_started_at.elapsed().as_millis();

	Some(PreparedStreamFilter {
		filter_mode: prepared_filter.filter_mode,
		filter: prepared_filter.filter,
		self_capture_filter_complete: prepared_filter.self_capture_filter_complete,
		self_capture_exception_window_ids_complete,
		excepting_window_count: prepared_filter.excepting_window_count,
		fallback_excluded_window_count: prepared_filter.fallback_excluded_window_count,
		missing_window_ids: prepared_filter.missing_window_ids,
		shareable_content_ms,
		find_display_ms,
		exception_windows_ms,
		filter_build_ms,
	})
}

fn build_stream_content_filter(
	monitor_id: u32,
	display: &SCDisplay,
	content: &SCShareableContent,
	excepting_windows: CurrentProcessExceptionWindows,
) -> PreparedStreamFilter {
	let excepting_window_count = excepting_windows.windows.len();
	let fallback_excluded_window_count = excepting_windows.fallback_excluded_windows.len();
	let missing_window_ids = excepting_windows.missing_window_ids;
	let preferred_filter_mode =
		stream_filter_mode_for_current_process(missing_window_ids.is_empty());

	match preferred_filter_mode {
		StreamFilterMode::ExcludeCurrentProcess => {
			let excluded_windows: Retained<NSArray<SCWindow>> =
				NSArray::from_retained_slice(&excepting_windows.windows);

			if let Some(current_process_application) = find_current_process_application(content) {
				let excluded_applications =
					NSArray::from_retained_slice(&[current_process_application]);

				tracing::trace!(
					op = "live_frame_stream.setup_filter_excluding_current_process",
					monitor_id,
					pid = process::id(),
					excepting_window_count,
					"Configured ScreenCaptureKit to exclude Rsnap windows from the live stream."
				);

				PreparedStreamFilter {
					filter_mode: StreamFilterMode::ExcludeCurrentProcess,
					filter: unsafe {
						SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
							SCContentFilter::alloc(),
							display,
							&excluded_applications,
							&excluded_windows,
						)
					},
					self_capture_filter_complete: true,
					self_capture_exception_window_ids_complete: true,
					excepting_window_count,
					fallback_excluded_window_count,
					missing_window_ids,
					shareable_content_ms: 0,
					find_display_ms: 0,
					exception_windows_ms: 0,
					filter_build_ms: 0,
				}
			} else {
				log_missing_current_process_fallback(
					monitor_id,
					excepting_window_count,
					fallback_excluded_window_count,
				);

				build_shareable_window_filter(
					monitor_id,
					display,
					excepting_windows.fallback_excluded_windows,
					excepting_window_count,
					fallback_excluded_window_count,
					missing_window_ids,
					false,
				)
			}
		},
		StreamFilterMode::ExcludeCurrentProcessShareableWindows => build_shareable_window_filter(
			monitor_id,
			display,
			excepting_windows.fallback_excluded_windows,
			excepting_window_count,
			fallback_excluded_window_count,
			missing_window_ids,
			true,
		),
	}
}

fn build_shareable_window_filter(
	monitor_id: u32,
	display: &SCDisplay,
	fallback_excluded_windows: Vec<Retained<SCWindow>>,
	excepting_window_count: usize,
	fallback_excluded_window_count: usize,
	missing_window_ids: Vec<u32>,
	log_partial_match: bool,
) -> PreparedStreamFilter {
	let excluded_windows: Retained<NSArray<SCWindow>> =
		NSArray::from_retained_slice(&fallback_excluded_windows);

	if log_partial_match {
		tracing::debug!(
			op = "live_frame_stream.setup_filter_fallback_excluding_shareable_windows",
			monitor_id,
			pid = process::id(),
			excepting_window_count,
			fallback_excluded_window_count,
			missing_window_ids = ?missing_window_ids,
			"ScreenCaptureKit omitted at least one requested self-capture exception window; falling back to excluding only Rsnap's currently shareable windows."
		);
	}

	PreparedStreamFilter {
		filter_mode: StreamFilterMode::ExcludeCurrentProcessShareableWindows,
		filter: unsafe {
			SCContentFilter::initWithDisplay_excludingWindows(
				SCContentFilter::alloc(),
				display,
				&excluded_windows,
			)
		},
		self_capture_filter_complete: false,
		self_capture_exception_window_ids_complete: missing_window_ids.is_empty(),
		excepting_window_count,
		fallback_excluded_window_count,
		missing_window_ids,
		shareable_content_ms: 0,
		find_display_ms: 0,
		exception_windows_ms: 0,
		filter_build_ms: 0,
	}
}

fn log_missing_current_process_fallback(
	monitor_id: u32,
	excepting_window_count: usize,
	fallback_excluded_window_count: usize,
) {
	tracing::debug!(
		op = "live_frame_stream.setup_filter_fallback_missing_current_process",
		monitor_id,
		pid = process::id(),
		excepting_window_count,
		fallback_excluded_window_count,
		"ScreenCaptureKit omitted Rsnap's running application during stream setup; falling back to excluding only Rsnap's currently shareable windows."
	);
}

fn load_shareable_content_for_monitor(monitor_id: u32) -> Option<Retained<SCShareableContent>> {
	match get_shareable_content() {
		Ok(content) => Some(content),
		Err(error) => {
			tracing::warn!(
				op = "live_frame_stream.get_shareable_content_failed",
				monitor_id,
				error_code = error.code(),
				error_domain = %error.domain(),
				error_description = %error.localizedDescription(),
				"Failed to load ScreenCaptureKit shareable content during live stream setup."
			);

			None
		},
	}
}

fn find_display_for_monitor(
	content: &SCShareableContent,
	monitor_id: u32,
) -> Option<Retained<SCDisplay>> {
	let Some(display) = find_display(content, monitor_id) else {
		tracing::warn!(
			op = "live_frame_stream.find_display_failed",
			monitor_id,
			"Failed to find the requested monitor in ScreenCaptureKit shareable content."
		);

		return None;
	};

	Some(display)
}

fn find_current_process_exception_windows(
	content: &SCShareableContent,
	self_capture_exception_window_ids: &[u32],
) -> CurrentProcessExceptionWindows {
	if self_capture_exception_window_ids.is_empty() {
		return CurrentProcessExceptionWindows {
			windows: Vec::new(),
			fallback_excluded_windows: Vec::new(),
			missing_window_ids: Vec::new(),
		};
	}

	let current_pid = process::id();
	let windows = unsafe { content.windows() };
	let mut matched = Vec::new();
	let mut fallback_excluded_windows = Vec::new();
	let mut matched_window_ids = Vec::new();

	for window in windows.iter() {
		let window_id = unsafe { window.windowID() };
		let is_requested_exception = self_capture_exception_window_ids.contains(&window_id);

		if is_requested_exception {
			matched_window_ids.push(window_id);
			matched.push(window.retain());
		}
		if window_is_owned_by_current_process(&window, current_pid) && !is_requested_exception {
			fallback_excluded_windows.push(window.retain());
		}
	}

	let missing_window_ids =
		missing_exception_window_ids(self_capture_exception_window_ids, &matched_window_ids);

	if !missing_window_ids.is_empty() {
		tracing::debug!(
			op = "live_frame_stream.self_capture_exception_window_ids_partial_match",
			requested_window_ids = ?self_capture_exception_window_ids,
			missing_window_ids = ?missing_window_ids,
			matched_window_count = matched.len(),
			fallback_excluded_window_count = fallback_excluded_windows.len(),
			"ScreenCaptureKit did not expose every requested current-process exception window; continuing stream setup with a capturable window-exclusion fallback."
		);
	}

	CurrentProcessExceptionWindows {
		windows: matched,
		fallback_excluded_windows,
		missing_window_ids,
	}
}

fn missing_exception_window_ids(
	self_capture_exception_window_ids: &[u32],
	matched_window_ids: &[u32],
) -> Vec<u32> {
	self_capture_exception_window_ids
		.iter()
		.copied()
		.filter(|window_id| !matched_window_ids.contains(window_id))
		.collect()
}

fn find_current_process_application(
	content: &SCShareableContent,
) -> Option<Retained<SCRunningApplication>> {
	let current_pid = process::id();
	let applications = unsafe { content.applications() };

	for application in applications.iter() {
		let Ok(application_pid) = u32::try_from(unsafe { application.processID() }) else {
			continue;
		};

		if application_pid == current_pid {
			return Some(application.retain());
		}
	}

	None
}

fn window_is_owned_by_current_process(window: &SCWindow, current_pid: u32) -> bool {
	unsafe { window.owningApplication() }
		.and_then(|application| u32::try_from(unsafe { application.processID() }).ok())
		.is_some_and(|window_pid| window_pid == current_pid)
}

fn stream_filter_mode_for_current_process(
	self_capture_exception_window_ids_complete: bool,
) -> StreamFilterMode {
	if self_capture_exception_window_ids_complete {
		StreamFilterMode::ExcludeCurrentProcess
	} else {
		StreamFilterMode::ExcludeCurrentProcessShareableWindows
	}
}

fn get_shareable_content() -> Result<Retained<SCShareableContent>, Retained<NSError>> {
	let (tx, rx) = mpsc::sync_channel::<Result<Retained<SCShareableContent>, Retained<NSError>>>(1);
	let tx = Mutex::new(Some(tx));
	let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
		let mut maybe_tx = match tx.lock() {
			Ok(guard) => guard,
			Err(poisoned) => poisoned.into_inner(),
		};
		let Some(tx) = maybe_tx.take() else {
			return;
		};

		if !err.is_null() {
			let Some(err) = (unsafe { Retained::retain(err) }) else {
				let _ = tx.send(Err(super::stream_error(STREAM_ERROR_RETAIN_FAILED_CODE)));

				return;
			};
			let _ = tx.send(Err(err));

			return;
		}

		let Some(content) = (unsafe { Retained::retain(content) }) else {
			let err = super::stream_error(STREAM_ERROR_NULL_CONTENT_CODE);
			let _ = tx.send(Err(err));

			return;
		};
		let _ = tx.send(Ok(content));
	});

	unsafe {
		SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
			false,
			true,
			&block,
		)
	};

	rx.recv_timeout(Duration::from_secs(2))
		.map_err(|_| super::stream_error(STREAM_ERROR_TIMEOUT_CODE))?
}

fn find_display(content: &SCShareableContent, monitor_id: u32) -> Option<Retained<SCDisplay>> {
	let displays = unsafe { content.displays() };

	for display in displays.iter() {
		let display_id = unsafe { display.displayID() };

		if display_id == monitor_id {
			return Some(display);
		}
	}

	None
}

#[cfg(test)]
mod tests {
	use crate::live_frame_stream_macos::stream_filter::{self, StreamFilterMode};

	#[test]
	fn stream_filter_mode_prefers_process_exclusion_only_when_exception_list_is_complete() {
		assert_eq!(
			stream_filter::stream_filter_mode_for_current_process(true),
			StreamFilterMode::ExcludeCurrentProcess
		);
		assert_eq!(
			stream_filter::stream_filter_mode_for_current_process(false),
			StreamFilterMode::ExcludeCurrentProcessShareableWindows
		);
	}

	#[test]
	fn missing_exception_window_ids_reports_unshareable_requested_windows() {
		assert_eq!(stream_filter::missing_exception_window_ids(&[], &[]), Vec::<u32>::new());
		assert_eq!(
			stream_filter::missing_exception_window_ids(&[7, 11], &[7, 11]),
			Vec::<u32>::new()
		);
		assert_eq!(stream_filter::missing_exception_window_ids(&[7, 11], &[11]), vec![7]);
	}
}
