//! Library surface for `rsnap` benchmark and test support.

#![allow(unused_crate_dependencies)]

pub mod settings_window;

mod app;
mod icon;
#[cfg(target_os = "macos")]
mod permissions_macos;
mod settings;
mod startup;

pub use self::{
	app::run,
	startup::{StartupBuildInfo, init_logging, startup_build_info},
};
