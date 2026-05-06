//! Crate startup orchestration: state init, window/tray, background tasks,
//! CLI install, server boot, logging, bridge.
//!
//! Layout matches the order the entry-point in `lib.rs` calls them in.

mod background;
mod bridge;
mod cli_install;
mod logging;
mod servers;
mod states;
mod window;

pub(crate) use background::start_background_tasks;
pub use bridge::{spawn_telegram_bridge, start_telegram_bridge};
#[cfg(test)]
pub use bridge::start_telegram_bridge_for_test;
pub(crate) use cli_install::install_bundled_cli;
pub(crate) use logging::setup_logging;
pub(crate) use servers::start_servers;
pub(crate) use states::init_states;
pub(crate) use window::{build_system_tray, setup_window_styling};
