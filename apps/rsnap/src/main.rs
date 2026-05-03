//! Desktop binary entrypoint for the Rsnap application.

#![allow(unused_crate_dependencies)]

use color_eyre::eyre::Result;

fn main() -> Result<()> {
	color_eyre::install()?;

	let _guard = rsnap::init_logging();
	let build_info = rsnap::startup_build_info();

	tracing::info!(
		schema = rsnap::RUST_TELEMETRY_SCHEMA,
		run_id = rsnap::telemetry_run_id(),
		op = "rsnap.starting",
		version = build_info.version,
		git_commit = build_info.git_commit,
		"Starting Rsnap."
	);

	rsnap::run()
}
