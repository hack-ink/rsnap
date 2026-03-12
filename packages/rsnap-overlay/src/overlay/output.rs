#[cfg(not(target_os = "macos"))]
use std::borrow::Cow;
#[cfg(target_os = "macos")]
use std::ffi::CString;
use std::{
	fs,
	path::{Path, PathBuf},
	time::{SystemTime, UNIX_EPOCH},
};

use arboard::Clipboard;
#[cfg(not(target_os = "macos"))]
use arboard::ImageData;
#[cfg(target_os = "macos")]
use color_eyre::eyre;
use color_eyre::eyre::{Result, WrapErr};
#[cfg(target_os = "macos")]
use objc::runtime::{BOOL, Object, YES};

use crate::overlay::{OutputNaming, OverlayConfig};

#[cfg(target_os = "macos")]
macro_rules! sel {
	($($tt:tt)*) => {
		objc::sel!($($tt)*)
	};
}

#[cfg(target_os = "macos")]
macro_rules! sel_impl {
	($($tt:tt)*) => {
		objc::sel_impl!($($tt)*)
	};
}

pub(super) fn save_png_bytes_to_configured_dir(
	png_bytes: &[u8],
	config: &OverlayConfig,
) -> Result<PathBuf> {
	let output_dir = if config.output_dir.as_os_str().is_empty() {
		PathBuf::from(".")
	} else {
		config.output_dir.clone()
	};

	fs::create_dir_all(&output_dir)
		.wrap_err_with(|| format!("Failed to create output directory: {}", output_dir.display()))?;

	let prefix = sanitize_output_filename_prefix(&config.output_filename_prefix);
	let target_path = next_output_png_path(&output_dir, &prefix, config.output_naming);

	write_png_bytes_atomic(&target_path, png_bytes)?;

	Ok(target_path)
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
pub(super) fn write_png_bytes_to_clipboard(png_bytes: &[u8]) -> Result<()> {
	let pasteboard_type = CString::new("public.png").wrap_err("Invalid NSPasteboard type")?;

	unsafe {
		let data: *mut Object = objc::msg_send![objc::class!(NSData), dataWithBytes: png_bytes.as_ptr() length: png_bytes.len()];
		let pasteboard: *mut Object =
			objc::msg_send![objc::class!(NSPasteboard), generalPasteboard];
		let _: i64 = objc::msg_send![pasteboard, clearContents];
		let ty: *mut Object =
			objc::msg_send![objc::class!(NSString), stringWithUTF8String: pasteboard_type.as_ptr()];
		let ok: BOOL = objc::msg_send![pasteboard, setData: data forType: ty];

		if ok != YES {
			return Err(eyre::eyre!("NSPasteboard setData:forType failed"));
		}
	}

	Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn write_png_bytes_to_clipboard(png_bytes: &[u8]) -> Result<()> {
	let image = image::load_from_memory(png_bytes).wrap_err("Failed to decode PNG bytes")?;
	let rgba = image.to_rgba8();
	let (width, height) = rgba.dimensions();
	let mut clipboard = Clipboard::new().wrap_err("Failed to initialize clipboard")?;

	clipboard
		.set_image(ImageData {
			width: width as usize,
			height: height as usize,
			bytes: Cow::Owned(rgba.into_raw()),
		})
		.wrap_err("Failed to write image to clipboard")?;

	Ok(())
}

pub(super) fn write_text_to_clipboard(text: &str) -> Result<()> {
	let mut clipboard = Clipboard::new().wrap_err("Failed to initialize clipboard")?;

	clipboard.set_text(text.to_string()).wrap_err("Failed to write text to clipboard")?;

	Ok(())
}

fn sanitize_output_filename_prefix(raw: &str) -> String {
	let trimmed = raw.trim();
	let mut sanitized = String::with_capacity(trimmed.len());

	for ch in trimmed.chars() {
		if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
			sanitized.push(ch);
		} else {
			sanitized.push('_');
		}
	}

	let sanitized = sanitized.trim_matches('_');

	if sanitized.is_empty() { String::from("rsnap") } else { sanitized.to_owned() }
}

fn next_output_png_path(output_dir: &Path, prefix: &str, naming: OutputNaming) -> PathBuf {
	let base = match naming {
		OutputNaming::Timestamp => format!("{prefix}-{}", current_unix_millis()),
		OutputNaming::Sequence => {
			format!("{prefix}-{:04}", next_sequence_index(output_dir, prefix))
		},
	};

	unique_png_path(output_dir, &base)
}

fn current_unix_millis() -> u128 {
	SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_millis())
}

fn next_sequence_index(output_dir: &Path, prefix: &str) -> u32 {
	let Ok(entries) = fs::read_dir(output_dir) else {
		return 1;
	};
	let mut max_seen = 0_u32;

	for entry in entries.flatten() {
		let file_name = entry.file_name();
		let Some(file_name) = file_name.to_str() else {
			continue;
		};
		let Some(stem) = file_name.strip_suffix(".png") else {
			continue;
		};
		let Some(number_text) = stem.strip_prefix(prefix).and_then(|rest| rest.strip_prefix('-'))
		else {
			continue;
		};

		if !number_text.chars().all(|ch| ch.is_ascii_digit()) {
			continue;
		}

		if let Ok(value) = number_text.parse::<u32>() {
			max_seen = max_seen.max(value);
		}
	}

	max_seen.saturating_add(1).max(1)
}

fn unique_png_path(output_dir: &Path, base: &str) -> PathBuf {
	let direct_path = output_dir.join(format!("{base}.png"));

	if !direct_path.exists() {
		return direct_path;
	}

	let mut suffix = 2_u32;

	loop {
		let candidate = output_dir.join(format!("{base}-{suffix}.png"));

		if !candidate.exists() {
			return candidate;
		}

		suffix = suffix.saturating_add(1);
	}
}

fn write_png_bytes_atomic(target_path: &Path, png_bytes: &[u8]) -> Result<()> {
	let tmp_path = target_path.with_extension("png.tmp");

	fs::write(&tmp_path, png_bytes)
		.wrap_err_with(|| format!("Failed to write temporary PNG file: {}", tmp_path.display()))?;
	fs::rename(&tmp_path, target_path)
		.wrap_err_with(|| format!("Failed to finalize PNG file: {}", target_path.display()))?;

	Ok(())
}
