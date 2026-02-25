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
	pub show_hud_blur: bool,
	#[serde(default)]
	pub hud_opaque: bool,
	#[serde(default)]
	pub hud_fog_enabled: bool,
	#[serde(default = "default_hud_fog_amount")]
	pub hud_fog_amount: f32,
	#[serde(default)]
	pub hud_milk_enabled: bool,
	#[serde(default = "default_hud_milk_amount")]
	pub hud_milk_amount: f32,
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
		let mut settings = serde_json::from_slice(&bytes).unwrap_or_else(|_| Self::default());

		settings.hud_fog_amount = settings.hud_fog_amount.clamp(0.0, 1.0);
		settings.hud_milk_amount = settings.hud_milk_amount.clamp(0.0, 1.0);

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
			show_hud_blur: true,
			hud_opaque: false,
			hud_fog_enabled: true,
			hud_fog_amount: default_hud_fog_amount(),
			hud_milk_enabled: false,
			hud_milk_amount: default_hud_milk_amount(),
			theme_mode: ThemeMode::System,
		}
	}
}

fn default_hud_fog_amount() -> f32 {
	0.16
}

fn default_hud_milk_amount() -> f32 {
	0.22
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
	let tmp = path.with_extension("json.tmp");

	fs::write(&tmp, bytes)?;
	fs::rename(&tmp, path)?;

	Ok(())
}
