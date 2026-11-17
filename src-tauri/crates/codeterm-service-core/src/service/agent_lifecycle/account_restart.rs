use crate::multiplexer::types::Tab;
use codeterm_plugin::provider_account::InvocationContext;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountRestartBinding {
    pub provider: String,
    pub account_id: Option<String>,
    pub account_namespace: Option<String>,
    pub binding_nonce: String,
    pub session_id: String,
    pub unbound_login: bool,
    pub refresh_login: bool,
    pub runtime_epoch: Option<u64>,
    pub lifecycle_epoch: Option<u64>,
}

#[derive(Clone)]
pub struct AccountRestartTarget {
    pub expected: AccountRestartBinding,
    pub account_id: Option<String>,
    pub account_namespace: Option<String>,
    pub invocation: Option<Arc<InvocationContext>>,
    pub terminal_turn: Option<String>,
}

impl AccountRestartBinding {
    pub fn capture(tab: &Tab, runtime_epoch: Option<u64>) -> Result<Self, String> {
        Self::capture_inner(tab, runtime_epoch, false, false)
    }

    pub fn capture_login(tab: &Tab, runtime_epoch: u64) -> Result<Self, String> {
        Self::capture_inner(tab, Some(runtime_epoch), true, false)
    }

    pub fn capture_refresh(
        svc: &crate::service::ServiceContext,
        tab_id: &str,
    ) -> Result<Self, String> {
        let session = crate::service::agent_runtime::engine::for_tab(tab_id)
            .ok_or("Credential refresh runtime absent")?;
        let epoch = session.state.lock().epoch;
        let tab = svc
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or("Credential refresh tab absent")?;
        let binding = Self::capture_inner(&tab, Some(epoch), false, true)?;
        binding.validate_refresh_authority(svc, tab_id, &session)?;
        Ok(binding)
    }

    pub(crate) fn validate_refresh_authority(
        &self,
        svc: &crate::service::ServiceContext,
        tab_id: &str,
        session: &Arc<crate::service::agent_runtime::engine::Session>,
    ) -> Result<(), String> {
        use crate::service::agent_runtime::{authority, engine, model::RuntimePhase};
        if !self.refresh_login
            || self.unbound_login
            || !engine::for_tab(tab_id).is_some_and(|current| Arc::ptr_eq(&current, session))
        {
            return Err("Credential refresh runtime replaced".into());
        }
        engine::validate_binding(svc, session).map_err(|error| error.to_string())?;
        if !authority::verifies(svc, tab_id, &self.provider, &self.binding_nonce) {
            return Err("Credential refresh retained restart authority absent".into());
        }
        let state = session.state.lock();
        let login_pending = matches!(state.phase, RuntimePhase::LoginPending)
            && self.runtime_epoch == Some(state.epoch)
            && state.thread_id.as_deref() == Some(self.session_id.as_str());
        drop(state);
        let context = session.context.lock();
        let exact_context = context.tab_id == tab_id
            && context.epoch == self.runtime_epoch.unwrap_or_default()
            && context.binding_nonce == self.binding_nonce
            && context.thread_id.as_deref() == Some(self.session_id.as_str());
        drop(context);
        if !login_pending
            || !exact_context
            || session
                .thread_loaded
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(
                "Credential refresh requires the exact retained LoginPending thread".into(),
            );
        }
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id).ok_or("Credential refresh tab absent")?;
        self.validate(tab, self.runtime_epoch)
    }

    fn capture_inner(
        tab: &Tab,
        runtime_epoch: Option<u64>,
        unbound_login: bool,
        refresh_login: bool,
    ) -> Result<Self, String> {
        let binding = Self {
            provider: tab
                .agent_type
                .clone()
                .ok_or("Account restart provider absent")?,
            account_id: tab.restorable_launch.account_id.clone(),
            account_namespace: tab.restorable_launch.account_namespace.clone(),
            binding_nonce: tab
                .session_binding_nonce
                .clone()
                .ok_or("Account restart nonce absent")?,
            session_id: if unbound_login {
                String::new()
            } else {
                tab.agent_session_id
                    .clone()
                    .filter(|id| !id.is_empty())
                    .ok_or("Account restart requires an exact session")?
            },
            unbound_login,
            refresh_login,
            runtime_epoch,
            lifecycle_epoch: tab.lifecycle_epoch,
        };
        binding.validate(tab, runtime_epoch)?;
        Ok(binding)
    }

    pub fn validate(&self, tab: &Tab, runtime_epoch: Option<u64>) -> Result<(), String> {
        self.validate_selection(tab)?;
        if self.unbound_login && self.refresh_login {
            return Err("Conflicting account restart capture modes".into());
        }
        let session_matches = if self.unbound_login {
            self.session_id.is_empty()
                && self.runtime_epoch.is_some()
                && self.account_id.as_deref().is_some_and(|id| id != "default")
                && tab.agent_session_id.is_none()
                && tab.reported_session_binding_nonce.is_none()
                && !tab.restored_from_snapshot
                && tab.prior_session_binding_nonces.iter().all(|prior| {
                    prior.session_id.is_none() && prior.resume_target_session_id.is_none()
                })
                && matches!(&tab.backing, crate::multiplexer::types::TabBacking::ChatBackend { plugin_id, session_id }
                    if plugin_id == &self.provider && session_id.is_empty())
        } else if self.refresh_login {
            !self.session_id.is_empty()
                && self.runtime_epoch.is_some()
                && self.account_id.as_deref().is_some_and(|id| id != "default")
                && tab.agent_session_id.as_deref() == Some(self.session_id.as_str())
                && tab.reported_session_binding_nonce.is_none()
                && matches!(&tab.backing, crate::multiplexer::types::TabBacking::ChatBackend { plugin_id, session_id }
                    if plugin_id == &self.provider && session_id == &self.session_id)
        } else {
            !self.session_id.is_empty()
                && tab.agent_session_id.as_deref() == Some(self.session_id.as_str())
                && tab.reported_session_binding_nonce.as_deref()
                    == Some(self.binding_nonce.as_str())
        };
        if !session_matches
            || tab.agent_type.as_deref() != Some(self.provider.as_str())
            || tab.session_binding_nonce.as_deref() != Some(self.binding_nonce.as_str())
            || self.runtime_epoch != runtime_epoch
        {
            return Err("Account restart binding changed".into());
        }
        Ok(())
    }

    pub fn validate_selection(&self, tab: &Tab) -> Result<(), String> {
        if tab.restorable_launch.account_id != self.account_id
            || tab.restorable_launch.account_namespace != self.account_namespace
            || tab.lifecycle_epoch != self.lifecycle_epoch
        {
            return Err("Account restart selection changed".into());
        }
        Ok(())
    }

    pub fn validate_retired(&self, tab: &Tab) -> Result<(), String> {
        if self.unbound_login
            || self.refresh_login
            || tab.restorable_launch.account_id != self.account_id
            || tab.restorable_launch.account_namespace != self.account_namespace
            || tab.lifecycle_epoch.is_some()
            || tab.agent_type.is_some()
            || tab.agent_session_id.is_some()
            || tab.session_binding_nonce.is_some()
            || !tab.prior_session_binding_nonces.iter().any(|prior| {
                prior.nonce == self.binding_nonce
                    && prior.session_id.as_deref() == Some(&self.session_id)
            })
        {
            return Err("Account restart retired binding changed".into());
        }
        Ok(())
    }
}

impl AccountRestartTarget {
    pub fn validate_target(&self) -> Result<(), String> {
        let account = self.account_id.as_deref().unwrap_or("default");
        if (self.expected.unbound_login || self.expected.refresh_login)
            && (account == "default" || self.expected.account_id.as_deref() != Some(account))
        {
            return Err(
                "Unbound sign-in can only replace credentials for the same named account".into(),
            );
        }
        if self.expected.refresh_login && self.account_namespace == self.expected.account_namespace
        {
            return Err("Credential refresh requires a new credential namespace".into());
        }
        if !crate::service::provider_accounts::invocation_matches(
            self.invocation.as_deref(),
            &self.expected.provider,
            account,
        ) || (self.invocation.is_some() != self.account_namespace.is_some())
        {
            return Err("Account restart target context mismatch".into());
        }
        Ok(())
    }

    pub fn commit_selection(&self, tab: &mut Tab) -> Result<(), String> {
        self.validate_target()?;
        self.expected.validate_selection(tab)?;
        tab.restorable_launch.account_id = self.account_id.clone();
        tab.restorable_launch.account_namespace = self.account_namespace.clone();
        Ok(())
    }

    pub fn commit_retired_selection(&self, tab: &mut Tab) -> Result<(), String> {
        self.validate_target()?;
        self.expected.validate_retired(tab)?;
        tab.restorable_launch.account_id = self.account_id.clone();
        tab.restorable_launch.account_namespace = self.account_namespace.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
