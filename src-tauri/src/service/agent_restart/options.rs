use super::{AgentRestartError, RestartAttemptError, RestartMode, RestartOutcome};
use crate::service::ServiceContext;

#[derive(Default)]
pub(crate) struct RestartOptions {
    pub force: bool,
    pub grace_secs: u64,
    pub target_provider: Option<String>,
    pub handoff_history: bool,
    pub fresh: bool,
}

impl ServiceContext {
    pub(crate) fn agent_restart_observed_with_options(
        &self,
        tab_id: &str,
        options: RestartOptions,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        let RestartOptions {
            force,
            grace_secs,
            target_provider,
            handoff_history,
            fresh,
        } = options;
        let (resolved, current_provider, backing) = {
            let mux = self.mux.read();
            let resolved = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {tab_id} not found")))?;
            let tab = mux
                .get_tab(&resolved)
                .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {resolved} not found")))?;
            (resolved, tab.agent_type.clone(), tab.backing.clone())
        };
        if let Some(source) = current_provider.as_deref() {
            if let Some(target) = target_provider
                .as_deref()
                .filter(|target| *target != source)
            {
                self.require_restart_provider_handoff(source, target)?;
            }
            let declared = crate::service::agent_runtime::engine::requested(self, source);
            if super::runtime::restart_route(declared, &backing)
                == super::runtime::RestartRoute::Runtime
            {
                if fresh && (target_provider.is_some() || handoff_history) {
                    return Err(AgentRestartError::BadRequest(
                        "fresh restart cannot change provider or hand off history".into(),
                    )
                    .into());
                }
                if handoff_history && target_provider.is_none() {
                    return Err(AgentRestartError::BadRequest(
                        "history handoff requires a target provider".into(),
                    )
                    .into());
                }
                return self.restart_runtime_agent(&resolved, force, fresh);
            }
        }
        if fresh {
            if target_provider.is_some() || handoff_history {
                return Err(AgentRestartError::BadRequest(
                    "fresh restart cannot change provider or hand off history".into(),
                )
                .into());
            }
            let snap = self.fresh_restart_snapshot(tab_id, force)?;
            return self.restart_live_agent(snap, force, grace_secs, None, RestartMode::Fresh);
        }
        if handoff_history && target_provider.is_none() {
            return Err(AgentRestartError::BadRequest(
                "history handoff requires a target provider".into(),
            )
            .into());
        }
        if let Some(target_provider) = target_provider.as_deref() {
            let current_provider = {
                let mux = self.mux.read();
                mux.resolve_tab_id(tab_id)
                    .and_then(|resolved| mux.get_tab(&resolved))
                    .and_then(|tab| tab.agent_type.clone())
            };
            let Some(current_provider) = current_provider else {
                return Err(AgentRestartError::BadRequest(
                    "provider restart requires a live PTY agent Tab".into(),
                )
                .into());
            };
            if current_provider != target_provider {
                let mut snap = self.snapshot_tab_for_restart(tab_id)?;
                self.prepare_provider_change(&mut snap, target_provider, handoff_history)?;
                return self.restart_live_agent(
                    snap,
                    force,
                    grace_secs,
                    None,
                    RestartMode::Standard,
                );
            }
        }
        if let Some(outcome) = self.restart_general_agent_if_target(tab_id, force)? {
            return outcome;
        }
        if let Some(snap) = self.failed_launch_restart_snapshot(tab_id)? {
            return self.restart_live_agent(snap, force, grace_secs, None, RestartMode::SkipExit);
        }
        if let Some(snap) = self.unverified_launch_shell_snapshot(tab_id)? {
            return self.restart_live_agent(snap, force, grace_secs, None, RestartMode::SkipExit);
        }
        if let Some(exited_tab_id) = self.absent_restart_target(tab_id)? {
            return self.recover_exited_provider(&exited_tab_id, force, false);
        }
        let snap = self.snapshot_tab_for_restart(tab_id)?;
        self.restart_live_agent(snap, force, grace_secs, None, RestartMode::Standard)
    }
}
