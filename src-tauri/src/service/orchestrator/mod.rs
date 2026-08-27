//! Agent spawn & batch command orchestration.

mod batch;
mod launch;
mod launch_env;
mod prompt_compose;
pub mod refresh;
mod spawn;
pub mod tab_relink_signal;
mod types;

#[cfg(test)]
pub(crate) use launch::{
    arm_provider_session_for_test, skip_local_worker_role, AgentRegistry, LaunchGeneration,
};
pub(crate) use launch::{
    build_agent_command, launch_agent_in_background, launch_agent_in_background_with_env,
    record_provider_session, record_reader_termination, ProviderSessionBinding,
};
pub(crate) use prompt_compose::{provider_launch_task, role_launch_task};
#[cfg(test)]
pub(crate) use prompt_compose::{role_onboarding_task, spawn_onboarding_task};
pub use refresh::{RefreshOutcome, RefreshRequest, RefreshTrigger, RefreshTuning};
pub(crate) use spawn::move_chatbackend_task_to_system_prompt;
pub(crate) use types::TaskLaunchAuthority;
pub use types::{BatchOp, SpawnAgentParams, SpawnIdentityProvenance, SpawnResult};

#[cfg(test)]
mod tests;
