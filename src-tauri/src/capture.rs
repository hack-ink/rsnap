use std::{
	fs,
	path::{Path, PathBuf},
};

use image::RgbaImage;
use tauri::{AppHandle, Manager, Runtime};

const LAST_CAPTURE_FILE: &str = "last_capture.png";

#[derive(Clone, Copy, Debug)]
pub struct RectI32 {
	pub x: i32,
	pub y: i32,
	pub width: u32,
	pub height: u32,
}

pub fn capture_primary_display_to_cache<R>(app: &AppHandle<R>) -> Result<PathBuf, String>
where
	R: Runtime,
{
	let cache_dir = app_cache_path(app)?;
	let monitors = xcap::Monitor::all()
		.map_err(|err| format!("Unable to enumerate monitors: {err}"))?
		.into_iter()
		.collect::<Vec<_>>();
	let monitor = monitors
		.iter()
		.find(|monitor| monitor.is_primary())
		.or_else(|| monitors.first())
		.ok_or_else(|| String::from("No monitor detected"))?;
	let image =
		monitor.capture_image().map_err(|err| format!("Failed to capture display: {err}"))?;

	save_capture_to_cache_dir(&cache_dir, &image)
}

pub fn capture_window_to_cache<R>(app: &AppHandle<R>, window_id: u32) -> Result<PathBuf, String>
where
	R: Runtime,
{
	let cache_dir = app_cache_path(app)?;
	let window = xcap::Window::all()
		.map_err(|err| format!("Unable to enumerate windows: {err}"))?
		.into_iter()
		.find(|window| window.id() == window_id)
		.ok_or_else(|| format!("Selected window not found: {window_id}"))?;
	let image = window
		.capture_image()
		.map_err(|err| format!("Failed to capture window {window_id}: {err}"))?;

	save_capture_to_cache_dir(&cache_dir, &image)
}

pub fn capture_region_to_cache<R>(app: &AppHandle<R>, rect: RectI32) -> Result<PathBuf, String>
where
	R: Runtime,
{
	if rect.width == 0 || rect.height == 0 {
		return Err(String::from("Selected region is empty"));
	}

	let cache_dir = app_cache_path(app)?;
	let (cx, cy) = rect_center_point(rect)?;
	let monitor = xcap::Monitor::from_point(cx, cy)
		.map_err(|err| format!("Failed to resolve monitor for point ({cx},{cy}): {err}"))?;

	ensure_rect_within_monitor(rect, &monitor)?;

	let image = monitor
		.capture_image()
		.map_err(|err| format!("Failed to capture monitor {}: {err}", monitor.name()))?;
	let (crop_x, crop_y, crop_w, crop_h) = monitor_crop_rect_px(rect, &monitor, &image)?;
	let cropped = image::imageops::crop_imm(&image, crop_x, crop_y, crop_w, crop_h).to_image();

	save_capture_to_cache_dir(&cache_dir, &cropped)
}

pub fn app_cache_path<R>(app: &AppHandle<R>) -> Result<PathBuf, String>
where
	R: Runtime,
{
	app.path()
		.app_cache_dir()
		.map_err(|err| format!("Failed to resolve app cache directory: {err}"))
}

pub fn last_capture_path<R>(app: &AppHandle<R>) -> Result<PathBuf, String>
where
	R: Runtime,
{
	let cache_dir = app_cache_path(app)?;

	Ok(resolve_output_path(&cache_dir))
}

fn rect_center_point(rect: RectI32) -> Result<(i32, i32), String> {
	let cx = i64::from(rect.x).saturating_add(i64::from(rect.width) / 2);
	let cy = i64::from(rect.y).saturating_add(i64::from(rect.height) / 2);
	let cx = i32::try_from(cx).map_err(|_| format!("Region center x overflow: {cx}"))?;
	let cy = i32::try_from(cy).map_err(|_| format!("Region center y overflow: {cy}"))?;

	Ok((cx, cy))
}

fn ensure_rect_within_monitor(rect: RectI32, monitor: &xcap::Monitor) -> Result<(), String> {
	let left = i64::from(rect.x);
	let top = i64::from(rect.y);
	let right = left.saturating_add(i64::from(rect.width));
	let bottom = top.saturating_add(i64::from(rect.height));
	let m_left = i64::from(monitor.x());
	let m_top = i64::from(monitor.y());
	let m_right = m_left.saturating_add(i64::from(monitor.width()));
	let m_bottom = m_top.saturating_add(i64::from(monitor.height()));

	if left < m_left || top < m_top || right > m_right || bottom > m_bottom {
		return Err(format!(
			"Selected region crosses displays (not supported yet): rect=({left},{top},{right},{bottom}) monitor=({m_left},{m_top},{m_right},{m_bottom})",
		));
	}

	Ok(())
}

fn monitor_crop_rect_px(
	rect: RectI32,
	monitor: &xcap::Monitor,
	image: &RgbaImage,
) -> Result<(u32, u32, u32, u32), String> {
	let local_x = rect.x.saturating_sub(monitor.x());
	let local_y = rect.y.saturating_sub(monitor.y());
	#[cfg(target_os = "macos")]
	let scale = monitor.scale_factor() as f64;
	#[cfg(not(target_os = "macos"))]
	let scale = 1.0_f64;
	let crop_x = ((local_x as f64) * scale).round().max(0.0) as u32;
	let crop_y = ((local_y as f64) * scale).round().max(0.0) as u32;
	let crop_w = ((rect.width as f64) * scale).round().max(0.0) as u32;
	let crop_h = ((rect.height as f64) * scale).round().max(0.0) as u32;
	let image_w = image.width();
	let image_h = image.height();

	if crop_x >= image_w || crop_y >= image_h {
		return Err(format!(
			"Selected region is outside captured monitor image: crop=({crop_x},{crop_y},{crop_w},{crop_h}) image=({image_w}x{image_h})",
		));
	}

	let crop_w = crop_w.min(image_w - crop_x);
	let crop_h = crop_h.min(image_h - crop_y);

	if crop_w == 0 || crop_h == 0 {
		return Err(String::from("Selected region is empty after scaling"));
	}

	Ok((crop_x, crop_y, crop_w, crop_h))
}

fn resolve_output_path(cache_dir: &Path) -> PathBuf {
	cache_dir.join(LAST_CAPTURE_FILE)
}

fn save_capture_to_cache_dir(cache_dir: &Path, image: &RgbaImage) -> Result<PathBuf, String> {
	fs::create_dir_all(cache_dir).map_err(|err| {
		format!("Failed to create cache directory {}: {err}", cache_dir.display())
	})?;

	let output_path = resolve_output_path(cache_dir);

	image
		.save(&output_path)
		.map_err(|err| format!("Failed to save image to {}: {err}", output_path.display()))?;

	Ok(output_path)
}

#[cfg(test)]
mod tests {
	use crate::capture::resolve_output_path;
	use std::path::Path;

	#[test]
	fn capture_output_path_uses_last_capture_filename() {
		let cache_dir = Path::new("cache_dir");
		let output_path = resolve_output_path(cache_dir);

		assert_eq!(output_path.file_name().unwrap(), "last_capture.png");
		assert_eq!(output_path.parent().unwrap(), cache_dir);
	}
}
