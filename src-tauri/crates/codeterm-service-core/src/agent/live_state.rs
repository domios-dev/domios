use std::collections::BTreeMap;

use codeterm_types::pty::{classify_agent_liveness, AgentBacking, AgentLiveness, LivenessEvidence};

use crate::service::{ServiceContext, ServiceState};

mod quiescence;
mod restore_worker;
pub(crate) use quiescence::schedule;
pub(crate) use restore_worker::RestoreReportWorker;

#[cfg(test)]
#[path = "live_state/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "live_state/restore_tests.rs"]
mod restore_tests;

pub fn start(svc: ServiceState) {
    quiescence::start(svc);
}

/// Gather the observable evidence for one tab. Returns `None` when the tab does
/// not exist — a missing tab is not an agent state, it is a missing tab.
fn evidence_for_tab(svc: &ServiceContext, tab_id: &str) -> Option<LivenessEvidence> {
    let tab = svc.mux.read().get_tab(tab_id)?.clone();
    Some(evidence_for_snapshot(svc, &tab))
}

fn evidence_for_snapshot(
    svc: &ServiceContext,
    tab: &crate::multiplexer::types::Tab,
) -> LivenessEvidence {
    let backing = match &tab.backing {
        crate::multiplexer::types::TabBacking::ChatBackend { .. } => AgentBacking::ChatBackend {
            session_open: svc.tab_chat_backing(&tab.id).is_some(),
        },
        _ => AgentBacking::Pty {
            pty_live: tab
                .pty_id
                .as_ref()
                .map(|pty_id| svc.pty.lock().get_session(pty_id).is_some()),
        },
    };
    LivenessEvidence {
        declares_agent: tab.agent_type.is_some(),
        backing,
        boot_stage: tab.boot_stage,
        boot_in_flight: tab.agent_booting,
        provider_exit_observed: tab.exit_suspected_at.is_some(),
        pending_prompt: tab.pending_prompt.is_some(),
    }
}

/// THE liveness observation for one tab. `Unobserved` when the tab is gone: an
/// absent tab supports no claim about an agent process.
pub fn observe_tab_liveness(svc: &ServiceContext, tab_id: &str) -> AgentLiveness {
    evidence_for_tab(svc, tab_id)
        .map(classify_agent_liveness)
        .unwrap_or(AgentLiveness::Unobserved)
}

fn classify_restore_liveness(
    tab: &crate::multiplexer::types::Tab,
    evidence: LivenessEvidence,
) -> AgentLiveness {
    use codeterm_types::pty::BootStage;

    let terminal = !evidence.declares_agent
        || evidence.provider_exit_observed
        || (!evidence.boot_in_flight && evidence.boot_stage == Some(BootStage::SpawnFailed))
        || (!tab.restored_from_snapshot
            && !evidence.boot_in_flight
            && matches!(
                evidence.backing,
                AgentBacking::Pty {
                    pty_live: Some(false)
                }
            ));
    if terminal || matches!(evidence.backing, AgentBacking::ChatBackend { .. }) {
        return classify_agent_liveness(evidence);
    }
    if tab.restored_from_snapshot
        || !tab.launch_session_authenticated()
        || !matches!(
            evidence.boot_stage,
            Some(BootStage::ProviderReady | BootStage::AgentReady | BootStage::PtyReattached)
        )
    {
        return AgentLiveness::Unobserved;
    }
    if evidence.boot_stage == Some(BootStage::PtyReattached)
        && !evidence.boot_in_flight
        && matches!(
            evidence.backing,
            AgentBacking::Pty {
                pty_live: Some(true)
            }
        )
    {
        return AgentLiveness::Live;
    }
    classify_agent_liveness(evidence)
}

fn observe_restore_tabs(
    svc: &ServiceContext,
    tab_ids: &[String],
) -> BTreeMap<String, AgentLiveness> {
    tab_ids
        .iter()
        .map(|tab_id| {
            let tab = svc.mux.read().get_tab(tab_id).cloned();
            let observed = tab
                .as_ref()
                .map(|tab| {
                    let liveness = classify_restore_liveness(tab, evidence_for_snapshot(svc, tab));
                    log::debug!(
                        "[restore-liveness] tab={} session={:?} stage={:?} snapshot_pending={} authenticated={} liveness={}",
                        tab.id,
                        tab.agent_session_id,
                        tab.boot_stage,
                        tab.restored_from_snapshot,
                        tab.launch_session_authenticated(),
                        liveness.as_str(),
                    );
                    liveness
                })
                .unwrap_or(AgentLiveness::Unobserved);
            (tab_id.clone(), observed)
        })
        .collect()
}

impl ServiceContext {
    /// Fold fresh liveness into the armed restore wave and reconcile any pane
    /// that came back without an agent. Driven by real launch transitions
    /// (`emit_boot_stage`, PTY death), never by a timer, and a cheap no-op once
    /// the wave has settled.
    pub(crate) fn settle_restore_report(&self) {
        let tab_ids = crate::crash::restore_report::pending_tab_ids();
        if tab_ids.is_empty() {
            crate::service::supporter_notify::restore_complete(self);
            return;
        }
        let restored_ids: std::collections::BTreeSet<String> = {
            let mux = self.mux.read();
            mux.snapshot()
                .tabs
                .values()
                .filter(|tab| tab.role.is_some() || tab.agent_type.is_some())
                .map(|tab| tab.id.clone())
                .collect()
        };
        let observed = observe_restore_tabs(self, &tab_ids);
        if !crate::crash::restore_report::settle(&restored_ids, observed.clone()) {
            return;
        }
        if crate::crash::restore_report::wave_is_settled() {
            crate::service::supporter_notify::restore_complete(self);
        }
        for (tab_id, liveness) in observed {
            if liveness == AgentLiveness::Absent {
                self.reconcile_tab_restored_without_agent(&tab_id);
            }
        }
    }

    /// Clear stale identity after a caller obtains an absent verdict.
    pub fn reconcile_tab_restored_without_agent(&self, tab_id: &str) -> bool {
        let tab = self.mux.read().get_tab(tab_id).cloned();
        let can_reconcile = tab.as_ref().is_some_and(|tab| {
            tab.agent_type.is_some()
                && !tab.agent_booting
                && (!tab.restored_from_snapshot || tab.agent_session_id.is_none())
        });
        if !can_reconcile {
            return false;
        }
        if tab.as_ref().is_some_and(|tab| {
            tab.boot_stage == Some(crate::service::boot_stage::BootStage::SpawnFailed)
        }) {
            return self.clear_tab_spawn_failed_identity_if_absent(
                tab_id,
                "restored launch failed without a live agent",
            );
        }
        self.clear_tab_restored_identity_if_absent(tab_id, "restored without a live agent")
    }
}
