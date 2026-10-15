use std::sync::Arc;
use std::time::Duration;

use super::super::{
    AgentRestartError, RestartAttemptError, RestartOutcome, TabRestartSnapshot,
    RESTART_READY_TIMEOUT_SECS,
};
use crate::service::ServiceContext;

impl ServiceContext {
    pub(super) fn recover_general_agent_fresh(
        &self,
        snap: &TabRestartSnapshot,
        force: bool,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        let config = self
            .try_ext::<crate::config::ConfigState>()
            .ok_or_else(|| {
                AgentRestartError::Internal("General Agent configuration is unavailable".into())
            })?;
        if config.lock().general_agent.agent_provider != snap.provider {
            return Err(AgentRestartError::BadRequest(
                "General Agent provider changed after its exit".into(),
            )
            .into());
        }
        self.record_lifecycle(
            &snap.tab_id,
            crate::session::lifecycle::LifecycleTransition::RestartRequested,
        );
        let outcome = RestartOutcome::accepted(snap, false, force);
        let svc = Arc::new(self.clone());
        crate::service::general_agent::restart_general_agent_fresh_after_exit(
            &svc,
            &self.mux,
            &self.pty,
            &self.screen_state,
            &config,
            &self.app_handle,
        )
        .map_err(AgentRestartError::Internal)?;
        let mut ready = self.finish_observed_restart(
            outcome,
            Some(&snap.pty_id),
            Duration::from_secs(RESTART_READY_TIMEOUT_SECS),
        )?;
        ready.old_pty_torn_down = self.old_pty_was_torn_down(snap);
        self.settle_restored_binding(ready, snap)
    }
}
