//! Crate startup orchestration: state init, window/tray, background tasks,
//! CLI install, server boot, logging, bridge.
//!
//! Layout matches the order the entry-point in `lib.rs` calls them in.

pub mod boot_timing;
mod bridge;
mod cli_install;
pub(crate) mod conpty_cleanup;
pub mod desk;
pub(crate) mod frontend_bundles;
pub use frontend_bundles::check as frontend_bundles_check;
mod frontend_origins;
mod frontend_ready;
pub use frontend_origins::declared_frontend_origins as frontend_shell_declared_frontend_origins;
mod progress;
pub mod rest_api;
mod storage;

pub use bridge::{spawn_telegram_bridge, start_telegram_bridge, unmark_tab_connected_service};
pub use cli_install::install_bundled_cli;
pub use cli_install::refresh_bundled_cli;
pub use frontend_ready::arm_frontend_readiness;
pub use frontend_ready::frontend_readiness_generation;
pub use frontend_ready::mark_frontend_ready;
pub use frontend_ready::observe_frontend_boot;
pub use frontend_ready::wait_for_frontend_ready_until;
pub use progress::StartupStep;
pub use rest_api::RestApiDiagnostic;
pub use rest_api::{
    RestApiControl, RestApiControlState, RestApiPhase, RestApiRecovery, RestApiRuntimePolicy,
    RestApiStatus,
};
mod state_types;
pub use state_types::AppStates;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use storage::open_instance_storage_at;
pub use storage::shutdown_bounded;
pub use storage::StorageAuthority;
pub use storage::STORAGE_UNAVAILABLE_MESSAGE;

#[cfg(windows)]
pub use conpty_cleanup::reap_orphaned_hosts_in_background;
pub use frontend_ready::FRONTEND_READINESS_DEADLINE_ELAPSED;

pub use frontend_bundles::open as frontend_bundles_open;

pub use progress::Boundary;
pub use rest_api::listeners::additional_bind as rest_api_additional_bind;
pub use storage::open_instance_storage;

#[cfg(debug_assertions)]
pub mod runtime_diagnostics;

pub use frontend_bundles::navigation_index as frontend_bundles_navigation_index;

pub use cli_install::resolve_bundled_cli_src;
