use std::{
	fs,
	path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

const SETTINGS_FILE: &str = "settings.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
	pub output_dir: String,
}

pub fn load_settings<R>(app: &AppHandle<R>) -> Result<Settings, String>
where
	R: Runtime,
{
	let path = settings_path(app)?;

	let bytes = match fs::read(&path) {
		Ok(bytes) => bytes,
		Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
			return Ok(default_settings());
		},
		Err(err) => return Err(format!("Failed to read settings file {}: {err}", path.display())),
	};

	serde_json::from_slice(&bytes)
		.map_err(|err| format!("Invalid settings JSON in {}: {err}", path.display()))
}

pub fn save_settings<R>(app: &AppHandle<R>, settings: &Settings) -> Result<(), String>
where
	R: Runtime,
{
	let path = settings_path(app)?;
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent).map_err(|err| {
			format!("Failed to create settings directory {}: {err}", parent.display())
		})?;
	}

	let bytes = serde_json::to_vec_pretty(settings)
		.map_err(|err| format!("Failed to serialize settings JSON: {err}"))?;
	fs::write(&path, bytes)
		.map_err(|err| format!("Failed to write settings file {}: {err}", path.display()))?;

	Ok(())
}

pub fn resolve_output_dir<R>(app: &AppHandle<R>) -> Result<PathBuf, String>
where
	R: Runtime,
{
	let settings = load_settings(app)?;
	let value = expand_tilde(settings.output_dir.trim())?;
	if value.as_os_str().is_empty() {
		return Err(String::from("Output directory is empty"));
	}
	Ok(value)
}

pub fn default_settings() -> Settings {
	let output_dir = dirs::desktop_dir()
		.or_else(dirs::download_dir)
		.or_else(dirs::home_dir)
		.unwrap_or_else(|| PathBuf::from("."));

	Settings { output_dir: output_dir.to_string_lossy().to_string() }
}

fn settings_path<R>(app: &AppHandle<R>) -> Result<PathBuf, String>
where
	R: Runtime,
{
	let config_dir = app
		.path()
		.app_config_dir()
		.map_err(|err| format!("Failed to resolve app config directory: {err}"))?;

	Ok(config_dir.join(SETTINGS_FILE))
}

fn expand_tilde(value: &str) -> Result<PathBuf, String> {
	if value == "~" {
		return dirs::home_dir().ok_or_else(|| String::from("Unable to resolve home directory"));
	}

	if let Some(rest) = value.strip_prefix("~/") {
		let home =
			dirs::home_dir().ok_or_else(|| String::from("Unable to resolve home directory"))?;
		return Ok(home.join(rest));
	}

	Ok(Path::new(value).to_path_buf())
}
