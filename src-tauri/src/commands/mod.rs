// Public command return DTOs for external callers (e.g. `commands::DirCompletion`).

#[derive(serde::Serialize)]
pub struct DirCompletion {
    pub name: String,
    pub score: i32,
}

mod agent;
mod automation_graph;
mod blueprint_portable;
pub use blueprint_portable::*;
pub(crate) mod bridge_threads;
mod clipboard;
mod completions;
mod context_epochs;
mod cron;
mod delayed_send;
mod essence_map;
mod file_actions;
mod hooks;
pub(crate) mod html_preview;
pub use file_actions::*;
mod model_providers;
pub mod mux_commands;
mod plans;
pub(crate) mod plugins;
mod pty;
pub(crate) mod repo_watch;
mod services;
mod skills;
mod system;
mod system_info;
mod tab;
mod update;
mod usage;

#[cfg(test)]
mod tests;

pub use agent::*;
pub use automation_graph::*;
pub use clipboard::*;
pub use completions::*;
pub use cron::*;
pub use delayed_send::*;
pub use essence_map::*;
pub use hooks::*;
pub use html_preview::*;
pub use model_providers::*;
pub use plans::*;
pub use plugins::*;
pub use pty::*;
pub use repo_watch::*;
pub use services::*;
pub use skills::*;
pub use system::*;
pub use system_info::*;
pub use tab::*;
pub use update::*;
pub use usage::*;

mod capability_probe;
pub use capability_probe::*;
