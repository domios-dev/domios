use super::{storage, ServiceContext};

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

pub(super) fn schedule_async(service: &ServiceContext, _tab: Option<String>) {
    let service = service.clone();
    super::spawn_utility("provider-update-idle", move || schedule(&service, None));
}

pub(super) fn schedule(service: &ServiceContext, _only_tab: Option<&str>) {
    if let Err(error) =
        storage::transact(|state| state.recover_interrupted(super::command::owner_alive))
    {
        log::error!("[provider-update] interrupted update recovery failed: {error}");
    }
    super::process_waiting(service);
}
