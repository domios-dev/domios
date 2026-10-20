//! Native window shell of the client: Tauri runtime aliases, desktop commands and the pair updater.

pub mod app_runtime;
pub mod application_host;
mod autostart;
pub mod clipboard_files;
pub mod commands;
mod update;

extern crate domios_build_info as codeterm_build_info; // rename-bridge
extern crate domios_crash as codeterm_crash; // rename-bridge
extern crate domios_paths as codeterm_paths; // rename-bridge
extern crate domios_update as codeterm_update; // rename-bridge
extern crate domios_util as codeterm_util; // rename-bridge
