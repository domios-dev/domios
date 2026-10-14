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
}
static BINDINGS: LazyLock<Mutex<HashMap<(usize, String), RestartBindingAuthority>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn key(svc: &ServiceContext, tab: &str) -> (usize, String) {
    (Arc::as_ptr(&svc.mux) as usize, tab.into())
}

impl ServiceContext {
    pub(crate) fn authorize_runtime_restart_binding(
        &self,
        tab_id: &str,
        provider: &str,
        nonce: &str,
        fresh: bool,
    ) -> Result<Option<RestartBindingAuthority>, String> {
        let tab = self
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
            .ok_or("Runtime restart task absent")?;
        let old = detail
            .task
            .execution_attempts
            .iter()
            .find(|entry| entry.id == attempt && entry.executor_tab_id == tab_id)
            .ok_or("Runtime restart attempt changed")?;
        if detail.plan_id != plan
            || old.retired_at.is_some()
            || (fresh && old.closed_at.is_some())
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
                old_thread: thread,
                old_nonce: nonce.into(),
                new_thread: String::new(),
                new_nonce: String::new(),
                owner_epoch: detail.task.interruption.owner_epoch,
                mode: if fresh {
                    RuntimeRestartMode::AuthorizedFresh
                } else {
                    RuntimeRestartMode::Resume
                },
            },
        }))
    }

    pub(crate) fn retain_runtime_restart_binding(
        &self,
        tab_id: &str,
        authority: Option<RestartBindingAuthority>,
        nonce: &str,
    ) {
        let mut bindings = BINDINGS.lock();
        bindings.retain(|_, entry| entry.owner.strong_count() > 0);
        if let Some(mut authority) = authority {
            authority.request.new_nonce = nonce.into();
            bindings.insert(key(self, tab_id), authority);
        } else {
            bindings.remove(&key(self, tab_id));
        }
    }

    pub(crate) fn finalize_authorized_runtime_restart_binding(
        &self,
        tab_id: &str,
        thread: &str,
        nonce: &str,
    ) -> Result<bool, String> {
        let Some(authority) = BINDINGS.lock().get(&key(self, tab_id)).cloned() else {
            return Ok(false);
        };
        if !authority
            .owner
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &self.mux))
            || authority.request.new_nonce != nonce
        {
            return Err("Runtime restart authorization changed".into());
        }
        let request = authority.request;
        let exact = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.agent_type.as_deref() == Some(request.provider.as_str())
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
        self.plan_store
            .rebind_runtime_restart(
                RuntimeRestartBinding {
                    new_thread: thread.into(),
                    ..request
                },
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis() as u64)
                    .unwrap_or_default(),
            )
            .map_err(|reason| reason.to_string())?;
        BINDINGS.lock().remove(&key(self, tab_id));
        Ok(true)
    }
}
