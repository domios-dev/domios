pub mod ansi;
pub mod capture;
pub mod cast;
pub mod login_shell;
pub mod manager;
pub mod screen_state;
pub mod shell_integration;
mod spawn_gate;
mod teardown;
pub mod write_queue;
#[cfg(all(test, target_os = "windows"))]
mod windows_test_support;
