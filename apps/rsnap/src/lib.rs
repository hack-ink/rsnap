//! Library surface for the `rsnap` native host crate.

#![allow(unused_crate_dependencies)]

#[cfg(target_os = "macos")]
pub mod host_macos;
pub mod runtime {
	//! Public runtime entry points for the desktop host crate.

	pub use crate::app::run;
	pub use crate::startup::{StartupBuildInfo, init_logging, startup_build_info};
}
pub mod settings_window;

mod app;
mod icon;
#[cfg(target_os = "macos")]
mod permissions_macos;
mod settings;
mod startup;

pub use self::runtime::{StartupBuildInfo, init_logging, run, startup_build_info};
