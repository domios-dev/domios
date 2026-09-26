//! Crate startup orchestration: state init, window/tray, background tasks,
//! CLI install, server boot, logging, bridge.
//!
//! Layout matches the order the entry-point in `lib.rs` calls them in.

mod background;
mod bridge;
mod cli_install;
mod cli_shim;
mod conpty_host;
mod frontend_ready;
mod logging;
mod plan_restore;
mod progress;
mod rest_api;
mod restore_complete;
mod servers;
#[cfg(test)]
mod stall_tests;
mod states;
mod user_path;
mod window;

pub(crate) use background::start_background_tasks;
pub use bridge::{spawn_telegram_bridge, start_telegram_bridge, unmark_tab_connected_service};
pub(crate) use cli_install::{install_bundled_cli, refresh_bundled_cli};
pub(crate) use cli_shim::forward_to_cli_if_invoked;
pub(crate) use conpty_host::setup_conpty_host;
pub(crate) use frontend_ready::{
    arm_frontend_readiness, frontend_readiness_generation, mark_frontend_ready,
    wait_for_frontend_ready_until,
};
pub(crate) use logging::setup_logging;
pub(crate) use plan_restore::restore as restore_plans;
pub(crate) use progress::StartupStep;
pub(crate) use rest_api::{
    RestApiControl, RestApiControlState, RestApiRecovery, RestApiRuntimePolicy, RestApiStatus,
};
pub(crate) use restore_complete::notify as restore_complete;
pub(crate) use servers::start_servers;
#[cfg(test)]
pub(crate) use servers::FRONTEND_READINESS_DEADLINE_ELAPSED;
pub(crate) use states::init_states;
pub(crate) use user_path::run_user_path_repair_if_invoked;
pub(crate) use window::{build_system_tray, setup_window_styling};
