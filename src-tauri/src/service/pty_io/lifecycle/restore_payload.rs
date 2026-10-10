use crate::multiplexer::types::{
    PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};

pub(super) fn requires_pending_input(record: Option<&PendingDeliveryRecord>) -> bool {
    record.is_some_and(|record| {
        record.channel == PendingDeliveryChannel::InteractiveInput
            && record.state() == PendingDeliveryState::Pending
            && record.attempt_provenance().is_none()
    })
}

pub(in crate::service::pty_io) fn resolve_restored_payload(
    record: Option<&PendingDeliveryRecord>,
) -> Result<Option<String>, String> {
    if !requires_pending_input(record) {
        return Ok(None);
    }
    Err(
        "restored_input_pending: Pending input is retained; automatic restore cannot replay it"
            .into(),
    )
}

#[derive(Clone)]
pub(super) struct RestoreAuthority {
    provider: Option<String>,
    session: Option<String>,
    cwd: Option<String>,
    epoch: Option<u64>,
    operation: Option<String>,
    pty: Option<String>,
    nonce: Option<String>,
    reported_nonce: Option<String>,
    stage: Option<crate::service::boot_stage::BootStage>,
    reason: Option<String>,
    pub(super) pending: Option<PendingDeliveryRecord>,
}

impl RestoreAuthority {
    pub(super) fn capture(tab: &crate::multiplexer::types::Tab) -> Self {
        Self {
            provider: tab.agent_type.clone(),
            session: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone(),
            epoch: tab.lifecycle_epoch,
            operation: tab.lifecycle_operation_id.clone(),
            pty: tab.pty_id.clone(),
            nonce: tab.session_binding_nonce.clone(),
            reported_nonce: tab.reported_session_binding_nonce.clone(),
            stage: tab.boot_stage,
            reason: tab.boot_failure_reason.clone(),
            pending: tab.pending_delivery.clone(),
        }
    }

    pub(super) fn matches(&self, tab: &crate::multiplexer::types::Tab) -> bool {
        tab.restored_from_snapshot
            && !tab.agent_booting
            && !tab.protected
            && self.provider == tab.agent_type
            && self.session == tab.agent_session_id
            && self.cwd == tab.cwd
            && self.epoch == tab.lifecycle_epoch
            && self.operation == tab.lifecycle_operation_id
            && self.pty == tab.pty_id
            && self.nonce == tab.session_binding_nonce
            && self.reported_nonce == tab.reported_session_binding_nonce
            && self.stage == tab.boot_stage
            && self.reason == tab.boot_failure_reason
            && self.pending == tab.pending_delivery
    }
}

pub(super) fn commit_restore_failure(
    mux: &crate::multiplexer::commands::MuxState,
    tab_id: &str,
    authority: &RestoreAuthority,
    reason: &str,
    publish: impl FnOnce(&crate::multiplexer::types::Tab),
) -> bool {
    crate::service::boot_stage::commit_boot_stage_transition(
        mux,
        tab_id,
        |tab| {
            if !authority.matches(tab) {
                return false;
            }
            tab.reported_session_binding_nonce = None;
            crate::service::boot_stage::apply_boot_stage(
                tab,
                crate::service::boot_stage::BootStage::SpawnFailed,
                Some(reason),
            );
            true
        },
        publish,
    )
}

#[cfg(test)]
mod tests;
