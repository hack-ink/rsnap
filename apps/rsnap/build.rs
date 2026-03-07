use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
	println!("cargo:rerun-if-changed=build.rs");

	let manifest_dir =
		env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
	let repo_root = resolve_repo_root(&manifest_dir).unwrap_or_else(|| manifest_dir.clone());

	emit_git_rerun_hints(&repo_root);

	let metadata = read_git_metadata(&repo_root);

	emit_env("RSNAP_BUILD_GIT_COMMIT", &metadata.commit);
	emit_env("RSNAP_BUILD_GIT_SHORT_COMMIT", &metadata.short_commit);
	emit_env("RSNAP_BUILD_GIT_DIRTY", metadata.dirty.as_str());
	emit_env("RSNAP_BUILD_GIT_SOURCE", &metadata.source);
	emit_env(
		"RSNAP_BUILD_PROFILE",
		&env::var("PROFILE").unwrap_or_else(|_| String::from("unknown")),
	);
	emit_env("RSNAP_BUILD_TARGET", &env::var("TARGET").unwrap_or_else(|_| String::from("unknown")));
	emit_env(
		"RSNAP_BUILD_UNIX_EPOCH_SECS",
		&SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|duration| duration.as_secs().to_string())
			.unwrap_or_else(|_| String::from("0")),
	);
}

fn emit_env(name: &str, value: &str) {
	println!("cargo:rustc-env={name}={value}");
}

fn resolve_repo_root(manifest_dir: &Path) -> Option<PathBuf> {
	run_git(manifest_dir, ["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}

fn emit_git_rerun_hints(repo_root: &Path) {
	for git_path in ["HEAD", "index", "packed-refs"] {
		if let Some(path) = resolve_git_path(repo_root, git_path) {
			println!("cargo:rerun-if-changed={}", path.display());
		}
	}

	if let Some(symbolic_ref) = run_git(repo_root, ["symbolic-ref", "-q", "HEAD"])
		&& let Some(path) = resolve_git_path(repo_root, symbolic_ref.trim())
	{
		println!("cargo:rerun-if-changed={}", path.display());
	}
}

fn resolve_git_path(repo_root: &Path, git_path: &str) -> Option<PathBuf> {
	let resolved = run_git(repo_root, ["rev-parse", "--git-path", git_path])?;
	let path = PathBuf::from(resolved.trim());

	if path.is_absolute() { Some(path) } else { Some(repo_root.join(path)) }
}

fn run_git<const N: usize>(repo_root: &Path, args: [&str; N]) -> Option<String> {
	let output = Command::new("git").args(args).current_dir(repo_root).output().ok()?;

	if !output.status.success() {
		return None;
	}

	let stdout = String::from_utf8(output.stdout).ok()?;
	let trimmed = stdout.trim();

	if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
}

struct GitMetadata {
	commit: String,
	short_commit: String,
	dirty: String,
	source: String,
}

fn read_git_metadata(repo_root: &Path) -> GitMetadata {
	let commit = run_git(repo_root, ["rev-parse", "HEAD"]);
	let short_commit = run_git(repo_root, ["rev-parse", "--short=12", "HEAD"]);
	let dirty = Command::new("git")
		.args(["status", "--porcelain", "--untracked-files=no"])
		.current_dir(repo_root)
		.output()
		.ok()
		.and_then(|output| {
			if output.status.success() {
				Some(if output.stdout.is_empty() {
					String::from("false")
				} else {
					String::from("true")
				})
			} else {
				None
			}
		});

	match (commit, short_commit, dirty) {
		(Some(commit), Some(short_commit), Some(dirty)) => {
			GitMetadata { commit, short_commit, dirty, source: String::from("git") }
		},
		(Some(commit), Some(short_commit), None) => GitMetadata {
			commit,
			short_commit,
			dirty: String::from("unknown"),
			source: String::from("git-no-dirty-state"),
		},
		_ => GitMetadata {
			commit: String::from("unknown"),
			short_commit: String::from("unknown"),
			dirty: String::from("unknown"),
			source: String::from("git-unavailable"),
		},
	}
}
