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
    pub runtime_epoch: Option<u64>,
    pub lifecycle_epoch: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct AccountRestartTarget {
    pub expected: AccountRestartBinding,
    pub account_id: Option<String>,
    pub account_namespace: Option<String>,
    pub invocation: Option<Arc<InvocationContext>>,
}

impl AccountRestartBinding {
    pub fn capture(tab: &Tab, runtime_epoch: Option<u64>) -> Result<Self, String> {
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
            session_id: tab
                .agent_session_id
                .clone()
                .filter(|id| !id.is_empty())
                .ok_or("Account restart requires an exact session")?,
            runtime_epoch,
            lifecycle_epoch: tab.lifecycle_epoch,
        };
        binding.validate(tab, runtime_epoch)?;
        Ok(binding)
    }

    pub fn validate(&self, tab: &Tab, runtime_epoch: Option<u64>) -> Result<(), String> {
        self.validate_selection(tab)?;
        if tab.agent_type.as_deref() != Some(&self.provider)
            || tab.agent_session_id.as_deref() != Some(&self.session_id)
            || tab.session_binding_nonce.as_deref() != Some(&self.binding_nonce)
            || tab.reported_session_binding_nonce.as_deref() != Some(&self.binding_nonce)
            || self.runtime_epoch != runtime_epoch
            || self.session_id.is_empty()
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
        if tab.restorable_launch.account_id != self.account_id
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
