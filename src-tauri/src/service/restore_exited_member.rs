//! A team member whose provider exited to its shell before shutdown restores
//! as that shell. Restore relaunches nothing; `agent restart` resumes the
//! lineage session through exit recovery, which verifies it against the journal.

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{latest_prior_session, Tab};
use crate::session::lifecycle::LifecycleTransition;

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

/// Runs once on the freshly restored state, before any launch can touch a tab.
pub(crate) fn journal_restore_decisions(mux: &MultiplexerState) {
    for (tab_id, session_id) in exited_members(mux) {
        log::warn!(
            "[restore] tab={tab_id} provider exited before shutdown; restored as a shell, recover session {session_id} with `codeterm agent restart --tab {tab_id}`"
        );
        crate::service::tab_lifecycle::append_minted(
            &tab_id,
            "restore-decision",
            LifecycleTransition::RestoreAwaitingAgentRestart { session_id },
        );
    }
}

#[cfg(test)]
mod tests;
