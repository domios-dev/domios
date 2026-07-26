//! Agent spawn & batch command orchestration.

mod batch;
mod launch;
pub mod pane_relink_signal;
pub mod refresh;
mod spawn;
mod types;

#[cfg(test)]
pub(crate) use launch::{
    arm_provider_session_for_test, skip_local_worker_role, spawn_onboarding_task, AgentRegistry,
    LaunchGeneration,
};
pub(crate) use launch::{
    build_agent_command, launch_agent_in_background, record_provider_session,
    record_reader_termination, role_onboarding_task, ProviderSessionBinding,
};
pub use refresh::{RefreshOutcome, RefreshRequest, RefreshTrigger, RefreshTuning};
pub use types::{BatchOp, SpawnAgentParams, SpawnIdentityProvenance, SpawnResult};

#[cfg(test)]
mod tests;
