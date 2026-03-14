use std::fs::{self, File};
use std::io::{self, Error, ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use directories::{ProjectDirs, UserDirs};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use serde::{Deserialize, Serialize};

use rsnap_overlay::{OutputNaming, ThemeMode, ToolbarPlacement, WindowCaptureAlphaMode};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AltActivationMode {
	#[default]
	Hold,
	Toggle,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LoupeSampleSize {
	Small,
	#[default]
	Medium,
	Large,
}
impl LoupeSampleSize {
	#[must_use]
	pub const fn side_px(self) -> u32 {
		match self {
			Self::Small => 15,
			Self::Medium => 21,
			Self::Large => 31,
		}
	}

	#[must_use]
	pub const fn sanitize(self) -> Self {
		match self {
			Self::Small => Self::Small,
			Self::Medium => Self::Medium,
			Self::Large => Self::Large,
		}
	}
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct AppSettings {
	#[serde(default)]
	pub show_alt_hint_keycap: bool,
	#[serde(default)]
	pub hud_glass_enabled: bool,
	#[serde(default = "default_capture_hotkey")]
	pub capture_hotkey: String,
	#[serde(default = "default_hud_opacity")]
	pub hud_opacity: f32,
	#[serde(default = "default_hud_blur")]
	pub hud_blur: f32,
	#[serde(default = "default_hud_tint")]
	pub hud_tint: f32,
	#[serde(default = "default_hud_tint_hue")]
	pub hud_tint_hue: f32,
	#[serde(default)]
	pub alt_activation: AltActivationMode,
	#[serde(default = "default_selection_particles")]
	pub selection_particles: bool,
	#[serde(default = "default_selection_flow_stroke_width_px")]
	pub selection_flow_stroke_width_px: f32,
	pub log_filter: Option<String>,
	#[serde(default = "default_output_dir")]
	pub output_dir: PathBuf,
	#[serde(default = "default_output_filename_prefix")]
	pub output_filename_prefix: String,
	#[serde(default)]
	pub output_naming: OutputNaming,
	#[serde(default)]
	pub window_capture_alpha_mode: WindowCaptureAlphaMode,
	#[serde(default)]
	pub toolbar_placement: ToolbarPlacement,
	#[serde(default)]
	pub loupe_sample_size: LoupeSampleSize,
	#[serde(default)]
	pub theme_mode: ThemeMode,
}
impl AppSettings {
	#[must_use]
	pub fn load() -> Self {
		let Some(path) = Self::path() else {
			return Self::default();
		};
		let Ok(bytes) = fs::read(&path) else {
			return Self::default();
		};
		let Ok(contents) = std::str::from_utf8(&bytes) else {
			return Self::default();
		};
		let mut settings: Self = toml::from_str(contents).unwrap_or_default();

		settings.capture_hotkey = sanitize_capture_hotkey(&settings.capture_hotkey)
			.unwrap_or_else(default_capture_hotkey);
		settings.hud_opacity = settings.hud_opacity.clamp(0.0, 1.0);
		settings.hud_blur = settings.hud_blur.clamp(0.0, 1.0);
		settings.hud_tint = settings.hud_tint.clamp(0.0, 1.0);
		settings.hud_tint_hue = settings.hud_tint_hue.clamp(0.0, 1.0);
		settings.selection_flow_stroke_width_px =
			settings.selection_flow_stroke_width_px.clamp(1.0, 8.0);
		settings.loupe_sample_size = settings.loupe_sample_size.sanitize();
		settings.output_dir = sanitize_output_dir(&settings.output_dir);
		settings.output_filename_prefix =
			sanitize_output_filename_prefix(&settings.output_filename_prefix);

		settings
	}

	pub fn save(&self) -> io::Result<()> {
		let Some(path) = Self::path() else {
			return Ok(());
		};
		let Some(dir) = path.parent() else {
			return Ok(());
		};

		fs::create_dir_all(dir)?;

		let content =
			toml::to_string_pretty(self).map_err(|err| Error::new(ErrorKind::InvalidData, err))?;

		write_atomic(&path, content.as_bytes())?;

		Ok(())
	}

	#[must_use]
	fn path() -> Option<PathBuf> {
		let dirs = ProjectDirs::from("ink", "hack", "rsnap")?;

		Some(dirs.config_dir().join("settings.toml"))
	}

	#[must_use]
	pub fn capture_hotkey(&self) -> HotKey {
		parse_capture_hotkey(&self.capture_hotkey)
			.unwrap_or_else(|| HotKey::new(Some(Modifiers::ALT), Code::KeyX))
	}
}

impl Default for AppSettings {
	fn default() -> Self {
		Self {
			show_alt_hint_keycap: true,
			hud_glass_enabled: true,
			capture_hotkey: default_capture_hotkey(),
			hud_opacity: default_hud_opacity(),
			hud_blur: default_hud_blur(),
			hud_tint: default_hud_tint(),
			hud_tint_hue: default_hud_tint_hue(),
			alt_activation: AltActivationMode::default(),
			selection_particles: default_selection_particles(),
			selection_flow_stroke_width_px: default_selection_flow_stroke_width_px(),
			log_filter: None,
			output_dir: default_output_dir(),
			output_filename_prefix: default_output_filename_prefix(),
			output_naming: OutputNaming::default(),
			window_capture_alpha_mode: WindowCaptureAlphaMode::default(),
			toolbar_placement: ToolbarPlacement::Bottom,
			loupe_sample_size: LoupeSampleSize::default(),
			theme_mode: ThemeMode::System,
		}
	}
}

pub(crate) fn sanitize_output_filename_prefix(raw: &str) -> String {
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

	if sanitized.is_empty() { default_output_filename_prefix() } else { sanitized.to_owned() }
}

fn default_hud_opacity() -> f32 {
	0.5
}

fn default_hud_blur() -> f32 {
	0.5
}

fn default_hud_tint() -> f32 {
	0.5
}

fn default_hud_tint_hue() -> f32 {
	215.0 / 360.0
}

fn default_selection_particles() -> bool {
	true
}

fn default_output_dir() -> PathBuf {
	let Some(user_dirs) = UserDirs::new() else {
		return PathBuf::from(".");
	};

	user_dirs
		.desktop_dir()
		.map(Path::to_path_buf)
		.unwrap_or_else(|| user_dirs.home_dir().to_path_buf())
}

fn default_output_filename_prefix() -> String {
	String::from("rsnap")
}

fn sanitize_output_dir(path: &Path) -> PathBuf {
	if path.as_os_str().is_empty() {
		return default_output_dir();
	}

	path.to_path_buf()
}

fn default_capture_hotkey() -> String {
	HotKey::new(Some(Modifiers::ALT), Code::KeyX).to_string()
}

fn parse_capture_hotkey(raw: &str) -> Option<HotKey> {
	let mut modifiers = Modifiers::empty();
	let mut has_required_modifier = false;
	let mut has_keycode = false;
	let mut code = None;

	for part in raw.split('+').map(str::trim).filter(|part| !part.is_empty()) {
		let token = part;

		match token.to_ascii_lowercase().as_str() {
			"alt" | "option" => {
				modifiers.insert(Modifiers::ALT);

				has_required_modifier = true;
			},
			"ctrl" | "control" => {
				modifiers.insert(Modifiers::CONTROL);

				has_required_modifier = true;
			},
			"super" | "meta" | "cmd" | "win" | "command" => {
				modifiers.insert(Modifiers::SUPER);

				has_required_modifier = true;
			},
			"shift" => {
				modifiers.insert(Modifiers::SHIFT);
			},
			other => {
				if !other.chars().all(|ch| ch.is_ascii_alphanumeric()) {
					return None;
				}
				if has_keycode {
					return None;
				}

				has_keycode = true;
				code = Code::from_str(token).ok();
			},
		}
	}

	let code = code?;

	if !has_required_modifier {
		return None;
	}

	Some(HotKey::new(Some(modifiers), code))
}

fn sanitize_capture_hotkey(raw: &str) -> Option<String> {
	parse_capture_hotkey(raw).map(|key| key.to_string())
}

fn default_selection_flow_stroke_width_px() -> f32 {
	2.4
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
	let tmp = path.with_extension("toml.tmp");
	let mut file = File::create(&tmp)?;

	file.write_all(bytes)?;

	fs::rename(&tmp, path)?;

	Ok(())
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use crate::settings::{AltActivationMode, AppSettings, LoupeSampleSize};
	use rsnap_overlay::{OutputNaming, ThemeMode, ToolbarPlacement, WindowCaptureAlphaMode};

	#[test]
	fn toml_roundtrip() {
		let settings = AppSettings::default();
		let content = toml::to_string_pretty(&settings).unwrap();
		let deserialized: AppSettings = toml::from_str(&content).unwrap();

		assert_eq!(settings, deserialized);
	}

	#[test]
	fn toml_parses_known_values() {
		let input = r#"
	show_alt_hint_keycap = true
	hud_glass_enabled = true
	capture_hotkey = "alt+KeyX"
	hud_opacity = 0.5
	hud_blur = 0.15
	hud_tint = 0.25
	hud_tint_hue = 0.4
	alt_activation = "toggle"
	selection_particles = true
	selection_flow_stroke_width_px = 2.4
	output_dir = "/tmp/rsnap-output"
	output_filename_prefix = "shot"
	output_naming = "sequence"
	window_capture_alpha_mode = "matte_dark"
	toolbar_placement = "top"
	loupe_sample_size = "large"
	theme_mode = "dark"
	"#;
		let settings: AppSettings = toml::from_str(input).unwrap();

		assert_eq!(settings.alt_activation, AltActivationMode::Toggle);
		assert!(settings.selection_particles);
		assert_eq!(settings.selection_flow_stroke_width_px, 2.4);
		assert_eq!(settings.output_dir, PathBuf::from("/tmp/rsnap-output"));
		assert_eq!(settings.output_filename_prefix, "shot");
		assert_eq!(settings.output_naming, OutputNaming::Sequence);
		assert_eq!(settings.window_capture_alpha_mode, WindowCaptureAlphaMode::MatteDark);
		assert_eq!(settings.toolbar_placement, ToolbarPlacement::Top);
		assert_eq!(settings.loupe_sample_size, LoupeSampleSize::Large);
		assert_eq!(settings.theme_mode, ThemeMode::Dark);
	}

	#[test]
	fn toml_ignores_legacy_tray_icon_keys() {
		let baseline: AppSettings = toml::from_str("").unwrap();
		let tray_icon_inverted: AppSettings = toml::from_str("tray_icon_inverted = true").unwrap();
		let tray_icon_filled: AppSettings = toml::from_str("tray_icon_filled = true").unwrap();

		assert_eq!(tray_icon_inverted, baseline);
		assert_eq!(tray_icon_filled, baseline);
	}

	#[test]
	fn window_capture_alpha_mode_preserve_alias_maps_to_background() {
		let input = r#"
	window_capture_alpha_mode = "preserve"
	"#;
		let settings: AppSettings = toml::from_str(input).unwrap();

		assert_eq!(settings.window_capture_alpha_mode, WindowCaptureAlphaMode::Background);
	}

	#[test]
	fn capture_hotkey_falls_back_to_default_on_invalid() {
		let input = r#"
	capture_hotkey = "bad_hotkey"
	"#;
		let settings: AppSettings =
			toml::from_str(input).unwrap_or_else(|_| AppSettings::default());
		let loaded = super::sanitize_capture_hotkey(&settings.capture_hotkey)
			.unwrap_or_else(super::default_capture_hotkey);

		assert_eq!(loaded, AppSettings::default().capture_hotkey);
	}

	#[test]
	fn output_filename_prefix_sanitizes_invalid_chars() {
		let sanitized = super::sanitize_output_filename_prefix("  rsnap:/demo?  ");

		assert_eq!(sanitized, "rsnap__demo");
	}
}
