use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;
pub use rsnap_capture_core::geometry::{
	GlobalPoint, MonitorRect, MonitorRectPoints, RectPoints, Rgb, WindowHit, WindowRect,
};

#[derive(Debug)]
pub(crate) struct LoupeSample {
	pub center: GlobalPoint,
	pub patch: RgbaImage,
}

#[derive(Debug)]
/// Cached full-monitor frame used for RGB and loupe sampling.
pub struct MonitorImageSnapshot {
	/// When the frame was captured.
	pub captured_at: Instant,
	/// Live-stream generation that produced this frame.
	pub stream_generation: u64,
	/// The monitor that produced this frame.
	pub monitor: MonitorRect,
	/// The captured monitor image in RGBA pixel format.
	pub image: Arc<RgbaImage>,
}

#[derive(Debug)]
/// Combined live cursor sample containing the current RGB and optional loupe patch.
pub struct LiveCursorSample {
	/// The sampled RGB value under the cursor when available.
	pub rgb: Option<Rgb>,
	/// The sampled loupe patch when requested and available.
	pub patch: Option<RgbaImage>,
}

#[derive(Debug)]
/// Cached window-list snapshot used for live hit testing.
pub struct WindowListSnapshot {
	/// When the snapshot was captured.
	pub captured_at: Instant,
	/// Windows ordered for hit testing.
	pub windows: Arc<Vec<WindowRect>>,
}

#[derive(Clone, Copy, Debug)]
/// Internal overlay runtime mode.
pub enum OverlayMode {
	Live,
	Frozen,
}

#[derive(Debug)]
/// Internal mutable state owned by a running overlay session.
pub struct OverlayState {
	pub mode: OverlayMode,
	pub cursor: Option<GlobalPoint>,
	pub rgb: Option<Rgb>,
	pub monitor: Option<MonitorRect>,
	pub hovered_window_rect: Option<MonitorRectPoints>,
	pub drag_rect: Option<MonitorRectPoints>,
	pub frozen_capture_rect: Option<RectPoints>,
	pub frozen_mosaic_preview_rect: Option<RectPoints>,
	pub live_bg_monitor: Option<MonitorRect>,
	pub live_bg_image: Option<RgbaImage>,
	pub live_bg_generation: u64,
	pub frozen_display_image: Option<RgbaImage>,
	pub frozen_export_image: Option<RgbaImage>,
	pub frozen_generation: u64,
	pub error_message: Option<String>,
	pub alt_held: bool,
	pub loupe: Option<LoupeSample>,
	pub loupe_patch_side_px: u32,
}
impl OverlayState {
	pub fn new() -> Self {
		Self {
			mode: OverlayMode::Live,
			cursor: None,
			rgb: None,
			monitor: None,
			hovered_window_rect: None,
			drag_rect: None,
			frozen_capture_rect: None,
			frozen_mosaic_preview_rect: None,
			live_bg_monitor: None,
			live_bg_image: None,
			live_bg_generation: 0,
			frozen_display_image: None,
			frozen_export_image: None,
			frozen_generation: 0,
			error_message: None,
			alt_held: false,
			loupe: None,
			loupe_patch_side_px: 21,
		}
	}

	pub fn set_error(&mut self, message: impl Into<String>) {
		self.error_message = Some(message.into());
	}

	pub fn clear_error(&mut self) {
		self.error_message = None;
	}

	pub fn reset_for_start(&mut self, loupe_patch_side_px: u32) {
		*self = Self::new();
		self.loupe_patch_side_px = loupe_patch_side_px;
	}

	pub fn begin_freeze(&mut self, monitor: MonitorRect) {
		self.monitor = Some(monitor);
		self.frozen_display_image = None;
		self.frozen_export_image = None;
		self.frozen_mosaic_preview_rect = None;
		self.loupe = None;
		self.mode = OverlayMode::Frozen;
		self.frozen_generation = self.frozen_generation.wrapping_add(1);
	}

	#[cfg(any(test, not(target_os = "macos")))]
	pub fn commit_frozen_final_image(&mut self, monitor: MonitorRect, image: RgbaImage) {
		// Keep the existing generation set by `begin_freeze` so renderers can key off a single
		// freeze request/response cycle.
		self.monitor = Some(monitor);
		self.frozen_export_image = Some(image.clone());
		self.frozen_display_image = Some(image);
		self.mode = OverlayMode::Frozen;
	}

	pub fn commit_frozen_display_image(&mut self, monitor: MonitorRect, image: RgbaImage) {
		self.monitor = Some(monitor);
		self.frozen_display_image = Some(image);
		self.mode = OverlayMode::Frozen;
	}

	pub fn commit_frozen_export_image(&mut self, image: RgbaImage) {
		self.frozen_export_image = Some(image);
	}

	pub fn frozen_display_surface_image(&self) -> Option<&RgbaImage> {
		self.frozen_display_image.as_ref()
	}
}

#[cfg(test)]
mod tests {
	use image::{Rgba, RgbaImage};

	use crate::state::{GlobalPoint, MonitorRect, RectPoints};

	#[test]
	fn monitor_contains_and_local_coords() {
		let monitor = MonitorRect {
			id: 0,
			origin: GlobalPoint::new(-100, 50),
			width: 200,
			height: 100,
			scale_factor_x1000: 1_000,
		};

		assert!(monitor.contains(GlobalPoint::new(-100, 50)));
		assert!(monitor.contains(GlobalPoint::new(99, 149)));
		assert!(!monitor.contains(GlobalPoint::new(100, 149)));
		assert!(!monitor.contains(GlobalPoint::new(99, 150)));
		assert_eq!(monitor.local_u32(GlobalPoint::new(-100, 50)), Some((0, 0)));
		assert_eq!(monitor.local_u32(GlobalPoint::new(-1, 51)), Some((99, 1)));
		assert_eq!(monitor.local_u32(GlobalPoint::new(100, 50)), None);
	}

	#[test]
	fn local_rect_and_pixels() {
		let monitor = MonitorRect {
			id: 0,
			origin: GlobalPoint::new(-100, -100),
			width: 300,
			height: 200,
			scale_factor_x1000: 2_000,
		};
		let rect = monitor.clip_global_rect(-90, -80, 40, 50).expect("clipped local rect");

		assert_eq!(rect, RectPoints::new(10, 20, 130, 130));
		assert!(rect.contains((20, 30)));

		let pixel_rect = monitor.local_rect_to_pixels(rect);

		assert_eq!(pixel_rect, RectPoints::new(20, 40, 260, 260));
	}

	#[test]
	fn begin_freeze_clears_frozen_images() {
		let monitor = MonitorRect {
			id: 7,
			origin: GlobalPoint::new(0, 0),
			width: 100,
			height: 100,
			scale_factor_x1000: 1_000,
		};
		let mut state = crate::state::OverlayState::new();

		state.commit_frozen_display_image(monitor, RgbaImage::new(2, 2));
		state.commit_frozen_export_image(RgbaImage::new(2, 2));
		state.begin_freeze(monitor);

		assert!(state.frozen_display_image.is_none());
		assert!(state.frozen_export_image.is_none());
	}

	#[test]
	fn commit_frozen_display_image_leaves_export_authority_unset() {
		let monitor = MonitorRect {
			id: 3,
			origin: GlobalPoint::new(0, 0),
			width: 100,
			height: 100,
			scale_factor_x1000: 1_000,
		};
		let display_image = RgbaImage::from_pixel(2, 2, Rgba([10, 20, 30, 255]));
		let mut state = crate::state::OverlayState::new();

		state.begin_freeze(monitor);
		state.commit_frozen_display_image(monitor, display_image.clone());

		assert_eq!(state.frozen_display_image.as_ref(), Some(&display_image));
		assert!(state.frozen_export_image.is_none());
		assert_eq!(state.frozen_display_surface_image(), Some(&display_image));
	}

	#[test]
	fn commit_frozen_final_image_populates_display_and_export_images() {
		let monitor = MonitorRect {
			id: 7,
			origin: GlobalPoint::new(0, 0),
			width: 100,
			height: 100,
			scale_factor_x1000: 1_000,
		};
		let final_image = RgbaImage::from_pixel(2, 2, Rgba([40, 50, 60, 255]));
		let mut state = crate::state::OverlayState::new();

		state.begin_freeze(monitor);
		state.commit_frozen_final_image(monitor, final_image.clone());

		assert_eq!(state.frozen_display_image.as_ref(), Some(&final_image));
		assert_eq!(state.frozen_export_image.as_ref(), Some(&final_image));
		assert_eq!(state.frozen_display_surface_image(), Some(&final_image));
	}
}
