use super::ServiceContext;

/// Deliver a task into a hidden PtyInput agent (e.g. cursor) after its TUI boots.
/// Mirrors the visible orchestrator launch path (orchestrator::launch step 4).
/// Uses the same optimistic seam as `send_text` — success never depends on
/// echo/submit confirmation, only a genuine write/transport failure errs.
pub(super) fn deliver_hidden_pty_task(
    svc: &ServiceContext,
    pty_id: &str,
    tab_id: &str,
    provider: &str,
    task_text: &str,
) -> Result<(), String> {
    if !svc.await_agent_tui_on_screen(pty_id, provider, std::time::Duration::from_secs(15)) {
        log::warn!(
            "[spawn.hidden] timed out waiting for agent TUI before task delivery in Tab {}",
            tab_id
        );
    }
    svc.send_text_to_pty(pty_id, task_text)
}

/// Model, reasoning-effort, and unattended-permission args for a hidden launch.
pub(super) fn hidden_launch_args(
    svc: &ServiceContext,
    tab_id: &str,
    agent: &super::AgentSpawnSpec,
    launch_model: &Option<String>,
    full_access: bool,
) -> Vec<String> {
    let launch_model = launch_model.as_deref();
    let mut args = launch_model
        .and_then(|model| {
            svc.agent_registry
                .with_provider(&agent.provider, |provider| {
                    provider.model_launch_args(model)
                })
        })
        .unwrap_or_default();
    args.extend(svc.reasoning_effort_launch_args_or_notice(
        tab_id,
        &agent.provider,
        launch_model,
        agent.reasoning_effort.as_deref(),
    ));
    args.extend(crate::service::unattended_launch::grant_args(
        &svc.agent_registry,
        &agent.unattended_launch(&agent.provider, launch_model, full_access),
    ));
    args
}
