//! Crate startup orchestration: state init, window/tray, background tasks,
//! CLI install, server boot, logging, bridge.
//!
//! Layout matches the order the entry-point in `lib.rs` calls them in.

mod bridge;
mod cli_install;
pub(crate) mod conpty_cleanup;
pub(crate) mod frontend_bundles;
pub use frontend_bundles::check as frontend_bundles_check;
mod frontend_ready;
pub(crate) mod frontend_shell;
pub use frontend_shell::declared_frontend_origins as frontend_shell_declared_frontend_origins;
mod progress;
pub mod rest_api;
pub(crate) mod restart_stdio;
pub use restart_stdio::detach_for_restart as restart_stdio_detach_for_restart;
pub mod snapshot_bound;
mod storage;

pub use bridge::{spawn_telegram_bridge, start_telegram_bridge, unmark_tab_connected_service};
pub use cli_install::install_bundled_cli;
pub use cli_install::refresh_bundled_cli;
pub use frontend_ready::arm_frontend_readiness;
pub use frontend_ready::frontend_readiness_generation;
pub use frontend_ready::mark_frontend_ready;
pub(crate) use frontend_ready::observe_frontend_boot;
pub use frontend_ready::wait_for_frontend_ready_until;
pub use progress::StartupStep;
pub use rest_api::RestApiDiagnostic;
pub use rest_api::{
    RestApiControl, RestApiControlState, RestApiPhase, RestApiRecovery, RestApiRuntimePolicy,
    RestApiStatus,
};
mod states;
pub use states::AppStates;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use storage::open_instance_storage_at;
pub use storage::shutdown_bounded;
pub use storage::StorageAuthority;
pub use storage::STORAGE_UNAVAILABLE_MESSAGE;

pub use conpty_cleanup::reap_orphaned_hosts_in_background;
pub use frontend_ready::FRONTEND_READINESS_DEADLINE_ELAPSED;

mod user_path;
pub use user_path::{repair_user_path_with, user_path_repair_report, SystemUserPathStore};

pub use frontend_bundles::open as frontend_bundles_open;
pub use frontend_shell::{
    application_context as frontend_shell_application_context,
    create_window as frontend_shell_create_window, retry as frontend_shell_retry,
    start as frontend_shell_start,
};
pub use progress::Boundary;
pub use rest_api::listeners::additional_bind as rest_api_additional_bind;
pub use storage::open_instance_storage;
