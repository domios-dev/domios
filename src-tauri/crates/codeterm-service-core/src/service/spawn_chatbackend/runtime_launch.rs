use super::super::ServiceContext;
use crate::service::MuxEmitCause;

/// The provider runtime start of a staged chatBackend tab, which a dispatch may run off its RPC thread.
pub(crate) struct ChatBackendRuntimeLaunch {
    pub(super) svc: ServiceContext,
    pub(super) plugin_id: String,
    pub(super) tab_id: String,
    pub(super) workspace_id: String,
    pub(super) config: serde_json::Value,
    pub(super) history_meta: serde_json::Value,
    pub(super) runtime_fork: Option<crate::service::spawn::RuntimeForkLaunch>,
    pub(super) emit_state_change: bool,
}

impl ChatBackendRuntimeLaunch {
    pub(crate) fn can_defer(&self) -> bool {
        self.runtime_fork.is_none()
    }

    pub(crate) fn run(self) -> Result<(), String> {
        let svc = self.svc;
        if let Err(error) = svc.chatbackend_launch_with_runtime_fork(
            self.plugin_id,
            self.tab_id.clone(),
            self.config,
            self.history_meta,
            self.runtime_fork.clone(),
        ) {
            if self.runtime_fork.is_some()
                || !crate::service::agent_runtime::requested_for_tab(&svc, &self.tab_id)
            {
                svc.rollback_provisional_tab(&self.workspace_id, &self.tab_id);
                let _ = svc.revoke_agent_pad(&self.tab_id);
            }
            if self.emit_state_change {
                svc.emit_state_changed(MuxEmitCause::TabLifecycle);
            }
            return Err(error);
        }
        svc.record_kpi_tab_created(
            &self.tab_id,
            crate::agent::kpi_ledger::TabCreatedKind::Agent,
        );
        Ok(())
    }
}
