use codeterm_types::pty::RemoteOwnership;

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::Tab;
use crate::service::{session_persist, ServiceContext};

#[cfg(test)]
#[path = "ownership/tests.rs"]
mod tests;

impl ServiceContext {
    pub(crate) fn release_remote_ownership(
        &self,
        tab_id: &str,
        caller_tab_id: Option<&str>,
    ) -> Result<Option<RemoteOwnership>, String> {
        let (tab_id, ownership) = {
            let mut mux = self.mux.write();
            let tab_id = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| format!("Tab not found: {tab_id}"))?;
            let tab = mux
                .get_tab(&tab_id)
                .ok_or_else(|| format!("Tab not found: {tab_id}"))?;
            let Some(ownership) = tab.ownership.clone() else {
                return Ok(None);
            };
            authorize_remote_ownership_release(&mux, tab, &ownership, caller_tab_id)?;
            mux.get_tab_mut(&tab_id)
                .ok_or_else(|| format!("Tab not found: {tab_id}"))?
                .ownership = None;
            (tab_id, ownership)
        };

        let persistence = session_persist::write_session_snapshot(&self.app_handle, &self.mux);
        let persist_error = match persistence {
            Ok(crate::session::store::SaveOutcome::Saved) => None,
            Ok(crate::session::store::SaveOutcome::RefusedReduction(refusal)) => {
                Some(refusal.to_string())
            }
            Err(error) => Some(error),
        };
        if let Some(error) = persist_error {
            let mut mux = self.mux.write();
            if let Some(tab) = mux.get_tab_mut(&tab_id) {
                if tab.ownership.is_none() {
                    tab.ownership = Some(ownership);
                }
            }
            return Err(error);
        }

        self.record_remote_ownership_release(
            &tab_id,
            ownership.peer_id.clone(),
            ownership.peer_name.clone(),
            caller_tab_id.map(str::to_string),
        );
        self.emit_state_changed();
        log::info!(
            "[tab-ownership] tab={} remote_peer={} released",
            tab_id,
            ownership.peer_id
        );
        Ok(Some(ownership))
    }
}

fn authorize_remote_ownership_release(
    mux: &MultiplexerState,
    target: &Tab,
    ownership: &RemoteOwnership,
    caller_tab_id: Option<&str>,
) -> Result<(), String> {
    let Some(caller_tab_id) = caller_tab_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(());
    };
    if caller_tab_id == target.id {
        return Ok(());
    }
    let caller = mux
        .get_tab(caller_tab_id)
        .ok_or_else(|| format!("Caller tab {caller_tab_id} not found"))?;
    if crate::service::general_agent::is_general_agent(caller) {
        return Ok(());
    }
    let Some(team_id) = caller.team_id.as_deref() else {
        return Err(format!(
            "Caller tab {caller_tab_id} is not authorized to release remote ownership"
        ));
    };
    let qualified_remote_team_id = ownership
        .team
        .as_ref()
        .map(|team| format!("mesh:{}/{}", ownership.peer_id, team.id));
    let owns_target_team = target.team_id.as_deref() == Some(team_id)
        || ownership
            .team
            .as_ref()
            .is_some_and(|team| team_id == team.id)
        || qualified_remote_team_id.as_deref() == Some(team_id);
    if caller.has_team_orchestrator_authority() && owns_target_team {
        return Ok(());
    }
    Err(format!(
        "Caller tab {caller_tab_id} is not authorized to release remote ownership"
    ))
}
