use std::fs;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use tauri::{AppHandle, Manager, Runtime, Wry};

use crate::{capture, export};

pub fn capture_now_with_app<R>(app: &AppHandle<R>) -> Result<(), String>
where
	R: Runtime,
{
	capture::capture_primary_display_to_cache(app)?;

	reveal_main_window(app)
}

#[tauri::command]
pub fn capture_now(app: AppHandle<Wry>) -> Result<(), String> {
	capture_now_with_app(&app)
}

#[tauri::command]
pub fn get_last_capture_base64(app: AppHandle<Wry>) -> Result<String, String> {
	let path = capture::last_capture_path(&app)?;
	let bytes = fs::read(path.clone())
		.map_err(|err| format!("Failed to read capture file {}: {err}", path.display()))?;

	Ok(STANDARD.encode(bytes))
}

#[tauri::command]
pub fn save_png_base64(png_base64: String) -> Result<String, String> {
	let timestamp = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_err(|err| format!("Failed to compute timestamp: {err}"))?
		.as_millis();
	let default_name = format!("rsnap-capture-{timestamp}.png");

	export::save_png_base64_to_downloads(default_name, png_base64)
}

#[tauri::command]
pub fn copy_png_base64(png_base64: String) -> Result<(), String> {
	export::copy_png_base64(png_base64)
}

#[tauri::command]
pub fn open_pin_window(app: AppHandle<Wry>) -> Result<(), String> {
	reveal_main_window(&app)?;

	let window =
		app.get_webview_window("main").ok_or_else(|| String::from("Main window not found"))?;

	window
		.set_always_on_top(true)
		.map_err(|err| format!("Failed to enable always-on-top window mode: {err}"))?;

	Ok(())
}

fn reveal_main_window<R>(app: &AppHandle<R>) -> Result<(), String>
where
	R: Runtime,
{
	let window =
		app.get_webview_window("main").ok_or_else(|| String::from("Main window not found"))?;

	window.show().map_err(|err| format!("Failed to show editor window: {err}"))?;
	window.set_focus().map_err(|err| format!("Failed to focus editor window: {err}"))?;
	window
		.eval("window.location.reload()")
		.map_err(|err| format!("Failed to refresh editor content: {err}"))?;

	Ok(())
}
