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
}
static BINDINGS: LazyLock<Mutex<HashMap<(usize, String), RestartBindingAuthority>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn key(svc: &ServiceContext, tab: &str) -> (usize, String) {
    (Arc::as_ptr(&svc.mux) as usize, tab.into())
}

impl ServiceContext {
    pub(crate) fn authorize_provider_switch_binding(
        &self,
        tab_id: &str,
        source: &str,
        target: &str,
        nonce: &str,
    ) -> Result<Option<RestartBindingAuthority>, String> {
        if !self.restore_provider_switch_task_binding(tab_id, source, nonce)? {
            return Ok(None);
        }
        let mut authority = self
            .authorize_restart_binding_mode(
                tab_id,
                source,
                nonce,
                RuntimeRestartMode::ProviderSwitch,
            )?
            .ok_or("provider_switch_binding_refused: task attempt cannot be carried")?;
        authority.request.target_provider = Some(target.into());
        Ok(Some(authority))
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
            self.ext::<super::super::RestartTracker>()
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
            return Ok(false);
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
