use std::{
	env,
	path::{Path, PathBuf},
};

const TAURI_CONFIG_DEV_MERGE_JSON: &str = r#"{"bundle":{"externalBin":[]}}"#;

fn main() {
	let target_triple = env::var("TARGET").ok();

	if let Some(target_triple) = &target_triple {
		println!("cargo:rustc-env=RSNAP_TARGET_TRIPLE={target_triple}");
	}

	let profile = env::var("PROFILE").unwrap_or_else(|_| String::from("debug"));
	let is_release = profile == "release";

	if env::var_os("TAURI_CONFIG").is_none()
		&& let Some(target_triple) = &target_triple
	{
		let manifest_dir = env::var("CARGO_MANIFEST_DIR")
			.ok()
			.map(PathBuf::from)
			.unwrap_or_else(|| PathBuf::from("."));
		let expected_sidecar = expected_sidecar_path(&manifest_dir, target_triple);

		if is_release {
			if !expected_sidecar.exists() {
				panic!(
					"Missing bundled sidecar at {}; run: TARGET_TRIPLE={target_triple} cargo make stage-overlay-sidecar",
					expected_sidecar.display()
				);
			}
		} else if !expected_sidecar.exists() {
			unsafe {
				env::set_var("TAURI_CONFIG", TAURI_CONFIG_DEV_MERGE_JSON);
			}

			println!(
				"cargo:warning=rsnap-overlay sidecar not staged ({}); disabling bundle.externalBin for dev builds",
				expected_sidecar.display()
			);
		}
	}

	tauri_build::build();
}

fn expected_sidecar_path(manifest_dir: &Path, target_triple: &str) -> PathBuf {
	let is_windows_target = target_triple.contains("windows");
	let suffix = if is_windows_target { ".exe" } else { "" };

	manifest_dir.join("bin").join(format!("rsnap-overlay-{target_triple}{suffix}"))
}
