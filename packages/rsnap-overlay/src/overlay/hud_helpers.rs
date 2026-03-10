use crate::state::{GlobalPoint, MonitorRect, OverlayState, Rgb};
use winit::window::Theme;

use super::{
	HUD_PILL_BLUR_TINT_ALPHA_DARK, HUD_PILL_BLUR_TINT_ALPHA_LIGHT, HUD_PILL_BODY_FILL_DARK_SRGBA8,
	HUD_PILL_BODY_FILL_LIGHT_SRGBA8, HudTheme, ThemeMode,
};

pub(super) fn srgb8_to_linear_f32(x: u8) -> f32 {
	let c = (x as f32) / 255.0;

	if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub(super) fn rgb_to_hsl(rgb: Rgb) -> (f32, f32, f32) {
	let red = f32::from(rgb.r) / 255.0;
	let green = f32::from(rgb.g) / 255.0;
	let blue = f32::from(rgb.b) / 255.0;
	let max_channel = red.max(green).max(blue);
	let min_channel = red.min(green).min(blue);
	let delta = max_channel - min_channel;
	let lightness = (max_channel + min_channel) / 2.0;

	if delta <= f32::EPSILON {
		return (0.0, 0.0, lightness);
	}

	let saturation = if lightness > 0.5 {
		delta / (2.0 - max_channel - min_channel)
	} else {
		delta / (max_channel + min_channel)
	};
	let mut hue = if (max_channel - red).abs() <= f32::EPSILON {
		(green - blue) / delta + if green < blue { 6.0 } else { 0.0 }
	} else if (max_channel - green).abs() <= f32::EPSILON {
		(blue - red) / delta + 2.0
	} else {
		(red - green) / delta + 4.0
	};

	hue /= 6.0;

	(hue, saturation, lightness)
}

pub(super) fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> Rgb {
	let hue = hue.clamp(0.0, 1.0);
	let saturation = saturation.clamp(0.0, 1.0);
	let lightness = lightness.clamp(0.0, 1.0);

	if saturation <= 0.0 {
		let gray = (lightness * 255.0).round().clamp(0.0, 255.0) as u8;

		return Rgb::new(gray, gray, gray);
	}

	let q = if lightness < 0.5 {
		lightness * (1.0 + saturation)
	} else {
		lightness + saturation - lightness * saturation
	};
	let p = 2.0 * lightness - q;
	let red = hue_to_rgb(p, q, hue + 1.0 / 3.0);
	let green = hue_to_rgb(p, q, hue);
	let blue = hue_to_rgb(p, q, hue - 1.0 / 3.0);

	Rgb::new(
		(red * 255.0).round().clamp(0.0, 255.0) as u8,
		(green * 255.0).round().clamp(0.0, 255.0) as u8,
		(blue * 255.0).round().clamp(0.0, 255.0) as u8,
	)
}

pub(super) fn hue_to_rgb(p: f32, q: f32, hue: f32) -> f32 {
	let normalized_hue = hue.rem_euclid(1.0);

	if normalized_hue < 1.0 / 6.0 {
		return p + (q - p) * 6.0 * normalized_hue;
	}
	if normalized_hue < 1.0 / 2.0 {
		return q;
	}
	if normalized_hue < 2.0 / 3.0 {
		return p + (q - p) * (2.0 / 3.0 - normalized_hue) * 6.0;
	}

	p
}

pub(super) fn effective_hud_theme(mode: ThemeMode, window_theme: Option<Theme>) -> HudTheme {
	match mode {
		ThemeMode::System => match window_theme.unwrap_or(Theme::Dark) {
			Theme::Dark => HudTheme::Dark,
			Theme::Light => HudTheme::Light,
		},
		ThemeMode::Dark => HudTheme::Dark,
		ThemeMode::Light => HudTheme::Light,
	}
}

pub(super) fn live_hud_coordinate_text_width(min_value: i32, max_value: i32) -> usize {
	min_value.to_string().len().max(max_value.to_string().len()).max(1)
}

pub(super) fn format_live_hud_position_text(monitor: MonitorRect, cursor: GlobalPoint) -> String {
	let max_x = monitor.origin.x.saturating_add_unsigned(monitor.width.saturating_sub(1));
	let max_y = monitor.origin.y.saturating_add_unsigned(monitor.height.saturating_sub(1));
	let x_width = live_hud_coordinate_text_width(monitor.origin.x, max_x);
	let y_width = live_hud_coordinate_text_width(monitor.origin.y, max_y);

	format!("x={:>x_width$}, y={:>y_width$}", cursor.x, cursor.y)
}

pub(super) fn format_live_hud_rgb_text(rgb: Option<Rgb>) -> (String, String) {
	match rgb {
		Some(rgb) => (rgb.hex_upper(), format!("RGB({:>3}, {:>3}, {:>3})", rgb.r, rgb.g, rgb.b)),
		None => (String::from("#??????"), String::from("RGB(???, ???, ???)")),
	}
}

pub(super) fn stable_live_loupe_side_px(state: &OverlayState) -> u32 {
	state.loupe_patch_side_px.max(1)
}

pub(super) fn stable_live_loupe_side_points(state: &OverlayState, cell: f32) -> f32 {
	(stable_live_loupe_side_px(state) as f32) * cell
}

pub(super) fn stable_live_loupe_window_inner_size_points(side_px: u32) -> (u32, u32) {
	let side_points = side_px.max(1).saturating_mul(10);

	(side_points.saturating_add(22), side_points.saturating_add(22))
}

pub(super) fn hud_body_fill_srgba8(theme: HudTheme, opaque: bool) -> [u8; 4] {
	let mut color = if matches!(theme, HudTheme::Light) {
		HUD_PILL_BODY_FILL_LIGHT_SRGBA8
	} else {
		HUD_PILL_BODY_FILL_DARK_SRGBA8
	};

	if opaque {
		color[3] = 255;
	}

	color
}

pub(super) fn hud_blur_tint_alpha(theme: HudTheme) -> f32 {
	if matches!(theme, HudTheme::Light) {
		HUD_PILL_BLUR_TINT_ALPHA_LIGHT
	} else {
		HUD_PILL_BLUR_TINT_ALPHA_DARK
	}
}
