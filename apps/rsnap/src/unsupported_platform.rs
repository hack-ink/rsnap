use color_eyre::eyre::{self, Result};

/// Returns an explicit unsupported-platform error for non-macOS builds.
pub fn run() -> Result<()> {
	eyre::bail!("rsnap currently ships only the native macOS host")
}
