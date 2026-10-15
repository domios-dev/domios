mod background;
mod cli_shim;
mod conpty_host;
mod frontend_shell;
mod logging;
pub(crate) mod model_catalogue;
mod native_notification;
mod plan_restore;
mod restore_complete;
mod restored_backends;
mod servers;
#[cfg(test)]
mod stall_tests;
mod state_incidents;
mod states;
mod user_path;
mod window;
mod window_events;

pub(crate) use background::start_background_tasks;
pub(crate) use cli_shim::forward_to_cli_if_invoked;
pub(crate) use codeterm_service_core::startup::install_bundled_cli;
pub(crate) use codeterm_service_core::startup::StartupStep;
pub(crate) use codeterm_service_core::startup::{
    arm_frontend_readiness, wait_for_frontend_ready_until, RestApiRuntimePolicy,
};
pub(crate) use conpty_host::setup_conpty_host;
pub(crate) use logging::setup_logging;
pub(crate) use native_notification::native_notification;
pub(crate) use plan_restore::restore as restore_plans;
pub(crate) use restore_complete::notify as restore_complete;
pub(crate) use servers::start_servers;
pub(crate) use states::init_states;
pub(crate) use user_path::run_user_path_repair_if_invoked;
pub(crate) use window::{build_system_tray, setup_window_styling};
pub(crate) use window_events::handle_window_event;

pub(crate) use frontend_shell::{
    application_context as frontend_shell_application_context,
    create_window as frontend_shell_create_window, start as frontend_shell_start,
};
