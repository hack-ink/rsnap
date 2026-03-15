//! Library surface for `rsnap` benchmark and test support.

mod app;
mod icon;
mod settings;
pub mod settings_window;
mod startup;

pub use app::run;
pub use startup::{StartupBuildInfo, init_logging, startup_build_info};
