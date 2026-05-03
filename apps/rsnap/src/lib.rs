//! Library surface for the Rsnap native host crate.

#![allow(unused_crate_dependencies)]

pub mod runtime {
	//! Public runtime entry points for the desktop host crate.

	#[cfg(target_os = "macos")]
	pub use crate::native_launcher_macos::run;
	pub use crate::startup::{
		RUST_TELEMETRY_SCHEMA, StartupBuildInfo, init_logging, startup_build_info, telemetry_run_id,
	};
	#[cfg(not(target_os = "macos"))]
	pub use crate::unsupported_platform::run;
}

#[cfg(target_os = "macos")]
mod native_launcher_macos;
mod startup;
#[cfg(not(target_os = "macos"))]
mod unsupported_platform;

#[cfg(target_os = "macos")]
pub use self::native_launcher_macos::run;
pub use self::runtime::{
	RUST_TELEMETRY_SCHEMA, StartupBuildInfo, init_logging, startup_build_info, telemetry_run_id,
};
#[cfg(not(target_os = "macos"))]
pub use self::unsupported_platform::run;
