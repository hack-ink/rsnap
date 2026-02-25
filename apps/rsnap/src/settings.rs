use std::fs;
use std::io::{self};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use rsnap_overlay::ThemeMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
		let mut settings = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
			let legacy: LegacyAppSettings =
				serde_json::from_slice(&bytes).unwrap_or_else(|_| LegacyAppSettings::default());

			legacy.into_current()
		});

		settings.hud_opacity = settings.hud_opacity.clamp(0.0, 1.0);
		settings.hud_blur = settings.hud_blur.clamp(0.0, 1.0);
		settings.hud_tint = settings.hud_tint.clamp(0.0, 1.0);

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

		let json = serde_json::to_vec_pretty(self)
			.map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

		write_atomic(&path, &json)?;

		Ok(())
	}

	#[must_use]
	fn path() -> Option<PathBuf> {
		let dirs = ProjectDirs::from("ink", "hack", "rsnap")?;

		Some(dirs.config_dir().join("settings.json"))
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
			theme_mode: ThemeMode::System,
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyAppSettings {
	#[serde(default)]
	show_alt_hint_keycap: bool,
	#[serde(default)]
	show_hud_blur: bool,
	#[serde(default)]
	hud_opaque: bool,
	#[serde(default)]
	hud_fog_enabled: bool,
	#[serde(default)]
	hud_fog_amount: f32,
	#[serde(default)]
	hud_milk_enabled: bool,
	#[serde(default)]
	hud_milk_amount: f32,
	#[serde(default)]
	theme_mode: ThemeMode,
}
impl LegacyAppSettings {
	fn into_current(self) -> AppSettings {
		let glass = !self.hud_opaque;

		AppSettings {
			show_alt_hint_keycap: self.show_alt_hint_keycap,
			hud_glass_enabled: glass,
			hud_opacity: if glass { default_hud_opacity() } else { 1.0 },
			hud_blur: if self.show_hud_blur && self.hud_fog_enabled {
				self.hud_fog_amount
			} else {
				0.0
			},
			hud_tint: if self.hud_milk_enabled { self.hud_milk_amount } else { 0.0 },
			theme_mode: self.theme_mode,
		}
	}
}

impl Default for LegacyAppSettings {
	fn default() -> Self {
		Self {
			show_alt_hint_keycap: true,
			show_hud_blur: true,
			hud_opaque: false,
			hud_fog_enabled: true,
			hud_fog_amount: default_hud_blur(),
			hud_milk_enabled: false,
			hud_milk_amount: default_hud_tint(),
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

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
	let tmp = path.with_extension("json.tmp");

	fs::write(&tmp, bytes)?;
	fs::rename(&tmp, path)?;

	Ok(())
}
