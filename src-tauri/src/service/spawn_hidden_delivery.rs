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
    svc.send_text_to_pty_smart(pty_id, task_text, true)
}
