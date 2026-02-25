use std::fs;
use std::io::{self};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
	pub show_alt_hint_keycap: bool,
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

		serde_json::from_slice(&bytes).unwrap_or_else(|_| Self::default())
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
		Self { show_alt_hint_keycap: true }
	}
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
	let tmp = path.with_extension("json.tmp");

	fs::write(&tmp, bytes)?;
	fs::rename(&tmp, path)?;

	Ok(())
}
