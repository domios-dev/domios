//! Crate startup orchestration: state init, window/tray, background tasks,
//! CLI install, server boot, logging, bridge.
//!
//! Layout matches the order the entry-point in `lib.rs` calls them in.

mod background;
mod bridge;
mod cli_install;
mod cli_shim;
pub(crate) mod conpty_cleanup;
mod conpty_host;
pub(crate) mod frontend_bundles;
pub use frontend_bundles::check as frontend_bundles_check;
mod frontend_ready;
pub(crate) mod frontend_shell;
pub use frontend_shell::declared_frontend_origins as frontend_shell_declared_frontend_origins;
mod logging;
mod native_notification;
pub(crate) use native_notification::native_notification;
pub(crate) mod model_catalogue;
mod plan_restore;
mod progress;
mod rest_api;
pub(crate) mod restart_stdio;
pub use restart_stdio::detach_for_restart as restart_stdio_detach_for_restart;
mod restore_complete;
mod servers;
pub mod snapshot_bound;
#[cfg(test)]
mod stall_tests;
mod state_incidents;
mod states;
mod storage;
mod user_path;
mod window;
mod window_events;
pub(crate) use window_events::handle_window_event;

pub(crate) use background::start_background_tasks;
pub use bridge::{spawn_telegram_bridge, start_telegram_bridge, unmark_tab_connected_service};
pub(crate) use cli_install::install_bundled_cli;
pub use cli_install::refresh_bundled_cli;
pub(crate) use cli_shim::forward_to_cli_if_invoked;
pub(crate) use conpty_host::setup_conpty_host;
pub(crate) use frontend_ready::arm_frontend_readiness;
pub use frontend_ready::frontend_readiness_generation;
pub use frontend_ready::mark_frontend_ready;
pub(crate) use frontend_ready::observe_frontend_boot;
pub(crate) use frontend_ready::wait_for_frontend_ready_until;
pub(crate) use logging::setup_logging;
pub(crate) use plan_restore::restore as restore_plans;
pub(crate) use progress::StartupStep;
pub use rest_api::RestApiControl;
pub use rest_api::RestApiControlState;
pub use rest_api::RestApiDiagnostic;
pub(crate) use rest_api::RestApiPhase;
pub(crate) use rest_api::RestApiRecovery;
pub use rest_api::RestApiRuntimePolicy;
pub(crate) use rest_api::RestApiStatus;
pub(crate) use restore_complete::notify as restore_complete;
pub(crate) use servers::start_servers;
#[cfg(test)]
pub(crate) use servers::FRONTEND_READINESS_DEADLINE_ELAPSED;
pub(crate) use states::init_states;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use storage::open_instance_storage_at;
pub(crate) use storage::shutdown_bounded;
pub use storage::StorageAuthority;
pub use storage::STORAGE_UNAVAILABLE_MESSAGE;
pub(crate) use user_path::run_user_path_repair_if_invoked;
pub(crate) use window::{build_system_tray, setup_window_styling};
