//! Native window shell of the client: Tauri runtime aliases, desktop commands and the pair updater.

pub mod app_runtime;
pub mod application_host;
mod autostart;
#[cfg(target_os = "linux")]
pub use autostart::refresh_owned_linux_unit;
pub mod clipboard_files;
pub mod commands;
mod update;
