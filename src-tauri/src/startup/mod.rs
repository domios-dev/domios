mod cli_shim;
pub(crate) mod desktop_lifecycle;
pub(crate) mod frontend_shell;
mod native_notification;
pub(crate) mod starting_window;
mod user_path;
mod window;
mod window_events;

pub(crate) use cli_shim::forward_to_cli_if_invoked;
pub(crate) use native_notification::native_notification;
pub(crate) use user_path::run_user_path_repair_if_invoked;
pub(crate) use window::build_system_tray;
pub(crate) use window_events::handle_window_event;

pub(crate) use frontend_shell::{
    application_context as frontend_shell_application_context,
    initialize as frontend_shell_initialize, start as frontend_shell_start,
};
