use std::sync::Arc;
use std::time::Instant;

use egui::Pos2;
use winit::event::{ElementState, Ime, MouseButton};
use winit::keyboard::ModifiersState;

use crate::overlay::OverlayKeyboardInputEvent;
use crate::state::{GlobalPoint, MonitorRect, RectPoints};

pub(in crate::overlay) type ExternalScrollInputEvent = (u64, Instant, f64, f64, f64, bool, bool);

pub(in crate::overlay) type ExternalScrollInputDrainReader =
	Arc<dyn Fn(u64, Instant) -> Vec<ExternalScrollInputEvent> + Send + Sync>;

pub(in crate::overlay) type ScrollCaptureHostStart =
	Arc<dyn Fn(ScrollCaptureHostStartRequest) -> color_eyre::eyre::Result<bool> + Send + Sync>;

pub(in crate::overlay) type ScrollCaptureHostStop = Arc<dyn Fn() + Send + Sync>;

pub(in crate::overlay) type ScrollCaptureHostFrameRequest = Arc<
	dyn Fn(
			MonitorRect,
			RectPoints,
			u64,
		) -> std::result::Result<(), ScrollCaptureHostFrameRequestError>
		+ Send
		+ Sync,
>;

#[cfg(test)]
pub(in crate::overlay) type ScrollCaptureStartGuard =
	Arc<dyn Fn() -> color_eyre::eyre::Result<bool> + Send + Sync>;

#[cfg(test)]
pub(in crate::overlay) type ScrollCaptureStartingHook =
	Arc<dyn Fn() -> color_eyre::eyre::Result<()> + Send + Sync>;

#[cfg(test)]
pub(in crate::overlay) type ScrollCaptureStartedHook = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug)]
/// Failure contract for one host-owned scroll-capture frame request.
pub enum ScrollCaptureHostFrameRequestError {
	/// The host capability is temporarily busy; the core should back off and retry.
	Busy,
	/// The host capability is unavailable and the session must fail closed with this message.
	Unavailable(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
/// Scroll-wheel delta routed from the native passive capture host.
pub enum MacOSNativeCaptureScrollDelta {
	/// A line-based scroll delta measured in lines along the x/y axes.
	Line {
		/// Horizontal line delta.
		x: f32,
		/// Vertical line delta.
		y: f32,
	},
	/// A pixel-based scroll delta measured in pixels along the x/y axes.
	Pixel {
		/// Horizontal pixel delta.
		x: f64,
		/// Vertical pixel delta.
		y: f64,
	},
}

#[derive(Clone, Debug, PartialEq)]
/// Input event routed from the app-owned macOS passive capture host into the overlay core.
pub enum MacOSNativeCaptureInputEvent {
	/// Pointer movement over a passive overlay shell.
	OverlayPointerMoved {
		/// Monitor whose passive shell observed the pointer movement.
		monitor: MonitorRect,
		/// Pointer location in global desktop coordinates.
		global: GlobalPoint,
	},
	/// Mouse-button activity observed by a passive overlay shell.
	OverlayMouseInput {
		/// Monitor whose passive shell observed the mouse input.
		monitor: MonitorRect,
		/// Pointer location in global desktop coordinates.
		global: GlobalPoint,
		/// Mouse button that changed state.
		button: MouseButton,
		/// New state for the button.
		state: ElementState,
	},
	/// Pointer movement over a passive toolbar shell.
	ToolbarPointerMoved {
		/// Monitor that currently anchors the toolbar shell.
		monitor: MonitorRect,
		/// Pointer location in toolbar-local coordinates.
		local: Pos2,
		/// Pointer location in global desktop coordinates.
		global: GlobalPoint,
		/// Toolbar shell origin in global desktop coordinates.
		outer_position: GlobalPoint,
	},
	/// Pointer exit from the passive toolbar shell.
	ToolbarPointerLeft,
	/// Mouse-button activity observed by the passive toolbar shell.
	ToolbarMouseInput {
		/// Mouse button that changed state.
		button: MouseButton,
		/// New state for the button.
		state: ElementState,
	},
	/// Scroll-wheel input observed by the passive toolbar shell.
	ToolbarScrollWheel {
		/// Scroll delta reported by the native host.
		delta: MacOSNativeCaptureScrollDelta,
	},
	/// Keyboard input forwarded from the passive key-focus shell.
	KeyboardInput {
		/// Monitor associated with the active key-focus shell, if any.
		monitor: Option<MonitorRect>,
		/// Opaque keyboard event payload translated from winit/native state.
		event: OverlayKeyboardInputEvent,
	},
	/// IME input forwarded from the passive key-focus shell.
	Ime {
		/// Monitor associated with the active key-focus shell, if any.
		monitor: Option<MonitorRect>,
		/// IME payload to apply inside the overlay session.
		event: Ime,
	},
	/// Modifier-key state update forwarded from the native host.
	ModifiersChanged {
		/// Current modifier-key state.
		state: ModifiersState,
	},
}

#[derive(Clone, Copy, Debug)]
/// Host-owned scroll-capture start request emitted by the core.
pub struct ScrollCaptureHostStartRequest {
	/// Target monitor for the scroll-capture session.
	pub monitor: MonitorRect,
	/// Scroll-capture rect in point-space.
	pub capture_rect_points: RectPoints,
	/// Scroll-capture rect in monitor-local pixels.
	pub capture_rect_pixels: RectPoints,
}

#[derive(Clone)]
/// Explicit host/core boundary for scroll-capture capability ownership.
pub struct ScrollCaptureHostAdapter {
	pub(in crate::overlay) start: ScrollCaptureHostStart,
	pub(in crate::overlay) stop: ScrollCaptureHostStop,
	pub(in crate::overlay) request_frame: ScrollCaptureHostFrameRequest,
	pub(in crate::overlay) external_input_drain_reader: ExternalScrollInputDrainReader,
}
impl ScrollCaptureHostAdapter {
	#[must_use]
	/// Creates the host-owned scroll-capture capability adapter for the overlay core.
	pub fn new(
		start: ScrollCaptureHostStart,
		stop: ScrollCaptureHostStop,
		request_frame: ScrollCaptureHostFrameRequest,
		external_input_drain_reader: ExternalScrollInputDrainReader,
	) -> Self {
		Self { start, stop, request_frame, external_input_drain_reader }
	}
}
