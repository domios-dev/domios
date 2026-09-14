//! A team member whose provider exited before shutdown restores as its shell;
//! `agent restart` resumes its lineage session through exit recovery.

use crate::crash::restore_report::RestorePane;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{latest_prior_session, Tab};
use crate::session::lifecycle::{JournalRecord, LifecycleTransition, TabLifecycleJournal};

/// The lineage session `agent restart` recovers for a restored member with no agent.
pub(crate) fn exited_member_session(tab: &Tab) -> Option<&str> {
    if !tab.restored_from_snapshot
        || tab.protected
        || tab.role.is_none()
        || tab.agent_type.is_some()
        || tab.agent_session_id.is_some()
    {
        return None;
    }
    latest_prior_session(&tab.prior_session_binding_nonces)
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
