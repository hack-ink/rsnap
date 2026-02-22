use std::{
	fs,
	path::{Path, PathBuf},
};

use tauri::{AppHandle, Manager, Runtime};

const LAST_CAPTURE_FILE: &str = "last_capture.png";

pub fn capture_primary_display_to_cache<R>(app: &AppHandle<R>) -> Result<PathBuf, String>
where
	R: Runtime,
{
	let cache_dir = app
		.path()
		.app_cache_dir()
		.map_err(|err| format!("Failed to resolve app cache directory: {err}"))?;
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

	fs::create_dir_all(&cache_dir).map_err(|err| {
		format!("Failed to create cache directory {}: {err}", cache_dir.display())
	})?;

	let output_path = resolve_output_path(&cache_dir);

	image
		.save(&output_path)
		.map_err(|err| format!("Failed to save image to {}: {err}", output_path.display()))?;

	Ok(output_path)
}

fn resolve_output_path(cache_dir: &Path) -> PathBuf {
	cache_dir.join(LAST_CAPTURE_FILE)
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
