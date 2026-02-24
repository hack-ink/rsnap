use color_eyre::eyre::{Result, WrapErr};
use tray_icon::Icon;

pub fn default_tray_icon() -> Result<Icon> {
	let width: u32 = 16;
	let height: u32 = 16;
	let mut rgba = vec![0_u8; (width * height * 4) as usize];

	for y in 0..height {
		for x in 0..width {
			let i = ((y * width + x) * 4) as usize;
			let is_border = x == 0 || y == 0 || x == width - 1 || y == height - 1;
			let is_dot = (6..=9).contains(&x) && (6..=9).contains(&y);

			if is_border || is_dot {
				rgba[i] = 0;
				rgba[i + 1] = 0;
				rgba[i + 2] = 0;
				rgba[i + 3] = 255;
			}
		}
	}

	Icon::from_rgba(rgba, width, height).wrap_err("Failed to build tray icon from RGBA bytes")
}
