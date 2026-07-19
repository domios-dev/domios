//! Agent spawn & batch command orchestration.

mod batch;
mod launch;
pub mod pane_relink_signal;
pub mod refresh;
mod spawn;
mod types;

pub(crate) use launch::{
    build_agent_command, launch_agent_in_background, record_reader_termination,
    role_onboarding_task,
};
#[cfg(test)]
pub(crate) use launch::{
    should_deliver_spawn_preferences, skip_local_worker_role, spawn_onboarding_task, AgentRegistry,
};
pub use refresh::{RefreshOutcome, RefreshRequest, RefreshTrigger, RefreshTuning};
pub use types::{BatchOp, SpawnAgentParams, SpawnIdentityProvenance, SpawnResult};

#[cfg(test)]
mod tests;
