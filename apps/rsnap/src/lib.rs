//! Library surface for `rsnap` benchmark and test support.

pub mod settings_window;

mod app;
mod icon;
mod settings;
mod startup;

pub use self::{
	app::run,
	startup::{StartupBuildInfo, init_logging, startup_build_info},
};
