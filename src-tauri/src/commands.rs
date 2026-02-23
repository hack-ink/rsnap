use std::{
	fs,
	io::{BufRead as _, BufReader},
	path::Path,
	process::{Command, Stdio},
	sync::mpsc,
	time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, Wry};

use crate::{capture, export, settings};

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum CaptureSelection {
	#[serde(rename = "cancel")]
	Cancel,
	#[serde(rename = "window")]
	Window { window_id: u32 },
	#[serde(rename = "region")]
	Region { rect: RectI32Json },
	#[serde(rename = "error")]
	Error { message: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct SettingsDto {
	pub output_dir: String,
}

#[derive(Debug, Deserialize)]
struct RectI32Json {
	x: i32,
	y: i32,
	width: u32,
	height: u32,
}

pub fn capture_now_with_app<R>(app: &AppHandle<R>) -> Result<(), String>
where
	R: Runtime,
{
	match read_capture_selection_from_sidecar(app) {
		Ok(selection) => match selection {
			CaptureSelection::Cancel => return Ok(()),
			CaptureSelection::Window { window_id } => {
				capture::capture_window_to_cache(app, window_id)?;
			},
			CaptureSelection::Region { rect } => {
				capture::capture_region_to_cache(
					app,
					capture::RectI32 {
						x: rect.x,
						y: rect.y,
						width: rect.width,
						height: rect.height,
					},
				)?;
			},
			CaptureSelection::Error { message } => {
				eprintln!("Overlay sidecar error: {message}");

				capture::capture_primary_display_to_cache(app)?;
			},
		},
		Err(err) => {
			eprintln!("Overlay sidecar failed; falling back to primary display: {err}");

			capture::capture_primary_display_to_cache(app)?;
		},
	}

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
pub fn save_png_base64(app: AppHandle<Wry>, png_base64: String) -> Result<String, String> {
	let timestamp = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_err(|err| format!("Failed to compute timestamp: {err}"))?
		.as_millis();
	let default_name = format!("rsnap-capture-{timestamp}.png");
	let output_dir = settings::resolve_output_dir(&app).unwrap_or_else(|_| {
		dirs::desktop_dir()
			.or_else(dirs::download_dir)
			.or_else(dirs::home_dir)
			.unwrap_or_else(|| std::path::PathBuf::from("."))
	});

	export::save_png_base64_to_dir(&output_dir, default_name, png_base64)
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

#[tauri::command]
pub fn get_settings(app: AppHandle<Wry>) -> Result<SettingsDto, String> {
	let settings = settings::load_settings(&app).unwrap_or_else(|_| settings::default_settings());

	Ok(SettingsDto { output_dir: settings.output_dir })
}

#[tauri::command]
pub fn set_output_dir(app: AppHandle<Wry>, output_dir: String) -> Result<SettingsDto, String> {
	let trimmed = output_dir.trim();

	if trimmed.is_empty() {
		return Err(String::from("Output directory is empty"));
	}

	let mut settings =
		settings::load_settings(&app).unwrap_or_else(|_| settings::default_settings());

	settings.output_dir = trimmed.to_string();

	settings::save_settings(&app, &settings)?;

	Ok(SettingsDto { output_dir: settings.output_dir })
}

pub fn show_settings_window<R>(app: &AppHandle<R>) -> Result<(), String>
where
	R: Runtime,
{
	if let Some(window) = app.get_webview_window("settings") {
		window.show().map_err(|err| format!("Failed to show settings window: {err}"))?;
		window.set_focus().map_err(|err| format!("Failed to focus settings window: {err}"))?;

		return Ok(());
	}

	let url = WebviewUrl::App(String::from("index.html?view=settings").into());
	let window = WebviewWindowBuilder::new(app, "settings", url)
		.title("rsnap Settings")
		.inner_size(520.0, 360.0)
		.resizable(false)
		.build()
		.map_err(|err| format!("Failed to create settings window: {err}"))?;

	window.show().map_err(|err| format!("Failed to show settings window: {err}"))?;
	window.set_focus().map_err(|err| format!("Failed to focus settings window: {err}"))?;

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

	let _ = window.emit("rsnap://capture-updated", ());

	Ok(())
}

fn read_capture_selection_from_sidecar<R>(app: &AppHandle<R>) -> Result<CaptureSelection, String>
where
	R: Runtime,
{
	let timeout = Duration::from_secs(10);
	let candidates = overlay_sidecar_candidates(app)?;
	let mut last_err = None;

	for candidate in candidates {
		match run_sidecar_and_read_one_line(&candidate, timeout) {
			Ok(line) => {
				let line = line.trim();

				if line.is_empty() {
					return Err(format!(
						"Overlay sidecar produced empty output: {}",
						candidate.display()
					));
				}

				let selection: CaptureSelection = serde_json::from_str(line).map_err(|err| {
					format!(
						"Invalid overlay JSON from {}: {err}; payload={line}",
						candidate.display()
					)
				})?;

				return Ok(selection);
			},
			Err(err) => {
				last_err = Some(format!("{}: {err}", candidate.display()));

				continue;
			},
		}
	}

	let hint = last_err.unwrap_or_else(|| String::from("No overlay sidecar candidates found"));

	Err(format!("Unable to launch overlay sidecar ({hint})"))
}

fn overlay_sidecar_candidates<R>(app: &AppHandle<R>) -> Result<Vec<std::path::PathBuf>, String>
where
	R: Runtime,
{
	if let Ok(path) = std::env::var("RSNAP_OVERLAY_PATH") {
		let path = std::path::PathBuf::from(path);

		return Ok(vec![path]);
	}

	let mut binary_names = if cfg!(windows) {
		vec![String::from("rsnap-overlay.exe"), String::from("rsnap-overlay")]
	} else {
		vec![String::from("rsnap-overlay")]
	};

	if let Some(target_triple) = option_env!("RSNAP_TARGET_TRIPLE") {
		if cfg!(windows) {
			binary_names.push(format!("rsnap-overlay-{target_triple}.exe"));
			binary_names.push(format!("rsnap-overlay-{target_triple}"));
		} else {
			binary_names.push(format!("rsnap-overlay-{target_triple}"));
		}
	}

	let mut candidates = Vec::new();

	if let Ok(current_exe) = std::env::current_exe()
		&& let Some(dir) = current_exe.parent()
	{
		for name in &binary_names {
			candidates.push(dir.join(name));
		}
	}
	if let Ok(resource_dir) = app.path().resource_dir() {
		for name in &binary_names {
			candidates.push(resource_dir.join(name));
		}
	}

	let mut existing = Vec::new();

	for candidate in candidates {
		if fs::metadata(&candidate).is_ok() {
			existing.push(candidate);
		}
	}

	if existing.is_empty() {
		return Err(String::from(
			"Sidecar not found; set RSNAP_OVERLAY_PATH or ensure rsnap-overlay is bundled",
		));
	}

	Ok(existing)
}

fn run_sidecar_and_read_one_line(path: &Path, timeout: Duration) -> Result<String, String> {
	let mut child = Command::new(path)
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|err| format!("Failed to spawn: {err}"))?;
	let stdout =
		child.stdout.take().ok_or_else(|| String::from("Failed to capture sidecar stdout"))?;
	let mut child_for_wait = child;
	let (tx, rx) = mpsc::channel();

	std::thread::spawn(move || {
		let mut reader = BufReader::new(stdout);
		let mut line = String::new();
		let res = reader.read_line(&mut line).map(|_| line);
		let _ = tx.send(res);
	});

	let line = match rx.recv_timeout(timeout) {
		Ok(res) => res.map_err(|err| format!("Failed to read sidecar stdout: {err}"))?,
		Err(_) => {
			let _ = child_for_wait.kill();
			let _ = child_for_wait.wait();

			return Err(format!(
				"Timed out waiting for sidecar response after {}s",
				timeout.as_secs()
			));
		},
	};
	let mut waited = Duration::from_millis(0);

	while waited < Duration::from_millis(500) {
		match child_for_wait.try_wait() {
			Ok(Some(_)) => return Ok(line),
			Ok(None) => {
				std::thread::sleep(Duration::from_millis(25));

				waited += Duration::from_millis(25);
			},
			Err(err) => return Err(format!("Failed waiting for sidecar exit: {err}")),
		}
	}

	let _ = child_for_wait.kill();
	let _ = child_for_wait.wait();

	Ok(line)
}
