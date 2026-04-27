use std::fs;
use std::path::PathBuf;
use std::process;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use serde::Deserialize;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;

/// Schema marker emitted by Rust-side telemetry events.
pub const RUST_TELEMETRY_SCHEMA: &str = "rsnap.rust.telemetry/1";

/// Build metadata logged during application startup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupBuildInfo {
	/// The full Git commit hash embedded at build time when available.
	pub git_commit: &'static str,
	/// The crate version declared in `Cargo.toml`.
	pub version: &'static str,
}

#[derive(Deserialize)]
struct LauncherSettingsFile {
	log_filter: Option<String>,
}

/// Returns the build metadata that should be logged during app startup.
pub fn startup_build_info() -> StartupBuildInfo {
	StartupBuildInfo {
		version: env!("CARGO_PKG_VERSION"),
		git_commit: option_env!("RSNAP_BUILD_GIT_COMMIT").unwrap_or("unknown"),
	}
}

/// Returns the process-scoped telemetry run identifier.
pub fn telemetry_run_id() -> &'static str {
	static RUN_ID: OnceLock<String> = OnceLock::new();

	RUN_ID.get_or_init(|| {
		let started_at_milliseconds =
			SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_millis());

		format!("{}-{started_at_milliseconds}", process::id())
	})
}

/// Initializes file logging when the settings and filesystem allow it.
pub fn init_logging() -> Option<WorkerGuard> {
	let filter = default_log_filter();
	let Some(log_dir) = resolve_log_dir() else {
		init_console_logging(filter);

		return None;
	};

	if let Err(err) = fs::create_dir_all(&log_dir) {
		eprintln!("Failed to create log directory {log_dir:?}: {err}");

		init_console_logging(filter);

		return None;
	}

	let appender = match RollingFileAppender::builder()
		.rotation(Rotation::DAILY)
		.filename_prefix("rsnap")
		.filename_suffix("log")
		.max_log_files(15)
		.build(&log_dir)
	{
		Ok(appender) => appender,
		Err(err) => {
			eprintln!("Failed to initialize rolling file appender: {err}");

			init_console_logging(filter);

			return None;
		},
	};
	let (writer, guard) = tracing_appender::non_blocking(appender);

	tracing_subscriber::fmt().with_writer(writer).with_env_filter(filter).with_ansi(false).init();

	tracing::info!(
		schema = RUST_TELEMETRY_SCHEMA,
		run_id = telemetry_run_id(),
		op = "logging.file_initialized",
		log_dir = %log_dir.display(),
		"File logging initialized."
	);

	Some(guard)
}

fn init_console_logging(filter: EnvFilter) {
	tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn default_log_filter() -> EnvFilter {
	EnvFilter::try_from_default_env()
		.or_else(|_| load_log_filter_from_settings_file().ok_or(()))
		.unwrap_or_else(|_| EnvFilter::new("warn,rsnap=info"))
}

fn resolve_log_dir() -> Option<PathBuf> {
	ProjectDirs::from("ink", "hack", "rsnap").map(|dirs| dirs.data_dir().join("logs"))
}

fn load_log_filter_from_settings_file() -> Option<EnvFilter> {
	let path = launcher_settings_path()?;
	let contents = fs::read_to_string(path).ok()?;
	let settings = match toml::from_str::<LauncherSettingsFile>(&contents) {
		Ok(settings) => settings,
		Err(err) => {
			eprintln!("Invalid launcher settings file: {err}");

			return None;
		},
	};
	let filter = settings.log_filter.as_deref()?.trim();

	if filter.is_empty() {
		return None;
	}

	match filter.parse::<EnvFilter>() {
		Ok(filter) => Some(filter),
		Err(err) => {
			eprintln!("Invalid log_filter in settings: {err}");

			None
		},
	}
}

fn launcher_settings_path() -> Option<PathBuf> {
	let dirs = ProjectDirs::from("ink", "hack", "rsnap")?;

	Some(dirs.config_dir().join("settings.toml"))
}

#[cfg(test)]
mod tests {
	use crate::startup;

	#[test]
	fn startup_build_info_includes_version_and_git_commit() {
		let info = startup::startup_build_info();

		assert!(!info.version.is_empty());
		assert!(!info.git_commit.is_empty());
	}
}
