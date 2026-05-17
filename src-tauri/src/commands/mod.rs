// Shared inline types used as command return types live here so external
// callers can reference them as `commands::DirCompletion`, etc.

#[derive(serde::Serialize)]
pub struct DirCompletion {
    pub name: String,
    pub score: i32,
}

mod agent;
mod clipboard;
mod completions;
mod cron;
mod hooks;
mod pane;
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
pub use hooks::*;
pub use pane::*;
pub use pty::*;
pub use services::*;
pub use skills::*;
pub use system::*;
pub use system_info::*;
pub use update::*;
pub use usage::*;
