use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use rsnap_overlay::{ThemeMode, ToolbarPlacement};

#[derive(Clone, Copy, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AltActivationMode {
	#[default]
	Hold,
	Toggle,
}

#[derive(Clone, Copy, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoupeSampleSize {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppSettings {
	#[serde(default)]
	pub show_alt_hint_keycap: bool,
	#[serde(default)]
	pub hud_glass_enabled: bool,
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
	pub log_filter: Option<String>,
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

		settings.hud_opacity = settings.hud_opacity.clamp(0.0, 1.0);
		settings.hud_blur = settings.hud_blur.clamp(0.0, 1.0);
		settings.hud_tint = settings.hud_tint.clamp(0.0, 1.0);
		settings.hud_tint_hue = settings.hud_tint_hue.clamp(0.0, 1.0);
		settings.loupe_sample_size = settings.loupe_sample_size.sanitize();

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

		let content = toml::to_string_pretty(self)
			.map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

		write_atomic(&path, content.as_bytes())?;

		Ok(())
	}

	#[must_use]
	fn path() -> Option<PathBuf> {
		let dirs = ProjectDirs::from("ink", "hack", "rsnap")?;

		Some(dirs.config_dir().join("settings.toml"))
	}
}

impl Default for AppSettings {
	fn default() -> Self {
		Self {
			show_alt_hint_keycap: true,
			hud_glass_enabled: true,
			hud_opacity: default_hud_opacity(),
			hud_blur: default_hud_blur(),
			hud_tint: default_hud_tint(),
			hud_tint_hue: default_hud_tint_hue(),
			alt_activation: AltActivationMode::default(),
			selection_particles: default_selection_particles(),
			log_filter: None,
			toolbar_placement: ToolbarPlacement::Bottom,
			loupe_sample_size: LoupeSampleSize::default(),
			theme_mode: ThemeMode::System,
		}
	}
}

fn default_hud_opacity() -> f32 {
	0.75
}

fn default_hud_blur() -> f32 {
	0.25
}

fn default_hud_tint() -> f32 {
	0.0
}

fn default_hud_tint_hue() -> f32 {
	0.585
}

fn default_selection_particles() -> bool {
	true
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
	let tmp = path.with_extension("toml.tmp");
	let mut file = fs::File::create(&tmp)?;

	file.write_all(bytes)?;

	fs::rename(&tmp, path)?;

	Ok(())
}

#[cfg(test)]
mod tests {
	use crate::settings::{AltActivationMode, AppSettings, LoupeSampleSize};
	use rsnap_overlay::{ThemeMode, ToolbarPlacement};

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
	hud_opacity = 0.5
	hud_blur = 0.15
	hud_tint = 0.25
	hud_tint_hue = 0.4
	alt_activation = "toggle"
	selection_particles = true
	toolbar_placement = "top"
	loupe_sample_size = "large"
	theme_mode = "dark"
	"#;
		let settings: AppSettings = toml::from_str(input).unwrap();

		assert_eq!(settings.alt_activation, AltActivationMode::Toggle);
		assert!(settings.selection_particles);
		assert_eq!(settings.toolbar_placement, ToolbarPlacement::Top);
		assert_eq!(settings.loupe_sample_size, LoupeSampleSize::Large);
		assert_eq!(settings.theme_mode, ThemeMode::Dark);
	}
}
