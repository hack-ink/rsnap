use std::{
	fs,
	path::{Path, PathBuf},
};

use image::{Rgba, RgbaImage};
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
	let monitors = xcap::Monitor::all()
		.map_err(|err| format!("Unable to enumerate monitors: {err}"))?
		.into_iter()
		.collect::<Vec<_>>();

	if monitors.is_empty() {
		return Err(String::from("No monitor detected"));
	}

	struct MonitorCapture {
		x: i32,
		y: i32,
		width: u32,
		height: u32,
		image: RgbaImage,
	}

	let mut captures = Vec::with_capacity(monitors.len());

	for monitor in monitors {
		let image = monitor
			.capture_image()
			.map_err(|err| format!("Failed to capture monitor {}: {err}", monitor.name()))?;

		captures.push(MonitorCapture {
			x: monitor.x(),
			y: monitor.y(),
			width: monitor.width(),
			height: monitor.height(),
			image,
		});
	}

	let min_x = captures
		.iter()
		.map(|cap| cap.x)
		.min()
		.ok_or_else(|| String::from("No monitor detected"))?;
	let min_y = captures
		.iter()
		.map(|cap| cap.y)
		.min()
		.ok_or_else(|| String::from("No monitor detected"))?;
	let max_x = captures
		.iter()
		.map(|cap| cap.x.saturating_add_unsigned(cap.width))
		.max()
		.ok_or_else(|| String::from("No monitor detected"))?;
	let max_y = captures
		.iter()
		.map(|cap| cap.y.saturating_add_unsigned(cap.height))
		.max()
		.ok_or_else(|| String::from("No monitor detected"))?;
	let desktop_width = u32::try_from(max_x - min_x)
		.map_err(|_| format!("Virtual desktop width overflow: min_x={min_x} max_x={max_x}"))?;
	let desktop_height = u32::try_from(max_y - min_y)
		.map_err(|_| format!("Virtual desktop height overflow: min_y={min_y} max_y={max_y}"))?;
	let mut desktop = RgbaImage::from_pixel(desktop_width, desktop_height, Rgba([0, 0, 0, 0]));

	for cap in captures {
		let offset_x = i64::from(cap.x - min_x);
		let offset_y = i64::from(cap.y - min_y);

		image::imageops::overlay(&mut desktop, &cap.image, offset_x, offset_y);
	}

	let desktop_left = i64::from(min_x);
	let desktop_top = i64::from(min_y);
	let desktop_right = i64::from(max_x);
	let desktop_bottom = i64::from(max_y);
	let rect_left = i64::from(rect.x);
	let rect_top = i64::from(rect.y);
	let rect_right = rect_left.saturating_add(rect.width.into());
	let rect_bottom = rect_top.saturating_add(rect.height.into());
	let crop_left = rect_left.max(desktop_left);
	let crop_top = rect_top.max(desktop_top);
	let crop_right = rect_right.min(desktop_right);
	let crop_bottom = rect_bottom.min(desktop_bottom);

	if crop_right <= crop_left || crop_bottom <= crop_top {
		return Err(format!(
			"Selected region is outside the virtual desktop bounds: rect=({},{},{},{}) desktop=({},{},{},{})",
			rect_left,
			rect_top,
			rect_right,
			rect_bottom,
			desktop_left,
			desktop_top,
			desktop_right,
			desktop_bottom
		));
	}

	let crop_x = u32::try_from(crop_left - desktop_left)
		.map_err(|_| format!("Invalid crop x offset: crop_left={crop_left}"))?;
	let crop_y = u32::try_from(crop_top - desktop_top)
		.map_err(|_| format!("Invalid crop y offset: crop_top={crop_top}"))?;
	let crop_w = u32::try_from(crop_right - crop_left).map_err(|_| {
		format!("Invalid crop width: crop_left={crop_left} crop_right={crop_right}")
	})?;
	let crop_h = u32::try_from(crop_bottom - crop_top).map_err(|_| {
		format!("Invalid crop height: crop_top={crop_top} crop_bottom={crop_bottom}")
	})?;
	let cropped = image::imageops::crop_imm(&desktop, crop_x, crop_y, crop_w, crop_h).to_image();

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
