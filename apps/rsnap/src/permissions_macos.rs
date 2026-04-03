use std::process::Command;

use color_eyre::eyre;
use color_eyre::eyre::{Result, WrapErr};

pub(crate) const SCREEN_RECORDING_SETTINGS_PATH: &str =
	"System Settings > Privacy & Security > Screen Recording";
pub(crate) const ACCESSIBILITY_SETTINGS_PATH: &str =
	"System Settings > Privacy & Security > Accessibility";
pub(crate) const INPUT_MONITORING_SETTINGS_PATH: &str =
	"System Settings > Privacy & Security > Input Monitoring";

const SCREEN_RECORDING_SETTINGS_URL: &str =
	"x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
const ACCESSIBILITY_SETTINGS_URL: &str =
	"x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const INPUT_MONITORING_SETTINGS_URL: &str =
	"x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";

pub(crate) fn screen_recording_access_granted() -> bool {
	unsafe { CGPreflightScreenCaptureAccess() }
}

pub(crate) fn open_screen_recording_settings() -> Result<()> {
	open_settings_url(SCREEN_RECORDING_SETTINGS_URL, "Screen Recording settings")
}

pub(crate) fn accessibility_access_granted() -> bool {
	unsafe { AXIsProcessTrusted() }
}

pub(crate) fn open_accessibility_settings() -> Result<()> {
	open_settings_url(ACCESSIBILITY_SETTINGS_URL, "Accessibility settings")
}

pub(crate) fn input_monitoring_access_granted() -> bool {
	unsafe { CGPreflightListenEventAccess() }
}

pub(crate) fn open_input_monitoring_settings() -> Result<()> {
	open_settings_url(INPUT_MONITORING_SETTINGS_URL, "Input Monitoring settings")
}

fn open_settings_url(settings_url: &str, description: &str) -> Result<()> {
	let status = Command::new("open")
		.arg(settings_url)
		.status()
		.wrap_err_with(|| format!("launch macOS {description}"))?;

	if status.success() {
		Ok(())
	} else {
		Err(eyre::eyre!("`open` exited with status {status} while opening macOS {description}"))
	}
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
	fn CGPreflightScreenCaptureAccess() -> bool;
	fn CGPreflightListenEventAccess() -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
	fn AXIsProcessTrusted() -> bool;
}
