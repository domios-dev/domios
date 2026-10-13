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
pub(crate) use launch::launch_waits_for_first_turn_binding;
#[cfg(test)]
pub(crate) use launch::{
    arm_provider_session_for_test, skip_local_worker_role, AgentRegistry, LaunchGeneration,
};
pub(crate) use launch::{
    build_agent_command, build_agent_command_for_service, build_agent_command_with_launch_marker,
    build_agent_command_with_launch_marker_for_service, launch_agent_in_background,
    launch_agent_in_background_with_env, mark_tab_agent_launch_started, record_provider_session,
    record_reader_termination, supersede_tab_launch, ProviderSessionBinding,
};
#[cfg(test)]
pub(crate) use prompt_compose::role_launch_task;
pub(crate) use prompt_compose::{provider_launch_task, restorable_identity_task};
#[cfg(test)]
pub(crate) use prompt_compose::{role_onboarding_task, spawn_onboarding_task};
pub use refresh::{RefreshOutcome, RefreshRequest, RefreshTrigger, RefreshTuning};
pub(crate) use spawn::{
    compose_runtime_system_prompt, ensure_tab_runtime_prompt,
    move_chatbackend_task_to_system_prompt, remote_task_binding_matches_authority, ROLE_AUTHORITY,
};
pub use types::{BatchOp, SpawnAgentParams, SpawnIdentityProvenance, SpawnResult};
pub(crate) use types::{RemoteTaskBinding, TaskLaunchAuthority};

#[cfg(test)]
mod tests;
