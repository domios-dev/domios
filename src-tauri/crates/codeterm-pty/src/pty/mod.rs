pub mod ansi;
pub mod backend_output;
pub mod capture;
pub mod cast;
pub mod child_env;
mod child_exit;
pub mod login_shell;
pub mod manager;
pub mod screen_state;
pub mod scrollback_capture;
pub mod shell_integration;
pub mod shell_profiles;
mod spawn_gate;
mod teardown;
#[cfg(all(test, target_os = "windows"))]
mod windows_test_support;
pub mod write_queue;
