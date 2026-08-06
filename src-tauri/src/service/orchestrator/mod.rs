//! Agent spawn & batch command orchestration.

mod batch;
mod launch;
mod launch_env;
pub mod refresh;
mod spawn;
pub mod tab_relink_signal;
mod types;

#[cfg(test)]
pub(crate) use launch::{
    arm_provider_session_for_test, role_onboarding_task, skip_local_worker_role,
    spawn_onboarding_task, AgentRegistry, LaunchGeneration,
};
pub(crate) use launch::{
    build_agent_command, launch_agent_in_background, launch_agent_in_background_with_env,
    provider_launch_task, record_provider_session, record_reader_termination, role_launch_task,
    ProviderSessionBinding,
};
pub use refresh::{RefreshOutcome, RefreshRequest, RefreshTrigger, RefreshTuning};
pub(crate) use types::TaskLaunchAuthority;
pub use types::{BatchOp, SpawnAgentParams, SpawnIdentityProvenance, SpawnResult};

#[cfg(test)]
mod tests;
