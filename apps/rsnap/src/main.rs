//! Desktop binary entrypoint for the `rsnap` application.

use color_eyre::eyre::Result;

fn main() -> Result<()> {
	color_eyre::install()?;

	let _guard = rsnap::init_logging();
	let build_info = rsnap::startup_build_info();

	tracing::info!(
		version = build_info.version,
		git_commit = build_info.git_commit,
		"Starting rsnap."
	);

	rsnap::run()
}
