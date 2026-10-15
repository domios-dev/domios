pub use codeterm_worker_services::launch_diagnostics::{
    log_shell_drop, redact_secrets, tail_bytes, AgentLaunchOrigin,
};
pub(crate) fn log_rpc_sidecar(tab: &str) {
    if let Some(diagnostics) = super::codex_rpc::diagnostics::for_tab(tab) {
        log::info!(
            "[launch] tab={tab} structured_control={}",
            serde_json::to_string(&diagnostics).unwrap_or_default()
        );
    }
}

pub(crate) fn log_launch(
    origin: AgentLaunchOrigin,
    tab_id: &str,
    provider: &str,
    command: &str,
    prompt: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
    prompt_delivery: &str,
) {
    log_launch_with_parameters(
        origin,
        tab_id,
        provider,
        command,
        prompt,
        role,
        role_profile,
        prompt_delivery,
        None,
        None,
    );
}

pub(crate) fn log_launch_with_parameters(
    origin: AgentLaunchOrigin,
    tab_id: &str,
    provider: &str,
    command: &str,
    prompt: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
    prompt_delivery: &str,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
) {
    if origin != AgentLaunchOrigin::GeneralAgent {
        crate::service::supporter_notify::record_launch(
            tab_id,
            origin == AgentLaunchOrigin::RestoredSnapshot,
        );
    }
    codeterm_worker_services::launch_diagnostics::log_launch_with_parameters(
        origin,
        tab_id,
        provider,
        command,
        prompt,
        role,
        role_profile,
        prompt_delivery,
        model,
        reasoning_effort,
    );
}
pub(crate) fn log_failure_screen(
    svc: &super::ServiceContext,
    tab_id: &str,
    pty_id: &str,
    reason: &str,
) {
    let screen = { svc.screen_state.lock().get_screen_text(pty_id) };
    codeterm_worker_services::launch_diagnostics::log_failure_screen(
        screen.as_deref(),
        tab_id,
        pty_id,
        reason,
    );
}

pub(crate) fn log_launch_environment(
    origin: &str,
    tab_id: &str,
    env: &std::collections::HashMap<String, String>,
) {
    codeterm_worker_services::launch_diagnostics::log_launch_environment(
        origin,
        tab_id,
        env,
        |job| {
            tauri::async_runtime::spawn_blocking(job);
        },
    );
}
