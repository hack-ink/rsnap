use color_eyre::eyre::{Result, WrapErr};
use tray_icon::Icon;

const TRAY_ICON_PNG_BYTES: &[u8] =
	include_bytes!("../assets/tray-icon/generated/tray-icon-template.png");

pub(crate) fn default_tray_icon() -> Result<Icon> {
	let image = image::load_from_memory(TRAY_ICON_PNG_BYTES)
		.wrap_err("Failed to decode tray icon PNG bytes")?;
	let rgba = image.into_rgba8();
	let (width, height) = rgba.dimensions();

	Icon::from_rgba(rgba.into_raw(), width, height)
		.wrap_err("Failed to build tray icon from embedded RGBA bytes")
}
