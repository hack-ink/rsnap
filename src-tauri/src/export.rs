use std::{
	borrow::Cow,
	fs,
	path::{Path, PathBuf},
};

use arboard::{Clipboard, ImageData};
use base64::{Engine as _, engine::general_purpose::STANDARD};

pub fn save_png_base64_to_dir(
	output_dir: &Path,
	file_name: String,
	png_base64: String,
) -> Result<String, String> {
	let bytes = decode_png_base64(png_base64)?;

	save_png_bytes_to_dir(output_dir, file_name, bytes)
}

pub fn copy_png_base64(png_base64: String) -> Result<(), String> {
	let bytes = decode_png_base64(png_base64)?;
	let image =
		image::load_from_memory(&bytes).map_err(|err| format!("Failed to decode PNG: {err}"))?;
	let image = image.to_rgba8();
	let width = image.width();
	let height = image.height();
	let image = ImageData {
		width: width as usize,
		height: height as usize,
		bytes: Cow::Owned(image.into_raw()),
	};
	let mut clipboard =
		Clipboard::new().map_err(|err| format!("Failed to open clipboard: {err}"))?;

	clipboard
		.set_image(image)
		.map_err(|err| format!("Failed to write image to clipboard: {err}"))?;

	Ok(())
}

fn decode_png_base64(png_base64: String) -> Result<Vec<u8>, String> {
	STANDARD.decode(png_base64.trim()).map_err(|err| format!("Invalid base64 png data: {err}"))
}

fn output_path_for_downloads(downloads_dir: &Path, file_name: String) -> Result<PathBuf, String> {
	if file_name.trim().is_empty() {
		return Err(String::from("Missing output filename"));
	}
	if !file_name.ends_with(".png") {
		return Err(String::from("Output filename must end with .png"));
	}

	let path = downloads_dir.join(&file_name);

	validate_png_extension(&path)?;

	Ok(path)
}

fn save_png_bytes_to_dir(
	output_dir: &Path,
	file_name: String,
	bytes: Vec<u8>,
) -> Result<String, String> {
	fs::create_dir_all(output_dir).map_err(|err| {
		format!("Failed to create output directory {}: {err}", output_dir.display())
	})?;

	let output_path = output_path_for_downloads(output_dir, file_name)?;

	fs::write(&output_path, bytes)
		.map_err(|err| format!("Failed to save png to {}: {err}", output_path.display()))?;

	Ok(output_path.to_string_lossy().to_string())
}

fn validate_png_extension(path: &Path) -> Result<(), String> {
	let ext = path.extension().and_then(|value| value.to_str());

	match ext {
		Some("png") => Ok(()),
		Some("PNG") => Ok(()),
		_ => Err(String::from("Only PNG output files are supported")),
	}
}
