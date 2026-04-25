use std::path::{Path, PathBuf};
use std::process::Command;

use color_eyre::eyre::{self, Context as _, Result};

const APP_NAME: &str = "rsnap.app";

/// Launches the staged native macOS host bundle for the current worktree.
pub fn run() -> Result<()> {
	let worktree_root = worktree_root_from_manifest_dir(Path::new(env!("CARGO_MANIFEST_DIR")))?;
	let launcher_script = worktree_root.join("scripts/build_and_run.sh");

	if !launcher_script.is_file() {
		let stable_bundle = stable_bundle_path(&worktree_root)?;

		if stable_bundle.exists() {
			open_app_bundle(&stable_bundle)?;

			return Ok(());
		}

		eyre::bail!("native host launcher script is missing: {}", launcher_script.display());
	}

	let status = Command::new(&launcher_script)
		.arg("run")
		.current_dir(&worktree_root)
		.status()
		.with_context(|| {
			format!("failed to run native host launcher: {}", launcher_script.display())
		})?;

	if !status.success() {
		eyre::bail!("native host launcher exited with status {status}");
	}

	Ok(())
}

fn stable_bundle_path(worktree_root: &Path) -> Result<PathBuf> {
	let output = Command::new("git")
		.args(["rev-parse", "--git-common-dir"])
		.current_dir(worktree_root)
		.output()
		.context("failed to resolve git common dir for native host bundle")?;

	if !output.status.success() {
		eyre::bail!("git rev-parse --git-common-dir failed with status {}", output.status);
	}

	let common_git_dir = String::from_utf8(output.stdout)
		.context("git common dir output was not valid UTF-8")?
		.trim()
		.to_owned();
	let common_git_dir = PathBuf::from(common_git_dir);
	let common_root = common_git_dir
		.parent()
		.ok_or_else(|| eyre::eyre!("git common dir has no parent: {}", common_git_dir.display()))?;

	Ok(common_root.join(".native-host-dist").join(APP_NAME))
}

fn open_app_bundle(app_bundle: &Path) -> Result<()> {
	let status = Command::new("/usr/bin/open")
		.arg(app_bundle)
		.status()
		.with_context(|| format!("failed to open native host bundle: {}", app_bundle.display()))?;

	if !status.success() {
		eyre::bail!("open exited with status {status}");
	}

	Ok(())
}

fn worktree_root_from_manifest_dir(manifest_dir: &Path) -> Result<PathBuf> {
	let apps_dir = manifest_dir
		.parent()
		.ok_or_else(|| eyre::eyre!("manifest dir has no parent: {}", manifest_dir.display()))?;
	let worktree_root = apps_dir
		.parent()
		.ok_or_else(|| eyre::eyre!("apps dir has no parent: {}", apps_dir.display()))?;

	Ok(worktree_root.to_path_buf())
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use crate::native_launcher_macos;

	#[test]
	fn derives_worktree_root_from_apps_manifest_dir() {
		let manifest_dir = Path::new("/tmp/rsnap/.worktrees/native/apps/rsnap");
		let worktree_root = native_launcher_macos::worktree_root_from_manifest_dir(manifest_dir)
			.expect("worktree root should resolve");

		assert_eq!(worktree_root, Path::new("/tmp/rsnap/.worktrees/native"));
	}
}
