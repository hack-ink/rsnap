#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
use objc2_app_kit::NSScreen;
#[cfg(target_os = "macos")]
use objc2_foundation::NSArray;

use crate::overlay::{GlobalPoint, MonitorRect, OverlaySession};

impl OverlaySession {
	pub(super) fn available_overlay_monitors(&self) -> Result<Vec<MonitorRect>, String> {
		#[cfg(target_os = "macos")]
		{
			Self::macos_monitor_rects()
		}

		#[cfg(not(target_os = "macos"))]
		{
			let monitors =
				xcap::Monitor::all().map_err(|err| format!("xcap Monitor::all failed: {err:?}"))?;
			let mut monitor_rects = Vec::with_capacity(monitors.len());

			for monitor in &monitors {
				monitor_rects.push(Self::monitor_rect_from_xcap_monitor(monitor)?);
			}

			Ok(monitor_rects)
		}
	}

	#[cfg(target_os = "macos")]
	fn macos_monitor_rects() -> Result<Vec<MonitorRect>, String> {
		let mtm = MainThreadMarker::new()
			.ok_or_else(|| String::from("Overlay startup requires the macOS main thread."))?;
		let screens = NSScreen::screens(mtm);
		let main_display_height = Self::main_display_height_points_from_screens(&screens)
			.ok_or_else(|| String::from("Overlay startup could not determine the main display."))?;
		let mut monitor_rects = Vec::with_capacity(screens.len());

		for screen in screens.iter() {
			let frame = screen.frame();
			let width = frame.size.width.round().max(0.0) as u32;
			let height = frame.size.height.round().max(0.0) as u32;

			if width == 0 || height == 0 {
				continue;
			}

			let scale_factor_x1000 =
				(screen.backingScaleFactor() * 1_000.0).round().max(1.0) as u32;
			let monitor_rect = MonitorRect {
				id: screen.CGDirectDisplayID(),
				origin: GlobalPoint::new(
					frame.origin.x.round() as i32,
					Self::window_server_top_from_appkit_bounds(
						frame.origin.y.round() as i64,
						height.into(),
						main_display_height,
					) as i32,
				),
				width,
				height,
				scale_factor_x1000,
			};

			if monitor_rect.id == 0 {
				continue;
			}

			monitor_rects.push(monitor_rect);
		}

		Ok(monitor_rects)
	}

	#[cfg(any(test, target_os = "macos"))]
	fn window_server_top_from_appkit_bounds(
		appkit_origin_y: i64,
		frame_height: i64,
		main_display_height: i64,
	) -> i64 {
		// AppKit screen coordinates are rooted at the main display's lower-left corner, while
		// Quartz global display coordinates are rooted at the main display's upper-left corner.
		// Secondary-display offsets are already encoded in `appkit_origin_y`, so only the main
		// display height participates in the y-axis flip into WindowServer space.
		main_display_height - (appkit_origin_y + frame_height)
	}

	#[cfg(target_os = "macos")]
	fn main_display_height_points_from_screens(screens: &NSArray<NSScreen>) -> Option<i64> {
		screens
			.iter()
			.find_map(|screen| {
				let frame = screen.frame();
				let origin_x = frame.origin.x.round() as i64;
				let origin_y = frame.origin.y.round() as i64;

				(origin_x == 0 && origin_y == 0).then(|| frame.size.height.round() as i64)
			})
			.or_else(|| {
				screens.iter().next().map(|screen| screen.frame().size.height.round() as i64)
			})
	}

	#[cfg(not(target_os = "macos"))]
	fn monitor_rect_from_xcap_monitor(monitor: &xcap::Monitor) -> Result<MonitorRect, String> {
		Ok(MonitorRect {
			id: monitor.id().map_err(|err| {
				format!(
					"Failed to read xcap monitor id while enumerating overlay monitors: {err:?}"
				)
			})?,
			origin: GlobalPoint::new(
				monitor.x().map_err(|err| {
					format!(
						"Failed to read xcap monitor x position while enumerating overlay monitors: {err:?}"
					)
				})?,
				monitor.y().map_err(|err| {
					format!(
						"Failed to read xcap monitor y position while enumerating overlay monitors: {err:?}"
					)
				})?,
			),
			width: monitor.width().map_err(|err| {
				format!(
					"Failed to read xcap monitor width while enumerating overlay monitors: {err:?}"
				)
			})?,
			height: monitor.height().map_err(|err| {
				format!(
					"Failed to read xcap monitor height while enumerating overlay monitors: {err:?}"
				)
			})?,
			scale_factor_x1000: {
				let scale_factor = monitor.scale_factor().map_err(|err| {
					format!(
						"Failed to read xcap monitor scale factor while enumerating overlay monitors: {err:?}"
					)
				})?;

				(scale_factor * 1_000.0).round() as u32
			},
		})
	}
}

#[cfg(test)]
mod tests {
	use crate::overlay::OverlaySession;

	#[test]
	fn window_server_top_from_appkit_bounds_maps_display_above_main_into_negative_y() {
		assert_eq!(OverlaySession::window_server_top_from_appkit_bounds(1_120, 360, 900), -580);
	}

	#[test]
	fn window_server_top_from_appkit_bounds_maps_display_below_main_below_main_display_height() {
		assert_eq!(OverlaySession::window_server_top_from_appkit_bounds(-760, 360, 900), 1_300);
	}
}
