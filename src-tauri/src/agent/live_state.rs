//! Two things live here, both about what is actually running in a tab.
//!
//! [`observe_tab_liveness`] is THE observation: whether an agent process is
//! live behind a tab. It reads only facts — a PtyManager session that exists, a
//! plugin session binding, a boot stage emitted at a real launch transition, a
//! shell that reported its foreground command finished — and hands them to the
//! pure classifier in `codeterm_types::pty::agent_liveness`. Restore reporting,
//! record reconciliation, lifecycle teardown and every state label read it.
//!
//! `start` is the pre-existing live 3-state sweep. `process_pty_output`
//! recomputes a tab's agent state on every output chunk, but the working→idle
//! edge has no output to ride on: when a turn ends the agent goes silent, so a
//! periodic sweep is what lets the quiescence clocks cross their thresholds.
//! Pairs with the output-driven path through the shared
//! `ServiceContext::recompute_agent_state`.

use std::collections::BTreeMap;
use std::time::Duration;

use codeterm_types::pty::{classify_agent_liveness, AgentBacking, AgentLiveness, LivenessEvidence};

use crate::service::{ServiceContext, ServiceState};

#[cfg(test)]
#[path = "live_state/tests.rs"]
mod tests;

/// How often the sweep re-runs the classifier per tab. Fast enough that the
/// "your turn" indicator appears promptly after a turn settles (DONE_MS = 800),
/// cheap enough to run over every tracked pty (one grid read + a pure classify).
const POLL_INTERVAL: Duration = Duration::from_millis(350);

/// Spawn the live-state sweep on its own thread. Each tick re-classifies every
/// tracked tab; `recompute_agent_state` gates to agent tabs and emits only on
/// a real change.
pub fn start(svc: ServiceState) {
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL_INTERVAL);
        let pty_ids = svc.agent_tracker.lock().all_ids();
        for pty_id in pty_ids {
            svc.recompute_agent_state(&pty_id);
            svc.run_watcher.complete_via_semantic_idle(&svc, &pty_id);
        }
    });
}

/// Gather the observable evidence for one tab. Returns `None` when the tab does
/// not exist — a missing tab is not an agent state, it is a missing tab.
fn evidence_for_tab(svc: &ServiceContext, tab_id: &str) -> Option<LivenessEvidence> {
    let (declares_agent, pty_id, chat_bound, boot_stage, boot_in_flight, exit_observed) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id)?;
        (
            tab.agent_type.is_some(),
            tab.pty_id.clone(),
            matches!(
                tab.backing,
                crate::multiplexer::types::TabBacking::ChatBackend { .. }
            ),
            tab.boot_stage,
            tab.agent_booting,
            tab.exit_suspected_at.is_some(),
        )
    };
    let backing = if chat_bound {
        AgentBacking::ChatBackend {
            session_open: svc.tab_chat_backing(tab_id).is_some(),
        }
    } else {
        AgentBacking::Pty {
            pty_live: pty_id.map(|pty_id| svc.pty.lock().get_session(&pty_id).is_some()),
        }
    };
    Some(LivenessEvidence {
        declares_agent,
        backing,
        boot_stage,
        boot_in_flight,
        provider_exit_observed: exit_observed,
    })
}

/// THE liveness observation for one tab. `Unobserved` when the tab is gone: an
/// absent tab supports no claim about an agent process.
pub fn observe_tab_liveness(svc: &ServiceContext, tab_id: &str) -> AgentLiveness {
    evidence_for_tab(svc, tab_id)
        .map(classify_agent_liveness)
        .unwrap_or(AgentLiveness::Unobserved)
}

/// Observe every tab in `tab_ids` in one pass, for callers (restore reporting)
/// that need a whole wave's verdict at once.
pub fn observe_tabs(svc: &ServiceContext, tab_ids: &[String]) -> BTreeMap<String, AgentLiveness> {
    tab_ids
        .iter()
        .map(|tab_id| (tab_id.clone(), observe_tab_liveness(svc, tab_id)))
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
        let observed = observe_tabs(self, &tab_ids);
        if !crate::crash::restore_report::settle(&restored_ids, observed.clone()) {
            return;
        }
        for (tab_id, liveness) in observed {
            if liveness == AgentLiveness::Absent {
                self.reconcile_tab_restored_without_agent(&tab_id);
            }
        }
    }

    /// Restore teardown is permitted only when the tab has no live PTY and no
    /// launch in flight; either fact makes an older wave verdict stale.
    pub(crate) fn reconcile_tab_restored_without_agent(&self, tab_id: &str) -> bool {
        let declares_agent = self
            .mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| tab.agent_type.is_some());
        if !declares_agent {
            return false;
        }
        self.clear_tab_restored_identity_if_absent(tab_id, "restored without a live agent")
    }
}
