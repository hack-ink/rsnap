use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::Key;

use rsnap_capture_core::{OutputNaming, PreparedHostEffectRequest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Selects how the live HUD should be positioned.
pub enum HudAnchor {
	/// Pin the HUD cluster to the current cursor position.
	Cursor,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
/// Chooses the requested HUD and chrome theme.
pub enum ThemeMode {
	#[default]
	/// Follow the host window or operating-system theme.
	System,
	/// Force the dark theme variant.
	Dark,
	/// Force the light theme variant.
	Light,
}

#[derive(Debug)]
/// Describes how an overlay session finished.
pub enum OverlayExit {
	/// The user cancelled the session without producing output.
	Cancelled,
	/// The session completed by handing a host-owned side effect to the caller.
	HostEffect(PreparedHostEffectRequest),
	/// The session failed with a user-visible error message.
	Error(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Host-routed frozen shortcuts that should work without a focused key window.
pub enum FrozenGlobalHotkey {
	/// Copy the current frozen export image.
	Copy,
	/// Recenter the drag-region capture rect around detected content.
	AutoCenter,
	/// Toggle toolbar visibility while frozen.
	ToggleToolbar,
	/// Start scroll capture from an existing frozen selection.
	StartScrollCapture,
	/// Save the current frozen export image.
	Save,
}

#[derive(Debug)]
/// Signals whether the caller should keep driving the overlay event loop.
pub enum OverlayControl {
	/// Keep the session alive and continue processing events.
	Continue,
	/// Execute the requested host-owned side effect before deciding whether to exit.
	HostEffect(PreparedHostEffectRequest),
	/// Exit the session with the provided terminal outcome.
	Exit(OverlayExit),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
/// Chooses where the frozen toolbar is anchored relative to the capture.
pub enum ToolbarPlacement {
	/// Render the toolbar above the frozen capture.
	Top,
	#[default]
	/// Render the toolbar below the frozen capture.
	Bottom,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
/// Controls how transparent window captures are composited before export.
pub enum WindowCaptureAlphaMode {
	#[default]
	/// Preserve the observed screen background behind transparent pixels.
	Background,
	/// Composite transparency against a light matte color.
	MatteLight,
	/// Composite transparency against a dark matte color.
	MatteDark,
}

#[derive(Clone, Debug)]
/// Runtime configuration applied to a capture overlay session.
pub struct OverlayConfig {
	/// Positions the live HUD relative to the cursor or another anchor point.
	pub hud_anchor: HudAnchor,
	/// Shows the Tab-key hint chip in the live HUD when enabled.
	pub show_alt_hint_keycap: bool,
	/// Enables blur or its platform fallback for HUD windows.
	pub show_hud_blur: bool,
	/// Enables the animated flow ring drawn around live auto-detected windows.
	pub selection_flow_enabled: bool,
	/// Sets the core stroke width used for the animated selection border.
	pub selection_flow_stroke_width_px: f32,
	/// Forces an opaque HUD background instead of glass styling.
	pub hud_opaque: bool,
	/// 0..=1. Controls HUD background alpha.
	pub hud_opacity: f32,
	/// 0..=1. 0 disables the effect.
	pub hud_fog_amount: f32,
	/// 0..=1. 0 disables the effect.
	pub hud_milk_amount: f32,
	/// Hue value for tint, 0..=1.
	pub hud_tint_hue: f32,
	/// Chooses where the frozen toolbar is placed.
	pub toolbar_placement: ToolbarPlacement,
	/// Sets the loupe sample size in source pixels.
	pub loupe_sample_side_px: u32,
	/// Requests the light, dark, or system theme.
	pub theme_mode: ThemeMode,
	/// Chooses the destination directory for saved captures.
	pub output_dir: PathBuf,
	/// Sets the filename prefix used for saved captures.
	pub output_filename_prefix: String,
	/// Selects the disk naming strategy for saved captures.
	pub output_naming: OutputNaming,
	/// Selects how transparent window captures are flattened.
	pub window_capture_alpha_mode: WindowCaptureAlphaMode,
	/// Current-process windows that should remain capturable while the rest of Rsnap stays excluded.
	pub self_capture_exception_window_ids: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
/// Opaque keyboard event payload forwarded from the native passive capture host.
pub struct OverlayKeyboardInputEvent {
	pub(in crate::overlay) logical_key: Key,
	pub(in crate::overlay) text: Option<String>,
	pub(in crate::overlay) state: ElementState,
	pub(in crate::overlay) repeat: bool,
}
impl OverlayKeyboardInputEvent {
	pub(in crate::overlay) fn from_winit(event: &KeyEvent) -> Self {
		Self {
			logical_key: event.logical_key.clone(),
			text: event.text.as_deref().map(ToOwned::to_owned),
			state: event.state,
			repeat: event.repeat,
		}
	}
}
