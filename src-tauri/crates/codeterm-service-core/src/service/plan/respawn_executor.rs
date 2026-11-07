use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, Tab, TabCloseReason};
use crate::service::ServiceContext;
use std::time::Duration;

use super::task_attempts::{ExecutorSourceFields, TaskRespawnDraft};

#[derive(Clone, Copy)]
struct ExecutorSource<'a> {
    plan_id: &'a str,
    task_id: &'a str,
    source_attempt: ExecutorSourceFields<'a>,
    allow_incomplete_binding: bool,
    /// The attempt is closed or retired, or its task is terminal: no live writer
    /// remains whose session identity must be proven before termination.
    settled: bool,
}

impl<'a> From<&'a TaskRespawnDraft> for ExecutorSource<'a> {
    fn from(draft: &'a TaskRespawnDraft) -> Self {
        Self {
            plan_id: &draft.plan_id,
            task_id: &draft.task_id,
            source_attempt: (&draft.source_attempt).into(),
            allow_incomplete_binding: false,
            settled: false,
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
        let projected = match (binding.plan_id.as_deref(), binding.task_id.as_deref()) {
            (Some(plan), Some(task)) => match self.plan_store.executor_source_projection(
                plan,
                task,
                binding.attempt_id.as_deref().unwrap_or_default(),
            ) {
                Err(crate::service::plan::PlanError::NotFound) => {
                    return self.close_orphan_worker_executor(&tab, reason, "plan_gone")
                }
                Err(crate::service::plan::PlanError::TaskNotFound) => {
                    return self.close_orphan_worker_executor(&tab, reason, "task_gone")
                }
                result => result.map_err(|error| error.to_string())?,
            },
            _ => None,
        };
        if binding.attempt_id.is_none() {
            let candidates = self
                .plan_store
                .executor_sources_for_tab_projection(&tab.id)
                .map_err(|error| error.to_string())?;
            let candidate = match candidates.as_slice() {
                [] if binding.plan_id.is_some() && binding.task_id.is_some() => {
                    return self.close_terminal_worker_executor(&tab, reason);
                }
                [] if binding.plan_id.is_none() && binding.task_id.is_none() => return Ok(false),
                [candidate] => candidate,
                _ => return Err("executor current attempt is unavailable or ambiguous".into()),
            };
            self.retire_bound_executor(
                &tab,
                ExecutorSource {
                    plan_id: &candidate.0,
                    task_id: &candidate.1,
                    source_attempt: (&candidate.2).into(),
                    allow_incomplete_binding: true,
                    settled: false,
                },
                reason,
            )?;
            return Ok(true);
        }
        let (Some(plan_id), Some(task_id)) = (binding.plan_id, binding.task_id) else {
            return Err("executor task binding is incomplete".into());
        };
        let attempt = projected.ok_or("executor attempt is unavailable")?;
        let settled = self.executor_attempt_settled(&plan_id, &task_id, &attempt.id)?;
        self.retire_bound_executor(
            &tab,
            ExecutorSource {
                plan_id: &plan_id,
                task_id: &task_id,
                source_attempt: (&attempt).into(),
                allow_incomplete_binding: false,
                settled,
            },
            reason,
        )?;
        Ok(true)
    }

    fn executor_attempt_settled(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
    ) -> Result<bool, String> {
        if self
            .plan_store
            .task_status_projection(plan_id, task_id)
            .map_err(|error| error.to_string())?
            .is_terminal()
        {
            return Ok(true);
        }
        Ok(self
            .plan_store
            .list_task_attempts(plan_id, task_id)
            .map_err(|error| error.to_string())?
            .iter()
            .any(|view| {
                view.attempt.id == attempt_id
                    && (view.attempt.closed_at.is_some() || view.attempt.retired_at.is_some())
            }))
    }

    /// A settled attempt closes without its writer's session proof, but never
    /// while the plan still assigns this tab any other live attempt.
    fn retire_bound_executor(
        &self,
        tab: &Tab,
        source: ExecutorSource<'_>,
        reason: TabCloseReason,
    ) -> Result<(), String> {
        if source.settled {
            let live = self
                .plan_store
                .executor_sources_for_tab_projection(&tab.id)
                .map_err(|error| error.to_string())?;
            if let Some((_, task, attempt)) = live.iter().find(|(plan, task, attempt)| {
                (plan.as_str(), task.as_str(), attempt.id.as_str())
                    != (source.plan_id, source.task_id, source.source_attempt.id)
            }) {
                return Err(format!(
                    "Worker Tab {} still executes live attempt {} of task {task}; settle that task before closing the tab",
                    tab.id, attempt.id
                ));
            }
        }
        validate_source(tab, source).map_err(|_| {
            let status = self
                .plan_store
                .task_status_projection(source.plan_id, source.task_id)
                .ok();
            executor_close_refusal(tab, source, status.as_ref())
        })?;
        self.retire_executor_with(source, reason, |tabs| self.terminate_executor_tabs(tabs))
    }

    fn close_terminal_worker_executor(
        &self,
        tab: &Tab,
        reason: TabCloseReason,
    ) -> Result<bool, String> {
        let binding = tab
            .task_binding
            .as_ref()
            .ok_or("executor task binding is missing")?;
        let plan_id = binding
            .plan_id
            .as_deref()
            .ok_or("executor plan binding is missing")?;
        let task_id = binding
            .task_id
            .as_deref()
            .ok_or("executor task binding is missing")?;
        let attempts = match self.plan_store.list_task_attempts(plan_id, task_id) {
            Err(crate::service::plan::PlanError::NotFound) => {
                return self.close_orphan_worker_executor(tab, reason, "plan_gone")
            }
            Err(crate::service::plan::PlanError::TaskNotFound) => {
                return self.close_orphan_worker_executor(tab, reason, "task_gone")
            }
            result => result.map_err(|error| error.to_string())?,
        };
        let candidates = attempts
            .iter()
            .filter(|view| {
                view.attempt.executor_tab_id == tab.id && view.attempt.closed_at.is_some()
            })
            .collect::<Vec<_>>();
        let [candidate] = candidates.as_slice() else {
            return Err("executor terminal attempt is unavailable or ambiguous".into());
        };
        self.retire_bound_executor(
            tab,
            ExecutorSource {
                plan_id,
                task_id,
                source_attempt: (&candidate.attempt).into(),
                allow_incomplete_binding: true,
                settled: true,
            },
            reason,
        )?;
        Ok(true)
    }

    fn close_orphan_worker_executor(
        &self,
        tab: &Tab,
        reason: TabCloseReason,
        cause: &str,
    ) -> Result<bool, String> {
        if !self
            .plan_store
            .executor_sources_for_tab_projection(&tab.id)
            .map_err(|error| error.to_string())?
            .is_empty()
        {
            return Err("executor orphan binding conflicts with a live plan/task attempt".into());
        }
        let tabs = closing_tabs(&self.mux.read(), &tab.id);
        let validate = |mux: &MultiplexerState, id: &str| {
            let current = mux.get_tab(id).ok_or("orphan executor disappeared")?;
            if current.role != tab.role
                || current.task_binding != tab.task_binding
                || current.lifecycle_epoch != tab.lifecycle_epoch
                || current.lifecycle_operation_id != tab.lifecycle_operation_id
                || current.agent_type != tab.agent_type
                || current.agent_session_id != tab.agent_session_id
                || current.session_binding_nonce != tab.session_binding_nonce
                || current.reported_session_binding_nonce != tab.reported_session_binding_nonce
                || current.pty_id != tab.pty_id
                || current.backing != tab.backing
                || current.protected != tab.protected
            {
                return Err(
                    "orphan executor identity or assignment changed during close".to_string(),
                );
            }
            validate_closing_tabs(mux, id, &tabs)
        };
        self.close_tab_with_teardown(
            &tab.id,
            reason,
            validate,
            || self.terminate_executor_tabs(&tabs),
            |mux, id| {
                validate(mux, id)?;
                let current = mux.get_tab_mut(id).ok_or("orphan executor disappeared")?;
                release_orphan_binding(current, cause);
                Ok(())
            },
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
        let tab_id = draft.source_attempt.executor_tab_id;
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
        let missing_owner = std::cell::Cell::new(None);
        self.close_tab_with_teardown(
            tab_id,
            reason,
            |mux, id| validate_snapshot(mux, id, draft, &tabs),
            || {
                terminate(&tabs)?;
                validate_snapshot(&self.mux.read(), tab_id, draft, &tabs)?;
                missing_owner.set(self.record_executor_termination(draft)?);
                Ok(())
            },
            |mux, id| {
                validate_snapshot(mux, id, draft, &tabs)?;
                if let Some(cause) = missing_owner.get() {
                    let tab = mux
                        .get_tab_mut(id)
                        .ok_or("executor disappeared during close")?;
                    release_orphan_binding(tab, cause);
                }
                Ok(())
            },
        )
        .map_err(|error| format!("respawn executor retirement failed: {error}"))?;
        Ok(())
    }

    fn record_executor_termination(
        &self,
        draft: ExecutorSource<'_>,
    ) -> Result<Option<&'static str>, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis() as u64;
        match self.plan_store.record_task_executor_termination(
            draft.plan_id,
            draft.task_id,
            draft.source_attempt,
            now,
        ) {
            Ok(()) => Ok(None),
            Err(crate::service::plan::PlanError::NotFound) => Ok(Some("plan_gone")),
            Err(crate::service::plan::PlanError::TaskNotFound) => Ok(Some("task_gone")),
            Err(error) => Err(format!(
                "executor terminated but termination evidence could not be persisted: {error}"
            )),
        }
    }
}

fn release_orphan_binding(tab: &mut Tab, cause: &str) {
    log::debug!(
        "[executor-retirement] tab={} binding_released=true cause={cause} plan={:?} task={:?}",
        tab.id,
        tab.task_binding
            .as_ref()
            .and_then(|binding| binding.plan_id.as_deref()),
        tab.task_binding
            .as_ref()
            .and_then(|binding| binding.task_id.as_deref())
    );
    tab.task_binding = None;
    tab.worker_id = None;
    tab.restorable_launch.worker_restart = None;
}

fn validate_source(tab: &Tab, draft: ExecutorSource<'_>) -> Result<(), String> {
    if tab.protected || executor_identity_drift(tab, draft).is_some() {
        return Err("respawn source executor identity changed".into());
    }
    Ok(())
}

/// Which identity proof a tab fails for an attempt it claims to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutorIdentityDrift {
    Assignment,
    Session,
    LaunchUnreported,
    LaunchRotated,
}

fn executor_identity_drift(tab: &Tab, draft: ExecutorSource<'_>) -> Option<ExecutorIdentityDrift> {
    let source = draft.source_attempt;
    let binding = tab.task_binding.as_ref();
    if tab.id != source.executor_tab_id
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
            source.id,
            draft.allow_incomplete_binding,
        )
    {
        return Some(ExecutorIdentityDrift::Assignment);
    }
    if draft.settled {
        return None;
    }
    if source
        .session_id
        .is_some_and(|session| tab.agent_session_id.as_deref() != Some(session))
    {
        return Some(ExecutorIdentityDrift::Session);
    }
    source_nonce_drift(tab, source)
}

fn executor_close_refusal(
    tab: &Tab,
    draft: ExecutorSource<'_>,
    task_status: Option<&crate::plan::TaskStatus>,
) -> String {
    let (attempt, task) = (draft.source_attempt.id, draft.task_id);
    if tab.protected {
        return format!("Tab {} is protected; unprotect it before closing", tab.id);
    }
    if draft.settled {
        return format!("Worker Tab {} no longer holds finished attempt {attempt} of task {task}; list the tab again before closing it", tab.id);
    }
    let drift = executor_identity_drift(tab, draft).unwrap_or(ExecutorIdentityDrift::Assignment);
    let cause = match drift {
        ExecutorIdentityDrift::Assignment => "its task binding no longer names that attempt".to_string(),
        ExecutorIdentityDrift::Session => format!(
            "the tab is bound to session {} while the attempt recorded session {}",
            tab.agent_session_id.as_deref().unwrap_or("(none)"),
            draft.source_attempt.session_id.unwrap_or("(none)")
        ),
        ExecutorIdentityDrift::LaunchUnreported => "its current launch has not reported its binding nonce, so the tab cannot be proven to be that attempt's writer".to_string(),
        ExecutorIdentityDrift::LaunchRotated => "its launch binding changed since the attempt recorded it and no same-session restoration links the two".to_string(),
    };
    if task_status == Some(&crate::plan::TaskStatus::InReview) {
        format!("Worker Tab {} holds live attempt {attempt} of task {task}, which is in review, and {cause}; accept, bounce or cancel the task before closing the tab", tab.id)
    } else {
        format!("Worker Tab {} still runs live attempt {attempt} of task {task} and {cause}; settle the task (report, cancel or reassign) before closing the tab", tab.id)
    }
}

fn binding_field_matches(actual: Option<&str>, expected: &str, allow_missing: bool) -> bool {
    actual.map_or(allow_missing, |actual| actual == expected)
}

fn source_nonce_drift<'a>(
    tab: &Tab,
    source: impl Into<ExecutorSourceFields<'a>>,
) -> Option<ExecutorIdentityDrift> {
    let source = source.into();
    let old_nonce = source.session_binding_nonce?;
    let Some(current_nonce) = tab
        .session_binding_nonce
        .as_deref()
        .filter(|current| tab.reported_session_binding_nonce.as_deref() == Some(*current))
    else {
        return Some(ExecutorIdentityDrift::LaunchUnreported);
    };
    if current_nonce == old_nonce {
        return None;
    }
    let restored = source.session_id.is_some_and(|session| {
        source.provider.is_some()
            && source.provider == tab.agent_type.as_deref()
            && tab.agent_session_id.as_deref() == Some(session)
            && crate::service::worker_identity::restore_binding::prove_same_session_restoration(
                &tab.prior_session_binding_nonces,
                old_nonce,
                current_nonce,
                session,
            )
            .is_ok()
    });
    (!restored).then_some(ExecutorIdentityDrift::LaunchRotated)
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
    validate_closing_tabs(mux, tab_id, expected)
}

fn validate_closing_tabs(
    mux: &MultiplexerState,
    tab_id: &str,
    expected: &[Tab],
) -> Result<(), String> {
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
