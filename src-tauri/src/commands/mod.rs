// Public command return DTOs for external callers (e.g. `commands::DirCompletion`).

#[derive(serde::Serialize)]
pub struct DirCompletion {
    pub name: String,
    pub score: i32,
}

mod agent;
mod clipboard;
mod completions;
mod cron;
mod delayed_send;
mod hooks;
pub mod mux_commands;
mod tab;
pub(crate) mod plugins;
mod pty;
mod services;
mod skills;
mod system;
mod system_info;
mod update;
mod usage;

#[cfg(test)]
mod tests;

pub use agent::*;
pub use clipboard::*;
pub use completions::*;
pub use cron::*;
pub use delayed_send::*;
pub use hooks::*;
pub use tab::*;
pub use plugins::*;
pub use pty::*;
pub use services::*;
pub use skills::*;
pub use system::*;
pub use system_info::*;
pub use update::*;
pub use usage::*;
