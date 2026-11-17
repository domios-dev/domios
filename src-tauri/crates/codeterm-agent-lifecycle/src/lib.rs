//! Agent restart, model switch, exited-member restore and worker interruption, above the core.
//!
//! The crate mirrors the core's `crate::` and `crate::service::` paths, so the moved modules
//! keep theirs; the core reaches this crate only through `ports::lifecycle`.

pub(crate) use codeterm_service_core::{
    agent, async_runtime, config, crash, multiplexer, paths, plan, process_util, pty, session,
};
#[cfg(test)]
pub(crate) use codeterm_service_core::{dispatch, plugin, prompts, secrets, storage, test_support};

mod lifecycle_port;
pub mod prelude;
pub mod service;

pub use lifecycle_port::register_agent_lifecycle;
