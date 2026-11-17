//! Agent spawn & batch command orchestration.

mod launch;
#[cfg(any(test, feature = "test-support"))]
pub use launch::test_support as launch_test_support;
mod launch_env;
mod prompt_compose;
mod spawn;
#[cfg(test)]
pub(crate) use spawn::initialize_ordinary_pty_agent_tab;
pub(crate) use spawn::runtime_prompt::bind_manager_launch_identity;
#[cfg(test)]
pub(crate) use spawn::runtime_prompt::bound_plan_id;
pub mod tab_relink_signal;
mod types;

#[cfg(any(test, feature = "test-support"))]
pub use launch::arm_provider_session_for_test;
#[cfg(any(test, feature = "test-support"))]
pub use launch::launch_waits_for_first_turn_binding;
pub use launch::prepare_agent_command_for_service;
#[cfg(test)]
pub(crate) use launch::skip_local_worker_role;
#[cfg(test)]
pub(crate) use launch::AgentRegistry;
pub use launch::LaunchGeneration;
pub use launch::PreparedAgentCommand;
pub use launch::{
    build_agent_command, build_agent_command_for_service, build_agent_command_with_launch_marker,
    build_agent_command_with_launch_marker_for_service, launch_agent_in_background,
    record_provider_session, record_reader_termination, supersede_tab_launch,
    ProviderSessionBinding,
};
#[cfg(test)]
pub(crate) use prompt_compose::role_launch_task;
#[cfg(test)]
pub(crate) use prompt_compose::role_onboarding_task;
#[cfg(any(test, feature = "test-support"))]
pub use prompt_compose::spawn_onboarding_task;
pub use prompt_compose::{provider_launch_task, restorable_identity_task};
pub use spawn::{
    compose_runtime_system_prompt, ensure_tab_runtime_prompt,
    move_chatbackend_task_to_system_prompt, remote_task_binding_matches_authority, ROLE_AUTHORITY,
};
pub use types::RemoteTaskBinding;
pub use types::TaskLaunchAuthority;
pub use types::{BatchOp, SpawnAgentParams, SpawnIdentityProvenance, SpawnResult};

#[cfg(test)]
mod tests;

pub use launch::mark_tab_agent_launch_started;
#[cfg(test)]
pub(crate) use launch::reserve_launch_binding_nonce;

pub use launch::launch_agent_in_background_with_invocation;

#[allow(clippy::too_many_arguments)]
pub fn launch_agent_in_background_with_env(
    svc: crate::service::ServiceContext,
    tab_id: String,
    launch_cmd: impl Into<PreparedAgentCommand>,
    agent_cmd: String,
    origin: crate::service::launch_diagnostics::AgentLaunchOrigin,
    has_startup_task_turn: bool,
    startup_turn_text: Option<String>,
    task: Option<String>,
    fallback_spec: Option<crate::service::launch_delivery::FallbackDeliverySpec>,
    launch_env: std::collections::HashMap<String, String>,
    task_identity: Option<crate::service::startup_lease::StartupTaskIdentity>,
    ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    resume_session_id: Option<String>,
) -> Option<String> {
    launch_agent_in_background_with_invocation(
        svc,
        tab_id,
        launch_cmd,
        agent_cmd,
        origin,
        has_startup_task_turn,
        startup_turn_text,
        task,
        fallback_spec,
        launch_env,
        task_identity,
        ready_tx,
        resume_session_id,
        None,
    )
}
