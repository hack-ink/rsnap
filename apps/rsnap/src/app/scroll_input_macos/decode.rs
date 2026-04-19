use crate::app::scroll_input_macos::{
	CGEventGetDoubleValueField, CGEventGetIntegerValueField, CGEventGetLocation, CGEventRef,
};

const KCG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1_FIELD: u32 = 11;
const KCG_SCROLL_WHEEL_EVENT_IS_CONTINUOUS_FIELD: u32 = 88;
const KCG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1_FIELD: u32 = 96;
const KCG_SCROLL_WHEEL_EVENT_SCROLL_PHASE_FIELD: u32 = 99;
const KCG_SCROLL_WHEEL_EVENT_MOMENTUM_PHASE_FIELD: u32 = 123;
const MACOS_SCROLL_PIXEL_WRAP_MODULUS: f64 = 4_294_967_296.0;
const MACOS_SCROLL_PIXEL_WRAP_THRESHOLD: f64 = 1_000_000.0;
const MACOS_SCROLL_PIXEL_DELTA_CLAMP: f64 = 240.0;
const NSEVENT_PHASE_BEGAN: u64 = 0x1 << 0;
const NSEVENT_PHASE_STATIONARY: u64 = 0x1 << 1;
const NSEVENT_PHASE_CHANGED: u64 = 0x1 << 2;
const NSEVENT_PHASE_ENDED: u64 = 0x1 << 3;
const NSEVENT_PHASE_CANCELLED: u64 = 0x1 << 4;
const NSEVENT_PHASE_MAY_BEGIN: u64 = 0x1 << 5;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct MacOSCGPoint {
	pub(super) x: f64,
	pub(super) y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DecodedScrollInput {
	pub(super) raw_delta_y: f64,
	pub(super) delta_y: f64,
	pub(super) global_x: f64,
	pub(super) global_y: f64,
	pub(super) gesture_active: bool,
	pub(super) gesture_ended: bool,
}

pub(super) fn decode_scroll_input_from_cg_event(
	cg_event: CGEventRef,
) -> Option<DecodedScrollInput> {
	let location = unsafe { CGEventGetLocation(cg_event) };
	let (raw_delta_y, is_continuous) = scroll_delta_y_from_cg_event(cg_event);
	let delta_y = if is_continuous {
		normalize_macos_scroll_pixel_component(raw_delta_y)
	} else {
		raw_delta_y
	};
	let scroll_phase = scroll_phase_bits_from_cg_event(cg_event);
	let momentum_phase = scroll_momentum_phase_bits_from_cg_event(cg_event);
	let gesture_active =
		scroll_phase_bits_are_active(scroll_phase) || scroll_phase_bits_are_active(momentum_phase);
	let gesture_ended = scroll_phase_bits_are_terminal(scroll_phase)
		|| scroll_phase_bits_are_terminal(momentum_phase);

	tracing::debug!(
		op = "scroll_input.tap_decoded",
		raw_delta_y,
		global_x = location.x,
		global_y = location.y,
		scroll_phase,
		momentum_phase,
		gesture_active,
		gesture_ended,
		"Decoded native macOS scroll input event."
	);

	decode_scroll_input_from_parts(raw_delta_y, delta_y, location, gesture_active, gesture_ended)
}

pub(super) fn normalize_macos_scroll_pixel_component(value: f64) -> f64 {
	if !value.is_finite() {
		return 0.0;
	}

	let normalized = if value.abs() > MACOS_SCROLL_PIXEL_WRAP_THRESHOLD {
		if value.is_sign_positive() {
			value - MACOS_SCROLL_PIXEL_WRAP_MODULUS
		} else {
			value + MACOS_SCROLL_PIXEL_WRAP_MODULUS
		}
	} else {
		value
	};

	normalized.clamp(-MACOS_SCROLL_PIXEL_DELTA_CLAMP, MACOS_SCROLL_PIXEL_DELTA_CLAMP)
}

pub(super) fn scroll_phase_bits_are_active(phase_bits: u64) -> bool {
	phase_bits
		& (NSEVENT_PHASE_BEGAN
			| NSEVENT_PHASE_STATIONARY
			| NSEVENT_PHASE_CHANGED
			| NSEVENT_PHASE_MAY_BEGIN)
		!= 0
}

pub(super) fn scroll_phase_bits_are_terminal(phase_bits: u64) -> bool {
	phase_bits & (NSEVENT_PHASE_ENDED | NSEVENT_PHASE_CANCELLED) != 0
}

#[cfg(test)]
fn decode_scroll_input_from_fields(
	raw_delta_y: f64,
	location: MacOSCGPoint,
	gesture_active: bool,
	gesture_ended: bool,
) -> Option<DecodedScrollInput> {
	decode_scroll_input_from_parts(
		raw_delta_y,
		raw_delta_y,
		location,
		gesture_active,
		gesture_ended,
	)
}

fn decode_scroll_input_from_parts(
	raw_delta_y: f64,
	delta_y: f64,
	location: MacOSCGPoint,
	gesture_active: bool,
	gesture_ended: bool,
) -> Option<DecodedScrollInput> {
	if raw_delta_y == 0.0 && !gesture_ended {
		return None;
	}

	Some(DecodedScrollInput {
		raw_delta_y,
		delta_y,
		global_x: location.x,
		global_y: location.y,
		gesture_active,
		gesture_ended,
	})
}

fn scroll_delta_y_from_cg_event(cg_event: CGEventRef) -> (f64, bool) {
	let is_continuous = unsafe {
		CGEventGetIntegerValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_IS_CONTINUOUS_FIELD)
	} != 0;

	if is_continuous {
		(
			unsafe {
				CGEventGetDoubleValueField(
					cg_event,
					KCG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1_FIELD,
				)
			},
			true,
		)
	} else {
		(
			unsafe {
				CGEventGetDoubleValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1_FIELD)
			},
			false,
		)
	}
}

fn scroll_phase_bits_from_cg_event(cg_event: CGEventRef) -> u64 {
	unsafe {
		CGEventGetIntegerValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_SCROLL_PHASE_FIELD) as u64
	}
}

fn scroll_momentum_phase_bits_from_cg_event(cg_event: CGEventRef) -> u64 {
	unsafe {
		CGEventGetIntegerValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_MOMENTUM_PHASE_FIELD) as u64
	}
}

#[cfg(test)]
mod tests {
	use crate::app::scroll_input_macos::decode::{
		self, DecodedScrollInput, MacOSCGPoint, NSEVENT_PHASE_BEGAN, NSEVENT_PHASE_CANCELLED,
		NSEVENT_PHASE_ENDED, NSEVENT_PHASE_MAY_BEGIN,
	};

	#[test]
	fn decode_scroll_input_ignores_zero_non_terminal_delta() {
		assert_eq!(
			decode::decode_scroll_input_from_fields(
				0.0,
				MacOSCGPoint { x: 10.0, y: 20.0 },
				false,
				false
			),
			None
		);
	}

	#[test]
	fn decode_scroll_input_preserves_terminal_zero_delta() {
		assert_eq!(
			decode::decode_scroll_input_from_fields(
				0.0,
				MacOSCGPoint { x: 10.0, y: 20.0 },
				false,
				true
			),
			Some(DecodedScrollInput {
				raw_delta_y: 0.0,
				delta_y: 0.0,
				global_x: 10.0,
				global_y: 20.0,
				gesture_active: false,
				gesture_ended: true,
			})
		);
	}

	#[test]
	fn phase_bits_classify_active_and_terminal_states() {
		assert!(decode::scroll_phase_bits_are_active(NSEVENT_PHASE_BEGAN));
		assert!(decode::scroll_phase_bits_are_active(NSEVENT_PHASE_MAY_BEGIN));
		assert!(decode::scroll_phase_bits_are_terminal(NSEVENT_PHASE_ENDED));
		assert!(decode::scroll_phase_bits_are_terminal(NSEVENT_PHASE_CANCELLED));
		assert!(!decode::scroll_phase_bits_are_terminal(NSEVENT_PHASE_BEGAN));
	}

	#[test]
	fn normalize_scroll_input_wraps_large_pixel_deltas_back_into_signed_range() {
		assert_eq!(decode::normalize_macos_scroll_pixel_component(4_294_967_294.0), -2.0);
		assert_eq!(decode::normalize_macos_scroll_pixel_component(4_294_967_290.0), -6.0);
	}

	#[test]
	fn normalize_scroll_input_clamps_continuous_pixel_delta_magnitude() {
		assert_eq!(decode::normalize_macos_scroll_pixel_component(512.0), 240.0);
		assert_eq!(decode::normalize_macos_scroll_pixel_component(-512.0), -240.0);
	}
}
