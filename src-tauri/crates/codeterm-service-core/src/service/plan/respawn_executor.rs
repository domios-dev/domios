use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, Tab, TabCloseReason};
use crate::service::ServiceContext;
use std::time::Duration;

use super::task_attempts::TaskRespawnDraft;

#[derive(Clone, Copy)]
struct ExecutorSource<'a> {
    plan_id: &'a str,
    task_id: &'a str,
    source_attempt: &'a crate::plan::TaskExecutionAttempt,
    allow_incomplete_binding: bool,
}

impl<'a> From<&'a TaskRespawnDraft> for ExecutorSource<'a> {
    fn from(draft: &'a TaskRespawnDraft) -> Self {
        Self {
            plan_id: &draft.plan_id,
            task_id: &draft.task_id,
            source_attempt: &draft.source_attempt,
            allow_incomplete_binding: false,
        }
    }
}

impl ServiceContext {
    pub fn retire_respawn_executor(&self, draft: &TaskRespawnDraft) -> Result<(), String> {
        self.retire_executor_with(draft.into(), TabCloseReason::AgentRunEnded, |tabs| {
            self.terminate_executor_tabs(tabs)
        })
    }

    pub(crate) fn close_bound_worker_executor(
        &self,
        tab_id: &str,
        reason: TabCloseReason,
    ) -> Result<bool, String> {
        let tab = {
            let mux = self.mux.read();
            let canonical = mux
                .resolve_tab_id(tab_id)
                .unwrap_or_else(|| tab_id.to_string());
            mux.get_tab(&canonical).cloned()
        };
        let Some(tab) = tab.filter(|tab| tab.role == Some(AgentRole::Worker)) else {
            return Ok(false);
        };
        let binding = tab.task_binding.clone().unwrap_or_default();
        if binding.attempt_id.is_none() {
            let candidates = self.plan_store.live_task_attempts_for_tab(&tab.id);
            let candidate = match candidates.as_slice() {
                [] if binding.plan_id.is_none() && binding.task_id.is_none() => return Ok(false),
                [candidate] => candidate,
                _ => return Err("executor current attempt is unavailable or ambiguous".into()),
            };
            self.retire_executor_with(
                ExecutorSource {
                    plan_id: &candidate.plan_id,
                    task_id: &candidate.task_id,
                    source_attempt: &candidate.attempt,
                    allow_incomplete_binding: true,
                },
                reason,
                |tabs| self.terminate_executor_tabs(tabs),
            )?;
            return Ok(true);
        }
        let (Some(plan_id), Some(task_id), Some(attempt_id)) =
            (binding.plan_id, binding.task_id, binding.attempt_id)
        else {
            return Err("executor task binding is incomplete".into());
        };
        let plan = self
            .plan_store
            .get(&plan_id)
            .ok_or("executor plan is unavailable")?;
        let task = crate::plan::task_key::find_task(&plan.tasks, &task_id)
            .ok_or("executor task is unavailable")?;
        let attempt = task
            .execution_attempts
            .iter()
            .find(|attempt| attempt.id == attempt_id)
            .ok_or("executor attempt is unavailable")?;
        self.retire_executor_with(
            ExecutorSource {
                plan_id: &plan_id,
                task_id: &task_id,
                source_attempt: attempt,
                allow_incomplete_binding: false,
            },
            reason,
            |tabs| self.terminate_executor_tabs(tabs),
        )?;
        Ok(true)
    }

    fn terminate_executor_tabs(&self, tabs: &[Tab]) -> Result<(), String> {
        for tab in tabs {
            if tab.is_chat_backend() {
                let receipt = self.chatbackend_close_confirmed(&tab.id)?;
                if receipt.tab_id != tab.id
                    || tab.agent_session_id.as_deref() != Some(receipt.session_id.as_str())
                    || tab.session_binding_nonce != receipt.binding_nonce
                {
                    return Err("chat backend retirement receipt identity changed".into());
                }
                log::debug!(
                    "[executor-retirement] tab={} session={} evidence={:?}",
                    receipt.tab_id,
                    receipt.session_id,
                    receipt.evidence
                );
            }
            if let Some(pty_id) = tab.pty_id.as_deref() {
                self.pty_ownership.release_pty(pty_id);
                let reaper = self.pty.lock().begin_kill_tree(pty_id)?;
                reaper.wait_for_termination_bounded(Duration::from_secs(5))?;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn retire_respawn_executor_with(
        &self,
        draft: &TaskRespawnDraft,
        terminate: impl FnOnce(&[Tab]) -> Result<(), String>,
    ) -> Result<(), String> {
        self.retire_executor_with(draft.into(), TabCloseReason::AgentRunEnded, terminate)
    }

    fn retire_executor_with(
        &self,
        draft: ExecutorSource<'_>,
        reason: TabCloseReason,
        terminate: impl FnOnce(&[Tab]) -> Result<(), String>,
    ) -> Result<(), String> {
        if draft.source_attempt.mesh_peer.is_some() {
            return Err("remote respawn requires peer-owned executor retirement".into());
        }
        let tab_id = &draft.source_attempt.executor_tab_id;
        let tabs = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .map(|tab| {
                    validate_source(tab, draft)?;
                    Ok::<_, String>(closing_tabs(&mux, tab_id))
                })
                .transpose()?
        };
        let Some(tabs) = tabs else {
            return if self
                .plan_store
                .task_executor_termination_confirmed(
                    draft.plan_id,
                    draft.task_id,
                    draft.source_attempt,
                )
                .map_err(|error| error.to_string())?
            {
                Ok(())
            } else {
                Err("respawn source tab is absent without confirmed executor termination".into())
            };
        };
        self.close_tab_with_teardown(
            tab_id,
            reason,
            |mux, id| validate_snapshot(mux, id, draft, &tabs),
            || terminate(&tabs),
            |mux, id| {
                validate_snapshot(mux, id, draft, &tabs)?;
                self.record_executor_termination(draft)
            },
        )
        .map_err(|error| format!("respawn executor retirement failed: {error}"))?;
        Ok(())
    }

    fn record_executor_termination(&self, draft: ExecutorSource<'_>) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis() as u64;
        self.plan_store
            .record_task_executor_termination(
                draft.plan_id,
                draft.task_id,
                draft.source_attempt,
                now,
            )
            .map_err(|error| {
                format!(
                    "executor terminated but termination evidence could not be persisted: {error}"
                )
            })?;
        Ok(())
    }
}

fn validate_source(tab: &Tab, draft: ExecutorSource<'_>) -> Result<(), String> {
    let source = &draft.source_attempt;
    let binding = tab.task_binding.as_ref();
    if tab.id != source.executor_tab_id
        || tab.protected
        || tab.role != Some(AgentRole::Worker)
        || !binding_field_matches(
            binding.and_then(|binding| binding.plan_id.as_deref()),
            draft.plan_id,
            draft.allow_incomplete_binding,
        )
        || !binding_field_matches(
            binding.and_then(|binding| binding.task_id.as_deref()),
            draft.task_id,
            draft.allow_incomplete_binding,
        )
        || !binding_field_matches(
            binding.and_then(|binding| binding.attempt_id.as_deref()),
            &source.id,
            draft.allow_incomplete_binding,
        )
        || source
            .session_id
            .as_ref()
            .is_some_and(|session| tab.agent_session_id.as_ref() != Some(session))
        || !source_nonce_matches(tab, source)
    {
        return Err("respawn source executor identity changed".into());
    }
    Ok(())
}

fn binding_field_matches(actual: Option<&str>, expected: &str, allow_missing: bool) -> bool {
    actual.map_or(allow_missing, |actual| actual == expected)
}

fn source_nonce_matches(tab: &Tab, source: &crate::plan::TaskExecutionAttempt) -> bool {
    let Some(old_nonce) = source.session_binding_nonce.as_deref() else {
        return true;
    };
    let Some(current_nonce) = tab.session_binding_nonce.as_deref() else {
        return false;
    };
    if tab.reported_session_binding_nonce.as_deref() != Some(current_nonce) {
        return false;
    }
    if current_nonce == old_nonce {
        return true;
    }
    let Some(session) = source.session_id.as_deref() else {
        return false;
    };
    source.provider.is_some()
        && source.provider == tab.agent_type
        && tab.agent_session_id.as_deref() == Some(session)
        && crate::service::worker_identity::restore_binding::prove_same_session_restoration(
            &tab.prior_session_binding_nonces,
            old_nonce,
            current_nonce,
            session,
        )
        .is_ok()
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
    draft: ExecutorSource<'_>,
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
