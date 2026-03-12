use std::fs;
use std::path::PathBuf;

use directories::ProjectDirs;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;

use crate::settings::AppSettings;

/// Build metadata logged during application startup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupBuildInfo {
	/// The full Git commit hash embedded at build time when available.
	pub git_commit: &'static str,
	/// The crate version declared in `Cargo.toml`.
	pub version: &'static str,
}

pub(crate) fn startup_build_info() -> StartupBuildInfo {
	StartupBuildInfo {
		version: env!("CARGO_PKG_VERSION"),
		git_commit: option_env!("RSNAP_BUILD_GIT_COMMIT").unwrap_or("unknown"),
	}
}

pub(crate) fn init_logging() -> Option<WorkerGuard> {
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

	tracing::info!(log_dir = %log_dir.display(), "File logging initialized.");

	Some(guard)
}

fn init_console_logging(filter: EnvFilter) {
	tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn default_log_filter() -> EnvFilter {
	EnvFilter::try_from_default_env()
		.or_else(|_| load_log_filter_from_settings().ok_or(()))
		.unwrap_or_else(|_| EnvFilter::new("warn,rsnap=info"))
}

fn resolve_log_dir() -> Option<PathBuf> {
	ProjectDirs::from("ink", "hack", "rsnap").map(|dirs| dirs.data_dir().join("logs"))
}

fn load_log_filter_from_settings() -> Option<EnvFilter> {
	let settings = AppSettings::load();
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
