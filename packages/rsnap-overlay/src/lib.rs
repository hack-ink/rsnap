//! Legacy-free Rust capture helpers still being migrated into durable owners.
//!
//! This crate intentionally exposes only the remaining Rust-owned transition helpers
//! used by native-host FFI and deterministic performance checks. The retired Rust UI
//! overlay runtime is no longer part of the public or compiled crate surface.

#[cfg(target_os = "macos")]
pub mod host_live_sampling_macos;

#[cfg(target_os = "macos")]
mod live_frame_stream_macos;
#[cfg(target_os = "macos")]
mod macos_color;
mod state;

/// Returns the `rsnap-overlay` crate version.
pub fn overlay_version() -> &'static str {
	env!("CARGO_PKG_VERSION")
}
