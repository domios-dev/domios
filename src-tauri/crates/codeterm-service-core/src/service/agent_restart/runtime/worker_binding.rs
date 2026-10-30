use crate::multiplexer::types::TaskBinding;
use crate::service::agent_runtime::plan_warning::PlanBindingFailure;
use crate::service::{
    plan::task_attempts::{RuntimeRestartBinding, RuntimeRestartMode},
    ServiceContext,
};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Weak},
};

type Mux = crate::lock_hold::TimedRwLock<crate::multiplexer::state::MultiplexerState>;

#[derive(Clone)]
pub(crate) struct RestartBindingAuthority {
    owner: Weak<Mux>,
    request: RuntimeRestartBinding,
    carried: bool,
    completed_task: Option<crate::plan::Task>,
    completed_source: Option<CurrentSession>,
    completed_binding: Option<TaskBinding>,
    completed_target: Option<CurrentSession>,
}
static BINDINGS: LazyLock<Mutex<HashMap<(usize, String), RestartBindingAuthority>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
// Keep BINDINGS -> COMPLETED_SESSIONS -> mux -> runtime ordering to prevent lifecycle deadlocks.

#[derive(Clone)]
struct RuntimeGeneration {
    session: Weak<crate::service::agent_runtime::engine::Session>,
    process: Weak<crate::service::agent_runtime::process::Process>,
    epoch: u64,
}

#[derive(Clone)]
struct CurrentSession {
    tab_id: String,
    provider: String,
    thread: String,
    nonce: String,
    lifecycle_epoch: Option<u64>,
    pty_id: Option<String>,
    runtime: Option<RuntimeGeneration>,
}

impl CurrentSession {
    fn capture(tab: &crate::multiplexer::types::Tab) -> Result<Self, String> {
        let provider = tab.agent_type.clone().filter(|id| !id.is_empty())
            .ok_or("Completed continuity provider absent")?;
        let thread = tab.agent_session_id.clone().filter(|id| !id.is_empty())
            .ok_or("Completed continuity session absent")?;
        let nonce = tab.session_binding_nonce.clone().filter(|id| !id.is_empty())
            .ok_or("Completed continuity nonce absent")?;
        if tab.reported_session_binding_nonce.as_ref() != Some(&nonce) {
            return Err("Completed continuity session unverified".into());
        }
        let runtime = if matches!(&tab.backing, crate::multiplexer::types::TabBacking::ChatBackend { .. }) {
            let session = crate::service::agent_runtime::engine::for_tab(&tab.id)
                .ok_or("Completed continuity runtime absent")?;
            let context = session.context.lock().clone();
            if session.provider != provider || context.tab_id != tab.id
                || context.thread_id.as_ref() != Some(&thread) || context.binding_nonce != nonce
            {
                return Err("Completed continuity runtime changed".into());
            }
            let process = session.process.lock().as_ref().map(Arc::downgrade)
                .ok_or("Completed continuity runtime process absent")?;
            Some(RuntimeGeneration { session: Arc::downgrade(&session), process, epoch: context.epoch })
        } else if matches!(&tab.backing, crate::multiplexer::types::TabBacking::Pty) {
            if tab.pty_id.as_deref().is_none_or(str::is_empty) || tab.lifecycle_epoch.is_none() {
                return Err("completed_continuity_pty_generation_missing: wake or resume this tab into a verified live PTY, then retry the provider handoff".into());
            }
            None
        } else {
            return Err("Completed continuity transport unavailable".into());
        };
        Ok(Self {
            tab_id: tab.id.clone(), provider, thread, nonce, lifecycle_epoch: tab.lifecycle_epoch,
            pty_id: tab.pty_id.clone(), runtime,
        })
    }

    fn matches(&self, tab: &crate::multiplexer::types::Tab) -> bool {
        let Ok(current) = Self::capture(tab) else {
            return false;
        };
        let runtime_matches = match (&self.runtime, &current.runtime) {
            (None, None) => true,
            (Some(expected), Some(actual)) => {
                expected.epoch == actual.epoch
                    && expected.session.upgrade().zip(actual.session.upgrade())
                    .is_some_and(|(expected, actual)| Arc::ptr_eq(&expected, &actual))
                    && expected.process.upgrade().zip(actual.process.upgrade())
                    .is_some_and(|(expected, actual)| Arc::ptr_eq(&expected, &actual))
            }
            _ => false,
        };
        self.tab_id == current.tab_id && self.provider == current.provider && self.thread == current.thread
            && self.nonce == current.nonce && self.lifecycle_epoch == current.lifecycle_epoch
            && self.pty_id == current.pty_id && runtime_matches
    }
}

#[derive(Clone)]
struct CompletedSession {
    owner: Weak<Mux>,
    plan_id: String,
    task: crate::plan::Task,
    binding: TaskBinding,
    current: CurrentSession,
}

static COMPLETED_SESSIONS: LazyLock<Mutex<HashMap<(usize, String), CompletedSession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn prune_completed_sessions(sessions: &mut HashMap<(usize, String), CompletedSession>) {
    sessions.retain(|(_, tab), proof| {
        proof.owner.upgrade().is_some_and(|owner| owner.read().get_tab(tab).is_some())
    });
}

fn key(svc: &ServiceContext, tab: &str) -> (usize, String) {
    (Arc::as_ptr(&svc.mux) as usize, tab.into())
}

fn completed_attempt_matches(
    tab: &crate::multiplexer::types::Tab,
    attempt: &crate::plan::TaskExecutionAttempt,
    source: &str,
    nonce: &str,
) -> bool {
    !source.is_empty()
        && !nonce.is_empty()
        && tab.agent_type.as_deref() == Some(source)
        && tab.session_binding_nonce.as_deref() == Some(nonce)
        && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
        && attempt.executor_tab_id == tab.id
        && attempt.closed_at.is_some()
        && attempt.retired_at.is_none()
        && attempt.acknowledged_at.is_some()
        && attempt.provider.as_deref() == Some(source)
        && attempt.session_id == tab.agent_session_id
        && attempt.session_id.as_deref().is_some_and(|id| !id.is_empty())
        && attempt.session_binding_nonce.as_deref() == Some(nonce)
}

impl ServiceContext {
    #[cfg(test)]
    pub(crate) fn forget_completed_session_for_test(&self, tab_id: &str) {
        COMPLETED_SESSIONS.lock().remove(&key(self, tab_id));
    }

    pub(crate) fn validate_completed_restart_binding(
        &self,
        tab_id: &str,
        authority: Option<&RestartBindingAuthority>,
    ) -> Result<(), String> {
        if authority.is_some_and(|authority| authority.completed_task.is_some()) {
            self.validate_provider_switch_binding(tab_id, authority)?;
        }
        Ok(())
    }

    fn retained_completed_session_matches(
        &self,
        tab_id: &str,
        thread: &str,
        nonce: &str,
    ) -> Result<bool, PlanBindingFailure> {
        let proof = {
            let mut sessions = COMPLETED_SESSIONS.lock();
            prune_completed_sessions(&mut sessions);
            sessions.get(&key(self, tab_id)).cloned()
        };
        let Some(proof) = proof else {
            return Ok(false);
        };
        let tab = self.mux.read().get_tab(tab_id).cloned();
        if tab.as_ref().is_none_or(|tab| {
            tab.task_binding.as_ref() != Some(&proof.binding)
        }) {
            COMPLETED_SESSIONS.lock().remove(&key(self, tab_id));
            return Ok(false);
        }
        let tab = tab.ok_or("Completed continuity tab disappeared")?;
        let exact = proof.owner.upgrade().is_some_and(|owner| Arc::ptr_eq(&owner, &self.mux))
            && proof.current.thread == thread && proof.current.nonce == nonce
            && proof.current.matches(&tab)
            && self.plan_store.show_task(&proof.task.id).is_some_and(|detail| {
                detail.plan_id == proof.plan_id && detail.task == proof.task
            })
            && self.plan_store.live_task_attempts_for_tab(tab_id).is_empty();
        if !exact {
            return Err("Completed current-session continuity changed".into());
        }
        Ok(true)
    }

    pub(crate) fn validate_provider_switch_binding(
        &self,
        tab_id: &str,
        authority: Option<&RestartBindingAuthority>,
    ) -> Result<(), String> {
        let Some(authority) = authority else {
            let bound = self.mux.read().get_tab(tab_id).is_none_or(|tab| {
                super::super::restart_carries_task_attempt(tab.task_binding.as_ref())
            });
            if bound || !self.plan_store.live_task_attempts_for_tab(tab_id).is_empty() {
                return Err("provider_switch_binding_refused: source task binding changed".into());
            }
            return Ok(());
        };
        let request = &authority.request;
        if request.executor_tab_id != tab_id
            || !authority.owner.upgrade().is_some_and(|owner| Arc::ptr_eq(&owner, &self.mux))
        {
            return Err("provider_switch_binding_refused: source owner changed".into());
        }
        let tab = self.mux.read().get_tab(tab_id).cloned()
            .ok_or("provider_switch_binding_refused: tab disappeared")?;
        let detail = self.plan_store.show_task(&request.task_id)
            .ok_or("provider_switch_binding_refused: task disappeared")?;
        let exact = detail.plan_id == request.plan_id
            && tab.agent_type.as_deref() == Some(request.provider.as_str())
            && tab.agent_session_id.as_deref() == Some(request.old_thread.as_str())
            && tab.session_binding_nonce.as_deref() == Some(request.old_nonce.as_str())
            && tab.reported_session_binding_nonce.as_deref() == Some(request.old_nonce.as_str())
            && tab.task_binding.as_ref().is_some_and(|binding| {
                binding.plan_id.as_ref() == Some(&request.plan_id)
                    && binding.task_id.as_ref() == Some(&request.task_id)
                    && binding.attempt_id.as_ref() == Some(&request.attempt_id)
            });
        let unchanged = if let Some(completed) = &authority.completed_task {
            detail.task == *completed
                && authority.completed_source.as_ref().is_some_and(|source| source.matches(&tab))
                && tab.task_binding == authority.completed_binding
                && self.plan_store.live_task_attempts_for_tab(tab_id).is_empty()
        } else {
            detail.task.assigned_tab_id.as_deref() == Some(tab_id)
                && detail.task.interruption.owner_epoch == request.owner_epoch
                && detail.task.interruption.owner_tab_id.as_deref() == Some(tab_id)
                && detail.task.interruption.owner_session_id.as_deref()
                    == Some(request.old_thread.as_str())
                && self.plan_store.open_attempt_task_for_effort_for_tab(
                    tab_id, &request.old_thread, &request.old_nonce
                ) == Some((request.plan_id.clone(), request.task_id.clone(), request.attempt_id.clone()))
                && detail.task.execution_attempts.iter().any(|attempt| {
                    attempt.id == request.attempt_id
                        && attempt.provider.as_deref() == Some(request.provider.as_str())
                })
        };
        if !exact || !unchanged {
            return Err("provider_switch_binding_refused: source task binding changed".into());
        }
        Ok(())
    }

    pub(crate) fn authorize_provider_switch_binding(
        &self,
        tab_id: &str,
        source: &str,
        target: &str,
        nonce: &str,
    ) -> Result<Option<RestartBindingAuthority>, String> {
        if let Some(authority) =
            self.completed_restart_binding(
                tab_id, source, Some(target), nonce, RuntimeRestartMode::ProviderSwitch,
            )?
        {
            return Ok(Some(authority));
        }
        if !self.restore_provider_switch_task_binding(tab_id, source, nonce)? {
            return Ok(None);
        }
        let mut authority = self
            .try_authorize_runtime_restart_binding(
                tab_id,
                source,
                nonce,
                RuntimeRestartMode::ProviderSwitch,
            )
            .map_err(|failure| format!("provider_switch_binding_refused: {}", failure.reason))?
            .ok_or("provider_switch_binding_refused: task attempt cannot be carried")?;
        authority.request.target_provider = Some(target.into());
        Ok(Some(authority))
    }

    fn completed_restart_binding(
        &self,
        tab_id: &str,
        source: &str,
        target: Option<&str>,
        nonce: &str,
        mode: RuntimeRestartMode,
    ) -> Result<Option<RestartBindingAuthority>, String> {
        let tab = self.mux.read().get_tab(tab_id).cloned()
            .ok_or("provider_switch_binding_refused: tab not found")?;
        let retained = {
            let mut sessions = COMPLETED_SESSIONS.lock();
            prune_completed_sessions(&mut sessions);
            sessions.get(&key(self, tab_id)).cloned()
        };
        if let Some(retained) = &retained {
            if tab.task_binding.as_ref() != Some(&retained.binding)
                || !retained.current.matches(&tab)
                || !retained.owner.upgrade().is_some_and(|owner| Arc::ptr_eq(&owner, &self.mux))
            {
                return Err("provider_switch_binding_refused: completed current-session proof changed".into());
            }
        }
        let Some(binding) = tab.task_binding.as_ref() else {
            return Ok(None);
        };
        let (Some(plan), Some(task), Some(attempt)) =
            (&binding.plan_id, &binding.task_id, &binding.attempt_id)
        else {
            return Ok(None);
        };
        let Some(detail) = self.plan_store.show_task(task) else {
            if retained.is_some() {
                return Err("provider_switch_binding_refused: completed task history disappeared".into());
            }
            return Ok(None);
        };
        if retained.as_ref().is_some_and(|retained| {
            retained.plan_id != detail.plan_id || retained.task != detail.task
        }) {
            return Err("provider_switch_binding_refused: completed task history changed".into());
        }
        if detail.task.status != crate::plan::TaskStatus::Done {
            return Ok(None);
        }
        if !matches!(mode, RuntimeRestartMode::ProviderSwitch | RuntimeRestartMode::Resume) {
            return Err("provider_switch_binding_refused: completed history only permits provider handoff or same-session resume".into());
        }
        let matches = detail.task.execution_attempts.iter()
            .filter(|entry| entry.id == *attempt)
            .collect::<Vec<_>>();
        let valid = matches.as_slice().first().filter(|_| matches.len() == 1)
            .is_some_and(|old| {
                completed_attempt_matches(&tab, old, source, nonce)
                    && old.interruption.owner_epoch == detail.task.interruption.owner_epoch
            });
        if (retained.is_none() && (!valid
            || detail.task.interruption.owner_session_id != tab.agent_session_id))
            || detail.plan_id != *plan
            || tab.agent_type.as_deref() != Some(source)
            || tab.session_binding_nonce.as_deref() != Some(nonce)
            || detail.task.execution_attempts.iter().filter(|entry| {
                entry.id == *attempt
            }).count() != 1
            || (retained.is_none() && detail.task.execution_attempts.iter().filter(|entry| {
                completed_attempt_matches(&tab, entry, source, nonce)
            }).count() != 1)
            || detail.task.assigned_tab_id.is_some()
            || detail.task.interruption.owner_epoch == 0
            || detail.task.interruption.owner_tab_id.as_deref() != Some(tab_id)
            || detail.task.execution_attempts.iter().any(|entry| entry.closed_at.is_none())
            || !self.plan_store.live_task_attempts_for_tab(tab_id).is_empty()
        {
            return Err("provider_switch_binding_refused: completed task history is not exact".into());
        }
        let current = CurrentSession::capture(&tab)
            .map_err(|reason| format!("provider_switch_binding_refused: {reason}"))?;
        Ok(Some(RestartBindingAuthority {
            owner: Arc::downgrade(&self.mux),
            request: RuntimeRestartBinding {
                plan_id: plan.clone(),
                task_id: task.clone(),
                attempt_id: attempt.clone(),
                executor_tab_id: tab_id.into(),
                provider: source.into(),
                target_provider: target.map(str::to_owned),
                old_thread: tab.agent_session_id.unwrap_or_default(),
                old_nonce: nonce.into(),
                new_thread: String::new(),
                new_nonce: String::new(),
                owner_epoch: detail.task.interruption.owner_epoch,
                mode,
            },
            carried: false,
            completed_task: Some(detail.task),
            completed_source: Some(current),
            completed_binding: Some(binding.clone()),
            completed_target: None,
        }))
    }

    fn restore_provider_switch_task_binding(
        &self,
        tab_id: &str,
        source: &str,
        nonce: &str,
    ) -> Result<bool, String> {
        let tab = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or("provider_switch_binding_refused: tab not found")?;
        let live_attempts = self
            .plan_store
            .live_task_attempts_for_tab(tab_id)
            .into_iter()
            .filter(|candidate| candidate.attempt.retired_at.is_none())
            .collect::<Vec<_>>();
        let task_binding_present = tab.task_binding.as_ref().is_some_and(|binding| {
            binding.plan_id.is_some() || binding.task_id.is_some() || binding.attempt_id.is_some()
        });
        if live_attempts.is_empty() && !task_binding_present {
            return Ok(false);
        }
        if tab.agent_type.as_deref() != Some(source)
            || tab.session_binding_nonce.as_deref() != Some(nonce)
        {
            return Err("provider_switch_binding_refused: source session changed".into());
        }
        let current = tab.agent_session_id.as_deref().and_then(|session_id| {
            self.plan_store
                .open_attempt_task_for_effort_for_tab(tab_id, session_id, nonce)
        });
        let Some((plan_id, task_id, attempt_id)) = current else {
            return Err(
                "provider_switch_binding_refused: current task attempt cannot be carried".into(),
            );
        };
        if tab.agent_session_id.is_none()
            || tab.reported_session_binding_nonce.as_deref() != Some(nonce)
            || live_attempts.len() != 1
            || live_attempts[0].plan_id != plan_id
            || live_attempts[0].task_id != task_id
            || live_attempts[0].attempt.id != attempt_id
        {
            return Err(
                "provider_switch_binding_refused: current task attempt is ambiguous or unverified"
                    .into(),
            );
        }
        let prior = tab.task_binding.clone().unwrap_or_default();
        let detail = self.plan_store.show_task(&task_id)
            .ok_or("provider_switch_binding_refused: task disappeared")?;
        let source_attempt = detail.task.execution_attempts.iter()
            .filter(|entry| entry.id == attempt_id).collect::<Vec<_>>();
        if source_attempt.len() != 1
            || source_attempt[0].provider.as_deref() != Some(source)
            || detail.task.assigned_tab_id.as_deref() != Some(tab_id)
            || detail.task.interruption.owner_epoch == 0
            || detail.task.interruption.owner_tab_id.as_deref() != Some(tab_id)
            || detail.task.interruption.owner_session_id != tab.agent_session_id
            || detail.task.execution_attempts.iter().any(|entry| {
                entry.id != attempt_id && entry.closed_at.is_none() && entry.retired_at.is_none()
            })
        {
            return Err("provider_switch_binding_refused: source task owner changed".into());
        }
        if prior
            .plan_id
            .as_deref()
            .is_some_and(|bound| bound != plan_id)
            || prior
                .task_id
                .as_deref()
                .is_some_and(|bound| bound != task_id)
            || prior
                .attempt_id
                .as_deref()
                .is_some_and(|bound| bound != attempt_id)
        {
            return Err(
                "provider_switch_binding_refused: tab task binding conflicts with its attempt"
                    .into(),
            );
        }
        let bound_at = if prior.bound_at == 0 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis() as u64)
                .unwrap_or_default()
        } else {
            prior.bound_at
        };
        let restored = TaskBinding {
            plan_id: Some(plan_id),
            task_id: Some(task_id),
            attempt_id: Some(attempt_id),
            spawn_handle: prior.spawn_handle,
            bound_at,
        };
        if tab.task_binding.as_ref() != Some(&restored) {
            let mut mux = self.mux.write();
            let current = mux
                .get_tab_mut(tab_id)
                .ok_or("provider_switch_binding_refused: tab disappeared")?;
            if current.agent_type != tab.agent_type
                || current.agent_session_id != tab.agent_session_id
                || current.session_binding_nonce != tab.session_binding_nonce
                || current.reported_session_binding_nonce != tab.reported_session_binding_nonce
                || current.task_binding != tab.task_binding
            {
                return Err("provider_switch_binding_refused: source task binding changed".into());
            }
            current.task_binding = Some(restored);
        }
        Ok(true)
    }

    pub(crate) fn authorize_runtime_restart_binding(
        &self,
        tab_id: &str,
        provider: &str,
        nonce: &str,
        fresh: bool,
    ) -> Result<Option<RestartBindingAuthority>, String> {
        self.authorize_restart_binding_mode(
            tab_id,
            provider,
            nonce,
            if fresh {
                RuntimeRestartMode::AuthorizedFresh
            } else {
                RuntimeRestartMode::Resume
            },
        )
    }

    pub(crate) fn authorize_model_switch_restart_binding(
        &self,
        tab_id: &str,
        provider: &str,
        nonce: &str,
    ) -> Result<Option<RestartBindingAuthority>, String> {
        self.authorize_restart_binding_mode(
            tab_id,
            provider,
            nonce,
            RuntimeRestartMode::ModelSwitch,
        )
    }

    fn authorize_restart_binding_mode(
        &self,
        tab_id: &str,
        provider: &str,
        nonce: &str,
        mode: RuntimeRestartMode,
    ) -> Result<Option<RestartBindingAuthority>, String> {
        if let Some(history) = self.completed_restart_binding(
            tab_id, provider, None, nonce, mode,
        )? {
            return Ok(Some(history));
        }
        match self.try_authorize_runtime_restart_binding(tab_id, provider, nonce, mode) {
            Ok(authority) => Ok(authority),
            Err(reason) => {
                self.warn_runtime_plan_binding(
                    tab_id,
                    crate::service::agent_runtime::plan_warning::PlanBindingWarningStage::RestartAuthorization,
                    &reason,
                );
                Ok(None)
            }
        }
    }

    fn try_authorize_runtime_restart_binding(
        &self,
        tab_id: &str,
        provider: &str,
        nonce: &str,
        mode: RuntimeRestartMode,
    ) -> Result<Option<RestartBindingAuthority>, PlanBindingFailure> {
        let mut tab = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or("Runtime tab disappeared")?;
        if tab.agent_type.as_deref() != Some(provider)
            || tab.session_binding_nonce.as_deref() != Some(nonce)
        {
            return Err("Runtime restart binding changed".into());
        }
        self.restore_restart_attempt_binding(tab_id)?;
        tab = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or("Runtime tab disappeared")?;
        if tab.agent_type.as_deref() != Some(provider)
            || tab.session_binding_nonce.as_deref() != Some(nonce)
        {
            return Err("Runtime restart binding changed".into());
        }
        let Some(binding) = tab.task_binding else {
            return Ok(None);
        };
        if binding.plan_id.is_none() && binding.task_id.is_none() && binding.attempt_id.is_none() {
            return Ok(None);
        }
        let (Some(plan), Some(task), Some(attempt)) =
            (binding.plan_id, binding.task_id, binding.attempt_id)
        else {
            return Err("Runtime restart task binding incomplete".into());
        };
        let thread = tab
            .agent_session_id
            .ok_or("Runtime restart thread absent")?;
        let detail = self
            .plan_store
            .show_task(&task)
            .ok_or_else(|| PlanBindingFailure::bookkeeping("Runtime restart task absent"))?;
        let old = detail
            .task
            .execution_attempts
            .iter()
            .find(|entry| entry.id == attempt && entry.executor_tab_id == tab_id)
            .ok_or("Runtime restart attempt changed")?;
        if detail.plan_id != plan
            || old.retired_at.is_some()
            || (matches!(
                mode,
                RuntimeRestartMode::AuthorizedFresh | RuntimeRestartMode::ProviderSwitch
            ) && old.closed_at.is_some())
            || old.session_id.as_deref() != Some(thread.as_str())
            || old.session_binding_nonce.as_deref() != Some(nonce)
            || old.provider.as_deref() != Some(provider)
        {
            return Err("Runtime restart previous task binding changed".into());
        }
        Ok(Some(RestartBindingAuthority {
            owner: Arc::downgrade(&self.mux),
            request: RuntimeRestartBinding {
                plan_id: plan,
                task_id: task,
                attempt_id: attempt,
                executor_tab_id: tab_id.into(),
                provider: provider.into(),
                target_provider: None,
                old_thread: thread,
                old_nonce: nonce.into(),
                new_thread: String::new(),
                new_nonce: String::new(),
                owner_epoch: detail.task.interruption.owner_epoch,
                mode,
            },
            carried: false,
            completed_task: None,
            completed_source: None,
            completed_binding: None,
            completed_target: None,
        }))
    }

    pub(crate) fn stage_runtime_restart_binding(
        &self,
        tab_id: &str,
        authority: Option<RestartBindingAuthority>,
    ) {
        let mut bindings = BINDINGS.lock();
        bindings.retain(|_, entry| entry.owner.strong_count() > 0);
        if let Some(authority) = authority {
            bindings.insert(key(self, tab_id), authority);
        } else {
            bindings.remove(&key(self, tab_id));
        }
    }

    pub(crate) fn restart_carries_worker_interruption(&self, tab_id: &str) -> bool {
        let authority = BINDINGS.lock().get(&key(self, tab_id)).cloned();
        authority.is_some_and(|authority| {
            authority.completed_task.is_none()
                && self.ext::<super::super::RestartTracker>()
                .lock()
                .in_progress
                .contains(tab_id)
                && authority
                    .owner
                    .upgrade()
                    .is_some_and(|owner| Arc::ptr_eq(&owner, &self.mux))
        })
    }

    pub(crate) fn clear_runtime_restart_binding(&self, tab_id: &str) {
        BINDINGS.lock().remove(&key(self, tab_id));
    }

    pub(crate) fn carry_authorized_runtime_restart_binding(
        &self,
        tab_id: &str,
        thread: &str,
        nonce: &str,
    ) -> Result<bool, PlanBindingFailure> {
        self.try_carry_authorized_runtime_restart_binding(tab_id, thread, nonce)
            .inspect_err(|_| self.clear_runtime_restart_binding(tab_id))
    }

    fn try_carry_authorized_runtime_restart_binding(
        &self,
        tab_id: &str,
        thread: &str,
        nonce: &str,
    ) -> Result<bool, PlanBindingFailure> {
        let mut bindings = BINDINGS.lock();
        let Some(authority) = bindings.get(&key(self, tab_id)).cloned() else {
            drop(bindings);
            return self.retained_completed_session_matches(tab_id, thread, nonce);
        };
        if !authority
            .owner
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &self.mux))
        {
            return Err("Runtime restart authorization changed".into());
        }
        let request = &authority.request;
        let exact = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.agent_type.as_deref()
                == Some(
                    request
                        .target_provider
                        .as_deref()
                        .unwrap_or(&request.provider),
                )
                && tab.agent_session_id.as_deref() == Some(thread)
                && tab.session_binding_nonce.as_deref() == Some(nonce)
                && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
                && tab.task_binding.as_ref().is_some_and(|binding| {
                    binding.plan_id.as_deref() == Some(request.plan_id.as_str())
                        && binding.task_id.as_deref() == Some(request.task_id.as_str())
                        && binding.attempt_id.as_deref() == Some(request.attempt_id.as_str())
                })
        });
        if !exact {
            return Err("Runtime restart loaded task binding changed".into());
        }
        if let Some(completed) = &authority.completed_task {
            if thread.is_empty() || nonce.is_empty() || nonce == request.old_nonce {
                return Err("Completed provider switch target is unverified".into());
            }
            if !self.plan_store.show_task(&request.task_id).is_some_and(|detail| {
                detail.plan_id == request.plan_id && detail.task == *completed
            }) || !self.plan_store.live_task_attempts_for_tab(tab_id).is_empty() {
                return Err("Completed provider switch history changed".into());
            }
            if authority.carried
                && (request.new_thread != thread || request.new_nonce != nonce)
            {
                return Err("Completed provider switch target changed".into());
            }
            if request.mode == RuntimeRestartMode::Resume && thread != request.old_thread {
                return Err("Completed resume changed the current session".into());
            }
            let tab = self.mux.read().get_tab(tab_id).cloned()
                .ok_or("Completed continuity tab disappeared")?;
            if tab.task_binding != authority.completed_binding
                || (authority.carried && !authority.completed_target.as_ref()
                    .is_some_and(|target| target.matches(&tab)))
            {
                return Err("Completed continuity generation or binding changed".into());
            }
            let current = CurrentSession::capture(&tab).map_err(PlanBindingFailure::from)?;
            let binding = authority.completed_binding.clone()
                .ok_or("Completed continuity historical binding absent")?;
            let mut sessions = COMPLETED_SESSIONS.lock();
            prune_completed_sessions(&mut sessions);
            sessions.insert(key(self, tab_id), CompletedSession {
                owner: authority.owner.clone(),
                plan_id: request.plan_id.clone(),
                task: completed.clone(),
                binding,
                current: current.clone(),
            });
            drop(sessions);
            let stored = bindings.get_mut(&key(self, tab_id))
                .ok_or("Completed provider switch continuity disappeared")?;
            stored.request.new_thread = thread.into();
            stored.request.new_nonce = nonce.into();
            stored.carried = true;
            stored.completed_target = Some(current);
            return Ok(true);
        }
        if authority.carried {
            if request.new_thread == thread
                && request.new_nonce == nonce
                && self.runtime_restart_attempt_matches(request)
            {
                return Ok(true);
            }
            return Err("Runtime restart authorization changed".into());
        }
        let mut request = request.clone();
        request.new_thread = thread.into();
        request.new_nonce = nonce.into();
        self.plan_store
            .rebind_runtime_restart(
                request.clone(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis() as u64)
                    .unwrap_or_default(),
            )
            .map_err(PlanBindingFailure::from_plan_error)?;
        let Some(stored) = bindings.get_mut(&key(self, tab_id)) else {
            return Err("Runtime restart authorization changed".into());
        };
        stored.request = request.clone();
        stored.carried = true;
        drop(bindings);
        self.persist_current_attempt_session_model(tab_id);
        let tab = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or("Restart tab disappeared while recording its effort")?;
        self.plan_store
            .update_dispatched_attempt_reasoning_effort(
                &request.plan_id,
                &request.task_id,
                tab_id,
                &request.attempt_id,
                tab.reasoning_effort_identity.resolved.clone(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis() as u64)
                    .unwrap_or_default(),
            )
            .map_err(PlanBindingFailure::from_plan_error)?;
        self.record_session_reasoning_effort(
            tab_id,
            tab.agent_type.as_deref().unwrap_or_default(),
            thread,
            Some(nonce),
            tab.reasoning_effort_identity.actual,
        );
        Ok(true)
    }

    pub(crate) fn finalize_authorized_runtime_restart_binding(
        &self,
        tab_id: &str,
        thread: &str,
        nonce: &str,
    ) -> Result<bool, PlanBindingFailure> {
        if !self.carry_authorized_runtime_restart_binding(tab_id, thread, nonce)? {
            return Ok(false);
        }
        let binding_key = key(self, tab_id);
        let mut bindings = BINDINGS.lock();
        let Some(authority) = bindings.get(&binding_key) else {
            return Ok(false);
        };
        if !authority.carried
            || authority.request.new_thread != thread
            || authority.request.new_nonce != nonce
        {
            return Err("Runtime restart authorization changed".into());
        }
        bindings.remove(&binding_key);
        Ok(true)
    }

    fn runtime_restart_attempt_matches(&self, request: &RuntimeRestartBinding) -> bool {
        self.plan_store
            .show_task(&request.task_id)
            .is_some_and(|detail| {
                detail.plan_id == request.plan_id
                    && detail.task.assigned_tab_id.as_deref()
                        == Some(request.executor_tab_id.as_str())
                    && detail.task.interruption.owner_epoch == request.owner_epoch
                    && detail.task.interruption.owner_tab_id.as_deref()
                        == Some(request.executor_tab_id.as_str())
                    && detail.task.interruption.owner_session_id.as_deref()
                        == Some(request.new_thread.as_str())
                    && detail.task.execution_attempts.iter().any(|attempt| {
                        attempt.id == request.attempt_id
                            && attempt.executor_tab_id == request.executor_tab_id
                            && attempt.provider.as_deref()
                                == Some(
                                    request
                                        .target_provider
                                        .as_deref()
                                        .unwrap_or(&request.provider),
                                )
                            && attempt.session_id.as_deref() == Some(request.new_thread.as_str())
                            && attempt.session_binding_nonce.as_deref()
                                == Some(request.new_nonce.as_str())
                            && attempt.retired_at.is_none()
                    })
            })
    }
}

#[cfg(test)]
mod tests;
