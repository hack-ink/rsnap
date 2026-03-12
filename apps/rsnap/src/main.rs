//! Desktop binary entrypoint for the `rsnap` application.

mod app;
mod icon;
mod settings;
mod settings_window;
mod startup;

use color_eyre::eyre::Result;

fn main() -> Result<()> {
	color_eyre::install()?;

	let _guard = startup::init_logging();
	let build_info = startup::startup_build_info();

	tracing::info!(
		version = build_info.version,
		git_commit = build_info.git_commit,
		"Starting rsnap."
	);

	app::run()
}
