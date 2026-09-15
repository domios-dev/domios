//! A tab whose provider exited before shutdown restores as its shell; once that
//! shell is ready, exit recovery resumes its lineage session automatically.

use std::collections::HashSet;
use std::sync::LazyLock;

use parking_lot::Mutex;

use crate::crash::restore_report::RestorePane;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{latest_prior_session, Tab};
use crate::service::ServiceContext;
use crate::session::lifecycle::{JournalRecord, LifecycleTransition, TabLifecycleJournal};

static AUTO_RECOVERY_CLAIMS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// The lineage session exit recovery resumes for a restored tab with no agent.
pub(crate) fn exited_member_session(tab: &Tab) -> Option<&str> {
    if !tab.restored_from_snapshot
        || tab.protected
        || tab.agent_type.is_some()
        || tab.agent_session_id.is_some()
    {
        return None;
    }
    latest_prior_session(&tab.prior_session_binding_nonces)
}

fn claim_auto_recovery(claims: &Mutex<HashSet<String>>, tab_id: &str) -> bool {
    claims.lock().insert(tab_id.to_string())
}

impl ServiceContext {
    /// Resumes a restored exited tab once per process, off the boot-stage caller.
    pub(crate) fn resume_exited_member_on_shell_ready(&self, tab_id: &str) {
        let eligible = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(exited_member_session)
            .is_some();
        if !eligible || !claim_auto_recovery(&AUTO_RECOVERY_CLAIMS, tab_id) {
            return;
        }
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        let spawned = std::thread::Builder::new()
            .name("restore-exit-recovery".into())
            .spawn(move || match svc.agent_restart(&tab_id, false, 1) {
                Ok(outcome) => log::info!(
                    "[restore] resumed exited Tab {tab_id}: {} {:?}",
                    outcome.session_action(),
                    outcome.session_id
                ),
                Err(error) => {
                    log::warn!("[restore] could not resume exited Tab {tab_id}: {error:?}")
                }
            });
        if let Err(error) = spawned {
            log::warn!("[restore] exit recovery thread failed to start: {error}");
        }
    }
}

pub(crate) fn exited_members(mux: &MultiplexerState) -> Vec<(String, String)> {
    let mut members: Vec<(String, String)> = mux
        .snapshot()
        .tabs
        .values()
        .filter_map(|tab| {
            exited_member_session(tab).map(|session| (tab.id.clone(), session.to_string()))
        })
        .collect();
    members.sort();
    members
}

/// Flags the report panes from the restored state and journals each decision
/// unless the member's newest record already holds it.
pub(crate) fn apply_restore_decisions(
    journal: &TabLifecycleJournal,
    mux: &MultiplexerState,
    panes: &mut [RestorePane],
) {
    let members = exited_members(mux);
    for pane in panes.iter_mut() {
        pane.exited_session_id = members
            .iter()
            .find(|(tab_id, _)| *tab_id == pane.tab_id)
            .map(|(_, session_id)| session_id.clone());
    }
    if members.is_empty() {
        return;
    }
    let records = journal.load().records;
    for (tab_id, session_id) in members {
        if !decision_already_journaled(&records, &tab_id, &session_id) {
            crate::service::tab_lifecycle::append_minted(
                journal,
                &tab_id,
                "restore-decision",
                LifecycleTransition::RestoreAwaitingAgentRestart { session_id },
            );
        }
    }
}

fn decision_already_journaled(records: &[JournalRecord], tab_id: &str, session_id: &str) -> bool {
    let newest = records
        .iter()
        .rev()
        .find(|record| record.tab_id() == tab_id);
    matches!(
        newest,
        Some(JournalRecord::Transition { entry }) if matches!(
            &entry.transition,
            LifecycleTransition::RestoreAwaitingAgentRestart { session_id: journaled }
                if journaled == session_id
        )
    )
}

#[cfg(test)]
mod tests;
