use super::*;
use crate::multiplexer::types::{AgentRole, TabBacking};
use crate::service::plan::task_attempts::{
    CompleteAcknowledgedDispatchBinding, TaskAttemptAcknowledgement,
};

impl WorkerAckRegistry {
    fn with_current_task_launch_authority<T>(
        &self,
        tab_id: &str,
        use_authority: impl FnOnce(&StartupAuthority) -> Result<T, String>,
    ) -> Result<T, String> {
        let waiters = self.startup_waiters.lock();
        let acknowledged = self.startup_acknowledged.lock();
        let finalized = self.finalized_startups.lock();
        if finalized.contains_key(tab_id) {
            return Err("retained startup authority is finalized".into());
        }
        let waiting = waiters.get(tab_id).map(|waiter| &waiter.authority);
        let ready = acknowledged.get(tab_id);
        if waiting
            .zip(ready)
            .is_some_and(|(waiting, ready)| waiting != ready)
        {
            return Err("retained startup authority is ambiguous".into());
        }
        let authority = waiting
            .or(ready)
            .ok_or("no retained startup authority for this launch")?;
        use_authority(authority)
    }
}

impl ServiceContext {
    pub(crate) fn complete_acknowledged_dispatch_binding(
        &self,
        acknowledgement: &TaskAttemptAcknowledgement,
        nonce: &str,
        session_claim: Option<&str>,
        now_ms: u64,
    ) -> Result<crate::plan::TaskExecutionAttempt, String> {
        let mux = self.mux.read();
        let tab_id = &acknowledgement.attempt.executor_tab_id;
        let tab = mux.get_tab(tab_id).ok_or("acknowledging tab is missing")?;
        let binding = super::super::task_assignment::authenticated_tab_binding(tab)
            .ok_or("acknowledging tab has no verified session binding")?;
        let team_id = tab
            .team_id
            .as_deref()
            .ok_or("acknowledging tab has no Team")?;
        if tab.role != Some(AgentRole::Worker)
            || tab.backing != TabBacking::Pty
            || tab.agent_booting
            || tab.boot_stage != Some(codeterm_types::pty::BootStage::AgentReady)
            || mux.get_team(team_id).is_none()
            || binding.session_binding_nonce != nonce
            || session_claim.is_some_and(|session| session != binding.session_id)
            || !acknowledgement.already_acknowledged
            || tab.task_binding.as_ref().is_none_or(|task| {
                task.plan_id.as_deref() != Some(acknowledgement.plan_id.as_str())
                    || task.task_id.as_deref() != Some(acknowledgement.task_id.as_str())
                    || task.attempt_id.as_deref() != Some(acknowledgement.attempt.id.as_str())
            })
        {
            return Err("acknowledging caller does not own the exact dispatch assignment".into());
        }
        self.worker_ack
            .with_current_task_launch_authority(tab_id, |authority| {
                let expected = StartupTaskIdentity {
                    plan_id: acknowledgement.plan_id.clone(),
                    task_id: acknowledgement.task_id.clone(),
                    attempt_id: acknowledgement.attempt.id.clone(),
                };
                let creator = mux
                    .get_tab(&authority.creator_tab_id)
                    .ok_or("launch creator is missing")?;
                if authority.child_tab_id != *tab_id
                    || authority.task_identity.as_ref() != Some(&expected)
                    || authority.team_id.as_deref() != Some(team_id)
                    || creator.team_id.as_deref() != Some(team_id)
                    || !creator.can_spawn
                    || creator.agent_type.is_none()
                    || tab.parent_tab_id.as_deref() != Some(authority.creator_tab_id.as_str())
                    || tab.pty_id.as_deref() != Some(authority.attempt_id.as_str())
                    || authority.binding_nonce.as_deref() != Some(nonce)
                    || authority
                        .session_id
                        .as_deref()
                        .is_some_and(|session| session != binding.session_id)
                    || authority.task_owner_epoch
                        != Some(acknowledgement.attempt.interruption.owner_epoch)
                {
                    return Err(
                        "retained launch provenance does not own the current assignment/session"
                            .into(),
                    );
                }
                self.plan_store
                    .complete_acknowledged_dispatch_binding(
                        CompleteAcknowledgedDispatchBinding {
                            plan_id: expected.plan_id,
                            task_id: expected.task_id,
                            attempt_id: expected.attempt_id,
                            tab_id: tab_id.clone(),
                            team_id: team_id.into(),
                            creator_tab_id: authority.creator_tab_id.clone(),
                            provider: binding.provider.ok_or("launch provider is missing")?,
                            session_id: binding.session_id,
                            nonce: nonce.into(),
                            owner_epoch: acknowledgement.attempt.interruption.owner_epoch,
                        },
                        now_ms,
                    )
                    .map_err(|error| error.to_string())
            })
    }
}

#[cfg(test)]
#[path = "launch_binding/tests.rs"]
mod tests;
