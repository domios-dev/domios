use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, Tab, TabCloseReason};
use crate::service::ServiceContext;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::Duration;

use super::task_attempts::TaskRespawnDraft;

#[derive(Default)]
pub(crate) struct RetirementReceipts(Mutex<HashMap<(String, String), SourceIdentity>>);

#[derive(PartialEq, Eq)]
struct SourceIdentity {
    attempt: String,
    tab: String,
    session: Option<String>,
    nonce: Option<String>,
}

impl SourceIdentity {
    fn from_draft(draft: &TaskRespawnDraft) -> Self {
        Self {
            attempt: draft.source_attempt.id.clone(),
            tab: draft.source_attempt.executor_tab_id.clone(),
            session: draft.source_attempt.session_id.clone(),
            nonce: draft.source_attempt.session_binding_nonce.clone(),
        }
    }
}

impl ServiceContext {
    pub fn retire_respawn_executor(&self, draft: &TaskRespawnDraft) -> Result<(), String> {
        self.retire_respawn_executor_with(draft, |tabs| {
            for tab in tabs {
                if tab.is_chat_backend() {
                    let receipt = self.chatbackend_close_confirmed(&tab.id)?;
                    if receipt.tab_id != tab.id
                        || tab.agent_session_id.as_deref() != Some(receipt.session_id.as_str())
                    {
                        return Err("chat backend retirement receipt identity changed".into());
                    }
                }
                if let Some(pty_id) = tab.pty_id.as_deref() {
                    self.pty_ownership.release_pty(pty_id);
                    let reaper = self.pty.lock().begin_kill_tree(pty_id)?;
                    reaper.wait_for_termination_bounded(Duration::from_secs(5))?;
                }
            }
            Ok(())
        })
    }

    fn retire_respawn_executor_with(
        &self,
        draft: &TaskRespawnDraft,
        terminate: impl FnOnce(&[Tab]) -> Result<(), String>,
    ) -> Result<(), String> {
        if draft.source_attempt.mesh_peer.is_some() {
            return Err("remote respawn requires peer-owned executor retirement".into());
        }
        let tab_id = &draft.source_attempt.executor_tab_id;
        let receipts = self.extensions.get_or_init(RetirementReceipts::default);
        let key = (draft.plan_id.clone(), draft.task_id.clone());
        let identity = SourceIdentity::from_draft(draft);
        let tabs = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return if receipts.0.lock().get(&key) == Some(&identity) {
                    Ok(())
                } else {
                    Err(
                        "respawn source tab is absent without confirmed executor termination"
                            .into(),
                    )
                };
            };
            validate_source(tab, draft)?;
            closing_tabs(&mux, tab_id)
        };
        self.close_tab_with_teardown(
            tab_id,
            TabCloseReason::AgentRunEnded,
            |mux, id| validate_snapshot(mux, id, draft, &tabs),
            || terminate(&tabs),
            |mux, id| validate_snapshot(mux, id, draft, &tabs),
        )
        .map_err(|error| format!("respawn executor retirement failed: {error}"))?;
        receipts.0.lock().insert(key, identity);
        Ok(())
    }
}

fn validate_source(tab: &Tab, draft: &TaskRespawnDraft) -> Result<(), String> {
    let source = &draft.source_attempt;
    let binding = tab.task_binding.as_ref();
    if tab.id != source.executor_tab_id
        || tab.protected
        || tab.role != Some(AgentRole::Worker)
        || binding.and_then(|binding| binding.plan_id.as_deref()) != Some(draft.plan_id.as_str())
        || binding.and_then(|binding| binding.task_id.as_deref()) != Some(draft.task_id.as_str())
        || binding.and_then(|binding| binding.attempt_id.as_deref()) != Some(source.id.as_str())
        || source
            .session_id
            .as_ref()
            .is_some_and(|session| tab.agent_session_id.as_ref() != Some(session))
        || source.session_binding_nonce.as_ref().is_some_and(|nonce| {
            tab.session_binding_nonce.as_ref() != Some(nonce)
                || tab.reported_session_binding_nonce.as_ref() != Some(nonce)
        })
    {
        return Err("respawn source executor identity changed".into());
    }
    Ok(())
}

fn closing_tabs(mux: &MultiplexerState, tab_id: &str) -> Vec<Tab> {
    let Some(tab) = mux.get_tab(tab_id) else {
        return Vec::new();
    };
    let mut tabs = vec![tab.clone()];
    for child_id in mux.get_children_ids(tab_id) {
        if let Some(child) = mux.get_tab(&child_id) {
            if child.closes_with_owner()
                || (child.is_utility_shell()
                    && tab.team_id.is_some()
                    && child.team_id == tab.team_id
                    && child.spawned_by_tab_id.as_deref() == Some(tab_id))
            {
                tabs.extend(closing_tabs(mux, &child_id));
            }
        }
    }
    tabs.sort_by(|left, right| left.id.cmp(&right.id));
    tabs
}

fn validate_snapshot(
    mux: &MultiplexerState,
    tab_id: &str,
    draft: &TaskRespawnDraft,
    expected: &[Tab],
) -> Result<(), String> {
    let tab = mux
        .get_tab(tab_id)
        .ok_or("respawn source executor disappeared")?;
    validate_source(tab, draft)?;
    let current = closing_tabs(mux, tab_id);
    if current.len() != expected.len()
        || current.iter().zip(expected).any(|(current, expected)| {
            current.id != expected.id
                || current.pty_id != expected.pty_id
                || current.agent_type != expected.agent_type
                || current.agent_session_id != expected.agent_session_id
                || current.session_binding_nonce != expected.session_binding_nonce
                || current.task_binding != expected.task_binding
                || current.backing != expected.backing
        })
    {
        return Err("respawn source executor generation changed during retirement".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
