use color_eyre::eyre::{Result, bail};

/// Returns an explicit unsupported-platform error for non-macOS builds.
pub fn run() -> Result<()> {
	bail!("rsnap currently ships only the native macOS host")
}
