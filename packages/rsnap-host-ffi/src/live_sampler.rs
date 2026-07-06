use std::mem;
use std::ptr::{self, NonNull};
use std::slice;

use crate::abi::{
	RsnapLiveSamplerHandle, RsnapMonitorRect, RsnapOwnedRgbaRegion, RsnapPixelRect, RsnapPoint,
	RsnapRect, RsnapStatus,
};
use rsnap_capture_core::{GlobalPoint, MonitorRect};
use rsnap_overlay::host_live_sampling_macos::HostMacLiveSampler;

/// Creates a new opaque live-sampler handle for the native host.
///
/// # Safety
///
/// The returned pointer must be released by calling `rsnap_live_sampler_destroy`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_create() -> *mut RsnapLiveSamplerHandle {
	Box::into_raw(Box::new(RsnapLiveSamplerHandle { sampler: HostMacLiveSampler::new() }))
}

/// Creates a live sampler that keeps selected current-process windows capturable.
///
/// # Safety
///
/// `window_ids` must point to `window_id_count` valid `u32` values, or be null when
/// `window_id_count` is zero. The returned pointer must be released by calling
/// `rsnap_live_sampler_destroy`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_create_with_self_capture_exception_window_ids(
	window_ids: *const u32,
	window_id_count: usize,
) -> *mut RsnapLiveSamplerHandle {
	if window_id_count > 0 && window_ids.is_null() {
		return ptr::null_mut();
	}

	let exception_window_ids = if window_id_count == 0 {
		Vec::new()
	} else {
		unsafe { slice::from_raw_parts(window_ids, window_id_count) }.to_vec()
	};

	Box::into_raw(Box::new(RsnapLiveSamplerHandle {
		sampler: HostMacLiveSampler::with_self_capture_exception_window_ids(exception_window_ids),
	}))
}

/// Starts warming the live sampler for the requested monitor without blocking on the
/// first captured frame.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_prime_monitor(
	handle: *mut RsnapLiveSamplerHandle,
	monitor: RsnapMonitorRect,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	handle.sampler.prime_monitor(decode_overlay_monitor(monitor));

	RsnapStatus::Ok
}

/// Stops any active ScreenCaptureKit stream while retaining the live-sampler worker.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_reset(
	handle: *mut RsnapLiveSamplerHandle,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	handle.sampler.reset();

	RsnapStatus::Ok
}

/// Destroys an opaque live-sampler handle.
///
/// # Safety
///
/// The pointer must either be null or a pointer returned by `rsnap_live_sampler_create`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_destroy(handle: *mut RsnapLiveSamplerHandle) {
	if let Some(handle) = NonNull::new(handle) {
		unsafe {
			drop(Box::from_raw(handle.as_ptr()));
		}
	}
}

/// Transfers ownership of a cached RGBA region from the latest live sampler monitor frame
/// to the caller.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`, and
/// `out_region` must be a valid writable pointer. The caller must later release the
/// returned buffer with `rsnap_owned_rgba_region_release`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_take_region_rgba(
	handle: *mut RsnapLiveSamplerHandle,
	monitor: RsnapMonitorRect,
	rect: RsnapRect,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let region = handle.sampler.peek_region_rgba(
		decode_overlay_monitor(monitor),
		decode_overlay_point(RsnapPoint { x: rect.x, y: rect.y }),
		rect.width,
		rect.height,
	);
	let Some(region) = region else {
		unsafe {
			ptr::write(out_region, RsnapOwnedRgbaRegion::default());
		}

		return RsnapStatus::Empty;
	};
	let mut rgba = region.rgba;
	let out = RsnapOwnedRgbaRegion {
		width: region.width,
		height: region.height,
		len: rgba.len(),
		capacity: rgba.capacity(),
		rgba: rgba.as_mut_ptr(),
	};

	mem::forget(rgba);

	unsafe {
		ptr::write(out_region, out);
	}

	RsnapStatus::Ok
}

/// Transfers ownership of the oldest queued RGBA region newer than `after_frame_seq`
/// to the caller, preserving live-stream frame order.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`,
/// `out_frame_seq` and `out_frame_age_micros` must be valid writable pointers, and
/// `out_region` must be a valid writable pointer. The caller must later release the
/// returned region buffer with `rsnap_owned_rgba_region_release`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_take_next_region_rgba_after_seq(
	handle: *mut RsnapLiveSamplerHandle,
	monitor: RsnapMonitorRect,
	rect: RsnapRect,
	after_frame_seq: u64,
	wait_for_fresh: u8,
	out_frame_seq: *mut u64,
	out_frame_age_micros: *mut u64,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_frame_seq.is_null() || out_frame_age_micros.is_null() || out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(frame) = handle.sampler.next_region_rgba_after_seq(
		decode_overlay_monitor(monitor),
		decode_overlay_point(RsnapPoint { x: rect.x, y: rect.y }),
		rect.width,
		rect.height,
		after_frame_seq,
		wait_for_fresh != 0,
	) else {
		unsafe {
			ptr::write(out_frame_seq, after_frame_seq);
			ptr::write(out_frame_age_micros, 0);
			ptr::write(out_region, RsnapOwnedRgbaRegion::default());
		}

		return RsnapStatus::Empty;
	};
	let mut rgba = frame.region.rgba;
	let out = RsnapOwnedRgbaRegion {
		width: frame.region.width,
		height: frame.region.height,
		len: rgba.len(),
		capacity: rgba.capacity(),
		rgba: rgba.as_mut_ptr(),
	};

	mem::forget(rgba);

	unsafe {
		ptr::write(out_frame_seq, frame.frame_seq);
		ptr::write(out_frame_age_micros, frame.frame_age_micros);
		ptr::write(out_region, out);
	}

	RsnapStatus::Ok
}

/// Transfers ownership of the oldest queued RGBA region newer than `after_frame_seq`
/// using a monitor-local pixel rectangle, preserving live-stream frame order.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `rsnap_live_sampler_create`,
/// `out_frame_seq` and `out_frame_age_micros` must be valid writable pointers, and
/// `out_region` must be a valid writable pointer. The caller must later release the
/// returned region buffer with `rsnap_owned_rgba_region_release`.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_live_sampler_take_next_region_rgba_pixels_after_seq(
	handle: *mut RsnapLiveSamplerHandle,
	monitor: RsnapMonitorRect,
	rect: RsnapPixelRect,
	after_frame_seq: u64,
	wait_for_fresh: u8,
	out_frame_seq: *mut u64,
	out_frame_age_micros: *mut u64,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	let Some(handle) = (unsafe { handle_mut(handle) }) else {
		return RsnapStatus::NullHandle;
	};

	if out_frame_seq.is_null() || out_frame_age_micros.is_null() || out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(frame) = handle.sampler.next_region_rgba_after_seq_pixels(
		decode_overlay_monitor(monitor),
		crate::decode_pixel_rect(rect),
		after_frame_seq,
		wait_for_fresh != 0,
	) else {
		unsafe {
			ptr::write(out_frame_seq, after_frame_seq);
			ptr::write(out_frame_age_micros, 0);
			ptr::write(out_region, RsnapOwnedRgbaRegion::default());
		}

		return RsnapStatus::Empty;
	};
	let mut rgba = frame.region.rgba;
	let out = RsnapOwnedRgbaRegion {
		width: frame.region.width,
		height: frame.region.height,
		len: rgba.len(),
		capacity: rgba.capacity(),
		rgba: rgba.as_mut_ptr(),
	};

	mem::forget(rgba);

	unsafe {
		ptr::write(out_frame_seq, frame.frame_seq);
		ptr::write(out_frame_age_micros, frame.frame_age_micros);
		ptr::write(out_region, out);
	}

	RsnapStatus::Ok
}

unsafe fn handle_mut<'a>(
	handle: *mut RsnapLiveSamplerHandle,
) -> Option<&'a mut RsnapLiveSamplerHandle> {
	unsafe { handle.as_mut() }
}

fn decode_overlay_point(point: RsnapPoint) -> GlobalPoint {
	GlobalPoint::new(point.x, point.y)
}

fn decode_overlay_monitor(monitor: RsnapMonitorRect) -> MonitorRect {
	MonitorRect {
		id: monitor.id,
		origin: GlobalPoint::new(monitor.origin.x, monitor.origin.y),
		width: monitor.width,
		height: monitor.height,
		scale_factor_x1000: monitor.scale_factor_x1000,
	}
}
