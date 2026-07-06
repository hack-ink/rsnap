use std::ptr;

use crate::abi::{
	RsnapOwnedRgbaRegion, RsnapScrollObserveOutcomeKind, RsnapScrollObserveResult,
	RsnapScrollSessionHandle, RsnapStatus,
};
use rsnap_capture_core::{ScrollStitchImage, ScrollStitchObserveOutcome, ScrollStitchSession};

/// Creates a scroll-capture stitcher from the first frozen viewport frame.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data. The returned pointer must be released by calling
/// `rsnap_scroll_session_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_create(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	preview_width_px: u32,
) -> *mut RsnapScrollSessionHandle {
	let Some(bytes) = (unsafe { crate::rgba_bytes(rgba, rgba_len) }) else {
		return ptr::null_mut();
	};
	let Ok(session) = ScrollStitchSession::new_from_rgba(width, height, bytes, preview_width_px)
	else {
		return ptr::null_mut();
	};

	Box::into_raw(Box::new(RsnapScrollSessionHandle { session }))
}

/// Destroys a scroll-capture stitcher returned by `rsnap_scroll_session_create`.
///
/// # Safety
///
/// The pointer must either be null or a pointer returned by
/// `rsnap_scroll_session_create` that has not already been destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_destroy(handle: *mut RsnapScrollSessionHandle) {
	if handle.is_null() {
		return;
	}

	unsafe {
		drop(Box::from_raw(handle));
	}
}

/// Observes one discrete viewport screenshot for downward scroll-capture stitching.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_scroll_session_create`.
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_result` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_observe_downward_frame(
	handle: *mut RsnapScrollSessionHandle,
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	out_result: *mut RsnapScrollObserveResult,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle.as_mut() }) else {
		return RsnapStatus::NullHandle;
	};

	if out_result.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { crate::rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let outcome = match handle.session.observe_worker_pairwise_rgba(width, height, bytes) {
		Ok(outcome) => outcome,
		Err(_err) => return RsnapStatus::InvalidInput,
	};
	let (export_width, export_height) = handle.session.export_dimensions();

	unsafe {
		ptr::write(
			out_result,
			encode_scroll_observe_result(outcome, export_width, export_height, &handle.session),
		);
	}

	RsnapStatus::Ok
}

/// Observes one discrete viewport screenshot with an optional downward motion hint.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_scroll_session_create`.
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data, and `out_result` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_observe_downward_frame_with_motion_hint(
	handle: *mut RsnapScrollSessionHandle,
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	motion_rows_hint: u32,
	allow_burst_search: u8,
	out_result: *mut RsnapScrollObserveResult,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle.as_mut() }) else {
		return RsnapStatus::NullHandle;
	};

	if out_result.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { crate::rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let hint = (motion_rows_hint > 0).then_some(motion_rows_hint);
	let outcome = match handle.session.observe_downward_rgba_with_motion_hint(
		width,
		height,
		bytes,
		hint,
		allow_burst_search != 0,
	) {
		Ok(outcome) => outcome,
		Err(_err) => return RsnapStatus::InvalidInput,
	};
	let (export_width, export_height) = handle.session.export_dimensions();

	unsafe {
		ptr::write(
			out_result,
			encode_scroll_observe_result(outcome, export_width, export_height, &handle.session),
		);
	}

	RsnapStatus::Ok
}

/// Copies the current committed scroll-capture export into a Rust-owned RGBA buffer.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_scroll_session_create`, and
/// `out_region` must be writable. The returned buffer must be released with
/// `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_take_export_rgba(
	handle: *mut RsnapScrollSessionHandle,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle.as_mut() }) else {
		return RsnapStatus::NullHandle;
	};

	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let export = handle.session.export_image();

	unsafe {
		ptr::write(out_region, owned_region_from_scroll_image(export));
	}

	RsnapStatus::Ok
}

/// Copies the current committed scroll-capture preview into a Rust-owned RGBA buffer.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_scroll_session_create`, and
/// `out_region` must be writable. The returned buffer must be released with
/// `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_take_preview_rgba(
	handle: *mut RsnapScrollSessionHandle,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle.as_mut() }) else {
		return RsnapStatus::NullHandle;
	};

	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let preview = handle.session.preview_image();

	unsafe {
		ptr::write(out_region, owned_region_from_scroll_image(preview));
	}

	RsnapStatus::Ok
}

/// Reverts the most recent committed scroll-capture append when possible.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_scroll_session_create`, and
/// `out_result` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_scroll_session_undo_last_append(
	handle: *mut RsnapScrollSessionHandle,
	out_result: *mut RsnapScrollObserveResult,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle.as_mut() }) else {
		return RsnapStatus::NullHandle;
	};

	if out_result.is_null() {
		return RsnapStatus::NullOutput;
	}

	let did_undo = handle.session.undo_last_append();
	let (export_width, export_height) = handle.session.export_dimensions();
	let kind = if did_undo {
		ScrollStitchObserveOutcome::PreviewUpdated
	} else {
		ScrollStitchObserveOutcome::NoChange
	};

	unsafe {
		ptr::write(
			out_result,
			encode_scroll_observe_result(kind, export_width, export_height, &handle.session),
		);
	}

	RsnapStatus::Ok
}

fn encode_scroll_observe_result(
	outcome: ScrollStitchObserveOutcome,
	export_width: u32,
	export_height: u32,
	session: &ScrollStitchSession,
) -> RsnapScrollObserveResult {
	let (kind, growth_rows) = match outcome {
		ScrollStitchObserveOutcome::NoChange => (RsnapScrollObserveOutcomeKind::NoChange, 0),
		ScrollStitchObserveOutcome::PreviewUpdated => {
			(RsnapScrollObserveOutcomeKind::PreviewUpdated, 0)
		},
		ScrollStitchObserveOutcome::Committed { growth_rows } => {
			(RsnapScrollObserveOutcomeKind::Committed, growth_rows)
		},
		ScrollStitchObserveOutcome::UnsupportedDirection => {
			(RsnapScrollObserveOutcomeKind::UnsupportedDirection, 0)
		},
	};

	RsnapScrollObserveResult {
		kind: kind as u32,
		growth_rows,
		export_width,
		export_height,
		current_viewport_top_y: session.current_viewport_top_y(),
	}
}

fn owned_region_from_scroll_image(image: ScrollStitchImage) -> RsnapOwnedRgbaRegion {
	crate::owned_region_from_raw_rgba(image.width, image.height, image.rgba)
}
