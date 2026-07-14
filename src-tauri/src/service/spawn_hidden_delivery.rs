use super::ServiceContext;

/// Deliver a task into a hidden PtyInput agent (codex/opencode) after its TUI boots.
/// Mirrors the visible orchestrator launch path (orchestrator::launch step 4).
pub(super) fn deliver_hidden_pty_task(
    svc: &ServiceContext,
    pty_id: &str,
    pane_id: &str,
    provider: &str,
    task_text: &str,
) {
    if !svc.await_agent_tui_on_screen(pty_id, provider, std::time::Duration::from_secs(15)) {
        log::warn!(
            "[spawn.hidden] timed out waiting for agent TUI before task delivery in pane {}",
            pane_id
        );
    }
    let submit_strategy = svc
        .agent_registry
        .with_provider(provider, |p| p.verify_submit_strategy())
        .unwrap_or(crate::agent::provider::VerifySubmitStrategy::ShellPrompt {
            timeout: std::time::Duration::from_secs(5),
        });
    let opts = crate::service::inject_verify::VerifyOpts {
        require_prompt_first: false,
        verify_echo: true,
        verify_submit: true,
        prompt_timeout: std::time::Duration::from_secs(0),
        echo_timeout: std::time::Duration::from_secs(10),
        submit_timeout: std::time::Duration::from_secs(15),
    };
    match svc.send_text_verified(pty_id, task_text, true, submit_strategy, opts) {
        Ok(result) => {
            if !result.echo_observed || !result.submit_observed {
                log::error!(
                    "[spawn.hidden] task delivery unconfirmed for pane {} (echo={}, submit={})",
                    pane_id,
                    result.echo_observed,
                    result.submit_observed
                );
            }
        }
        Err(e) => log::error!(
            "[spawn.hidden] task delivery failed for pane {}: {}",
            pane_id,
            e
        ),
    }
}
