use super::ServiceContext;

pub(super) fn provider_busy(service: &ServiceContext, provider: &str) -> bool {
    let snapshot = service.snapshot_with_reconciled_status();
    snapshot.tabs.iter().any(|(id, tab)| {
        if tab.agent_type.as_deref() != Some(provider) {
            return false;
        }
        if !crate::service::background_work::running(service, id).is_empty() {
            return true;
        }
        if let Some(sidecar) = crate::service::codex_rpc::diagnostics::for_tab(id) {
            return sidecar.active_turn || !sidecar.selection_verified;
        }
        let status = crate::multiplexer::state::effective_tab_status(
            tab,
            snapshot.tab_statuses.get(id).copied(),
        );
        !matches!(
            status,
            codeterm_types::pty::TabStatus::Idle | codeterm_types::pty::TabStatus::Dead
        )
    })
}
