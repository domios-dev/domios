mod agent;
mod clipboard;
mod context_epochs;
mod essence_map;
#[cfg(feature = "desktop")]
mod file_actions;
#[cfg(feature = "desktop")]
pub use file_actions::*;
#[cfg(feature = "desktop")]
pub mod local_recording;
pub mod mux_commands;
pub mod plugins;
mod pty;
pub mod repo_watch;
#[cfg(feature = "desktop")]
mod system;
mod system_info;
mod tab;
#[cfg(feature = "desktop")]
mod update;

#[cfg(test)]
mod tests;

pub use agent::*;
pub use clipboard::*;
pub use essence_map::*;
pub use pty::*;
#[cfg(feature = "desktop")]
pub use system::*;
pub use system_info::*;
pub use tab::*;
#[cfg(feature = "desktop")]
pub use update::*;

#[cfg(test)]
mod identity_tests;
#[cfg(test)]
mod ownership_tests;
#[cfg(test)]
mod resize_tests;
