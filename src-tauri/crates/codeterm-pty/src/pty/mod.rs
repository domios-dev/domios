pub mod ansi;
pub mod capture;
pub mod cast;
pub mod login_shell;
pub mod manager;
pub mod screen_state;
pub mod scrollback_capture;
pub mod shell_integration;
mod spawn_gate;
mod teardown;
#[cfg(all(test, target_os = "windows"))]
mod windows_test_support;
pub mod write_queue;
