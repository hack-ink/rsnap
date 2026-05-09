//! Platform-neutral host/core protocol types.

use std::path::PathBuf;

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::RectPoints;
use crate::export;
use crate::geometry::{GlobalPoint, GlobalRect, MonitorRect, Rgb, WindowRect};

/// Supported platform families for the host/core boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub enum PlatformTag {
	/// Native macOS host.
	#[default]
	MacOS,
	/// Future native Windows host.
	Windows,
	/// Future native Linux host.
	Linux,
	/// Placeholder for unsupported or test-only hosts.
	Unsupported,
}

/// Product-visible capture mode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub enum CaptureMode {
	/// Capture UI is hidden.
	#[default]
	Hidden,
	/// Live targeting is active.
	Live,
	/// Frozen editing / action mode is active.
	Frozen,
}

/// Semantic cursor intent emitted by the product core.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub enum CursorIntent {
	/// Platform default cursor.
	#[default]
	Default,
	/// Crosshair targeting cursor.
	Crosshair,
	/// Open-hand affordance for a movable frozen selection.
	Grab,
	/// Closed-hand affordance while dragging a frozen selection.
	Grabbing,
	/// Resize north edge.
	ResizeNorth,
	/// Resize south edge.
	ResizeSouth,
	/// Resize east edge.
	ResizeEast,
	/// Resize west edge.
	ResizeWest,
	/// Resize north-east corner.
	ResizeNorthEast,
	/// Resize north-west corner.
	ResizeNorthWest,
	/// Resize south-east corner.
	ResizeSouthEast,
	/// Resize south-west corner.
	ResizeSouthWest,
	/// Text editing cursor.
	Text,
}

/// Host-owned permission surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum PermissionKind {
	/// Screen recording or equivalent display-capture access.
	ScreenRecording,
}

/// Host-owned effect surface that remains outside the Rust core.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum HostEffectKind {
	/// Copy the committed capture to the clipboard.
	CopyCapture,
	/// Save the committed capture to disk.
	SaveCapture,
	/// Run deferred text recognition.
	RecognizeText,
}

/// Selects how saved captures are named on disk.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputNaming {
	/// Use the current Unix timestamp in milliseconds.
	#[default]
	Timestamp,
	/// Use a zero-padded incrementing sequence number.
	Sequence,
}

/// One semantic toolbar action surfaced by the product core.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum ToolbarItemKind {
	/// Pointer/move tool.
	Pointer,
	/// Pen annotation tool.
	Pen,
	/// Arrow annotation tool.
	Arrow,
	/// Text annotation tool.
	Text,
	/// Mosaic annotation tool.
	Mosaic,
	/// Spotlight annotation tool.
	Spotlight,
	/// Undo command.
	Undo,
	/// Redo command.
	Redo,
	/// Auto-center command.
	AutoCenter,
	/// Scroll-capture command.
	Scroll,
	/// OCR command.
	Ocr,
	/// Copy command.
	Copy,
	/// Save command.
	Save,
}
impl ToolbarItemKind {
	/// Whether this item is a sticky mode tool rather than an instant action.
	#[must_use]
	pub const fn is_mode_tool(self) -> bool {
		matches!(
			self,
			Self::Pointer | Self::Pen | Self::Arrow | Self::Text | Self::Mosaic | Self::Spotlight
		)
	}
}

/// Host-to-core event.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum HostEvent {
	/// Capture UI became active and should enter live mode.
	SessionActivated,
	/// Pointer position and optional RGB sample changed.
	PointerMoved {
		/// Global pointer location.
		point: GlobalPoint,
		/// Optional sampled color at the pointer location.
		rgb: Option<Rgb>,
		/// Current active monitor when the host knows it.
		active_monitor: Option<MonitorRect>,
		/// Current highlighted live window when the host knows it.
		highlighted_window: Option<WindowRect>,
	},
	/// Primary interaction began while targeting in live mode.
	PrimaryInteractionStarted {
		/// Global pointer location.
		point: GlobalPoint,
		/// Current active monitor when the host knows it.
		active_monitor: Option<MonitorRect>,
		/// Current highlighted live window when the host knows it.
		highlighted_window: Option<WindowRect>,
	},
	/// Primary interaction updated while targeting in live mode.
	PrimaryInteractionUpdated {
		/// Global pointer location.
		point: GlobalPoint,
		/// Current active monitor when the host knows it.
		active_monitor: Option<MonitorRect>,
		/// Current highlighted live window when the host knows it.
		highlighted_window: Option<WindowRect>,
	},
	/// Primary interaction completed while targeting in live mode.
	PrimaryInteractionCompleted {
		/// Global pointer location.
		point: GlobalPoint,
		/// Current active monitor when the host knows it.
		active_monitor: Option<MonitorRect>,
		/// Current highlighted live window when the host knows it.
		highlighted_window: Option<WindowRect>,
	},
	/// User requested session cancellation.
	CancelRequested,
	/// User requested copy.
	CopyRequested,
	/// User requested save.
	SaveRequested,
	/// User requested text recognition.
	RecognizeTextRequested,
	/// User toggled the live loupe.
	ToggleLoupe,
	/// User invoked a frozen toolbar item.
	ToolbarItemInvoked {
		/// Invoked toolbar item kind.
		item: ToolbarItemKind,
	},
}

/// Core-to-host command or capability request.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum HostRequest {
	/// Start native live capture.
	StartLiveCapture,
	/// Stop native live capture.
	StopLiveCapture,
	/// Request a frozen snapshot handoff from the native host for the provided selection.
	RequestFreezeSnapshot {
		/// Selection rectangle that should become the first visible frozen frame.
		selection: GlobalRect,
		/// Whether the frozen selection may be moved or resized after commit.
		selection_editable: bool,
	},
	/// Start a native scroll-capture session for the current dragged-region freeze.
	StartScrollCapture,
	/// Perform a host-owned effect.
	PerformHostEffect(HostEffectKind),
	/// Request a host-owned permission flow.
	RequestPermission(PermissionKind),
}

/// Host-to-core report for completed capability or effect work.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum HostReport {
	/// A frozen snapshot was committed for the requested selection.
	FreezeSnapshotCommitted {
		/// Frozen selection rectangle in global coordinates.
		selection: GlobalRect,
	},
	/// A host-owned effect completed.
	HostEffectCompleted {
		/// Effect that completed.
		effect: HostEffectKind,
	},
	/// A permission state changed.
	PermissionChanged {
		/// Permission kind that changed.
		kind: PermissionKind,
		/// Whether the permission is now granted.
		granted: bool,
	},
	/// A host-owned status message should be surfaced in the scene.
	StatusMessage {
		/// Human-readable status line.
		message: String,
	},
}

/// Final background OCR outcome reported for telemetry and host publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferredTextRecognitionOutcomeKind {
	/// OCR produced non-empty text that the native host may publish.
	TextReady,
	/// OCR completed successfully but did not return any non-whitespace text.
	NoText,
	/// OCR finished, but a newer capture superseded this request before publish.
	StaleRequestSuppressed,
	/// OCR could not prepare the export image or the host OCR engine failed.
	RecognizeError,
}

/// A fully prepared host-owned effect request emitted by the product core.
#[derive(Debug, Eq, PartialEq)]
pub enum PreparedHostEffectRequest {
	/// Copy the encoded PNG for the completed capture to the host clipboard.
	CopyPng {
		/// Immutable encoded PNG payload prepared from the authoritative export image.
		png_bytes: Vec<u8>,
	},
	/// Save the encoded PNG for the completed capture through the host-owned output path.
	SavePng {
		/// Immutable encoded PNG payload prepared from the authoritative export image.
		png_bytes: Vec<u8>,
		/// Output directory snapshot captured when the save request was issued.
		output_dir: PathBuf,
		/// Filename prefix snapshot captured when the save request was issued.
		output_filename_prefix: String,
		/// Naming policy snapshot captured when the save request was issued.
		output_naming: OutputNaming,
	},
	/// Run deferred OCR for the completed capture through the native host.
	#[cfg(target_os = "macos")]
	DeferredTextRecognition(DeferredTextRecognitionRequest),
}

#[derive(Debug, Eq, PartialEq)]
enum DeferredTextRecognitionImageSource {
	Prepared { image: RgbaImage },
	FrozenCrop { export_image: RgbaImage, crop_rect: Option<RectPoints> },
}
impl DeferredTextRecognitionImageSource {
	fn image_dimensions(&self) -> (u32, u32) {
		match self {
			Self::Prepared { image } => image.dimensions(),
			Self::FrozenCrop { export_image, crop_rect } => crop_rect
				.map(|crop_rect| (crop_rect.width, crop_rect.height))
				.unwrap_or_else(|| export_image.dimensions()),
		}
	}

	fn export_image(&self) -> Option<RgbaImage> {
		match self {
			Self::Prepared { image } => Some(image.clone()),
			Self::FrozenCrop { export_image, crop_rect } => {
				export::crop_export_image(export_image, *crop_rect)
			},
		}
	}

	fn into_export_image(self) -> Option<RgbaImage> {
		match self {
			Self::Prepared { image } => Some(image),
			Self::FrozenCrop { export_image, crop_rect } => {
				export::crop_export_image(&export_image, crop_rect)
			},
		}
	}
}

/// Configuration values the native host provides when creating a capture session.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SessionConfig {
	/// Platform family that owns the session host.
	pub platform: PlatformTag,
	/// Whether the host supports native text entry on this session path.
	pub allow_text_input: bool,
	/// Whether the host prefers the toolbar above the frozen selection.
	pub prefers_toolbar_above_selection: bool,
}
impl Default for SessionConfig {
	fn default() -> Self {
		Self {
			platform: PlatformTag::MacOS,
			allow_text_input: true,
			prefers_toolbar_above_selection: false,
		}
	}
}

/// Lightweight HUD model emitted by the product core.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct HudModel {
	/// Current pointer location when known.
	pub pointer: Option<GlobalPoint>,
	/// Current sampled color when known.
	pub rgb: Option<Rgb>,
	/// Whether the loupe should be visible.
	pub loupe_visible: bool,
}

/// One semantic toolbar item surfaced by the product core.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ToolbarItemModel {
	/// Stable semantic kind.
	pub kind: ToolbarItemKind,
	/// Whether the action is currently allowed.
	pub enabled: bool,
	/// Whether the item is currently selected.
	pub selected: bool,
}

/// Current semantic scene snapshot emitted by the product core.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct SceneModel {
	/// Current capture mode.
	pub mode: CaptureMode,
	/// Semantic cursor intent for the current interaction state.
	pub cursor_intent: CursorIntent,
	/// Current pointer location when known.
	pub pointer: Option<GlobalPoint>,
	/// Current active monitor when known.
	pub active_monitor: Option<MonitorRect>,
	/// Highlighted live window when one exists.
	pub highlighted_window: Option<WindowRect>,
	/// Live drag preview rectangle before a frozen capture commits.
	pub live_selection_preview: Option<GlobalRect>,
	/// Frozen selection rectangle in global coordinates when committed.
	pub frozen_selection: Option<GlobalRect>,
	/// Lightweight HUD state.
	pub hud: HudModel,
	/// Semantic toolbar items currently available.
	pub toolbar_items: Vec<ToolbarItemModel>,
	/// Optional human-readable status line.
	pub status_message: Option<String>,
}
impl SceneModel {
	/// Creates a hidden scene snapshot.
	#[must_use]
	pub fn hidden() -> Self {
		Self::default()
	}
}

/// A deferred OCR job emitted by the product core and executed by the host.
#[derive(Debug, Eq, PartialEq)]
pub struct DeferredTextRecognitionRequest {
	/// Monotonic request identifier used to correlate logs across threads.
	pub request_id: u64,
	/// Wall-clock timestamp in Unix milliseconds captured when the request was issued.
	pub requested_at_unix_ms: u64,
	image_source: DeferredTextRecognitionImageSource,
}
impl DeferredTextRecognitionRequest {
	/// Creates a request from an already prepared export image.
	#[doc(hidden)]
	pub fn prepared(request_id: u64, requested_at_unix_ms: u64, image: RgbaImage) -> Self {
		Self {
			request_id,
			requested_at_unix_ms,
			image_source: DeferredTextRecognitionImageSource::Prepared { image },
		}
	}

	/// Creates a request that crops from a frozen export image.
	#[doc(hidden)]
	pub fn frozen_crop(
		request_id: u64,
		requested_at_unix_ms: u64,
		export_image: RgbaImage,
		crop_rect: Option<RectPoints>,
	) -> Self {
		Self {
			request_id,
			requested_at_unix_ms,
			image_source: DeferredTextRecognitionImageSource::FrozenCrop {
				export_image,
				crop_rect,
			},
		}
	}

	/// Returns the dimensions of the eventual OCR export image.
	#[must_use]
	pub fn image_dimensions(&self) -> (u32, u32) {
		self.image_source.image_dimensions()
	}

	/// Converts the request into a host-ready export image.
	#[doc(hidden)]
	pub fn into_export_image(self) -> Option<RgbaImage> {
		self.image_source.into_export_image()
	}

	/// Returns the export image snapshot for validation.
	#[doc(hidden)]
	pub fn export_image(&self) -> Option<RgbaImage> {
		self.image_source.export_image()
	}

	/// Test helper for building prepared OCR requests.
	#[doc(hidden)]
	pub fn debug_prepared_for_test(
		request_id: u64,
		requested_at_unix_ms: u64,
		image: RgbaImage,
	) -> Self {
		Self::prepared(request_id, requested_at_unix_ms, image)
	}
}

/// Structured result returned after a deferred OCR request finishes.
#[derive(Debug, Eq, PartialEq)]
pub struct DeferredTextRecognitionOutcome {
	/// Monotonic request identifier used to correlate logs across threads.
	pub request_id: u64,
	/// Final high-level outcome for the deferred OCR request.
	pub kind: DeferredTextRecognitionOutcomeKind,
	/// Number of non-empty lines returned by the host OCR engine.
	pub recognized_lines: usize,
	/// Number of characters returned after line joining.
	pub recognized_chars: usize,
	/// Recognized text to publish through the host-owned clipboard effect.
	pub recognized_text: Option<String>,
}

#[cfg(test)]
mod tests {
	use image::RgbaImage;

	use crate::RectPoints;
	use crate::protocol::{DeferredTextRecognitionRequest, OutputNaming};

	#[test]
	fn deferred_ocr_request_exports_frozen_crop() {
		let mut image = RgbaImage::new(4, 4);

		image.put_pixel(1, 1, image::Rgba([10, 20, 30, 255]));
		image.put_pixel(2, 2, image::Rgba([40, 50, 60, 255]));

		let request = DeferredTextRecognitionRequest::frozen_crop(
			7,
			1_234,
			image,
			Some(RectPoints::new(1, 1, 2, 2)),
		);
		let export = request.export_image().expect("cropped export image");

		assert_eq!(request.image_dimensions(), (2, 2));
		assert_eq!(export.dimensions(), (2, 2));
	}

	#[test]
	fn output_naming_defaults_to_timestamp() {
		assert_eq!(OutputNaming::default(), OutputNaming::Timestamp);
	}
}
