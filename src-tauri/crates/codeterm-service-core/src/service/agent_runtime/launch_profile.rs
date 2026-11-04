use super::model::RuntimeFailure;
use codeterm_plugin::{
    agent_runtime::{AgentRuntimeCapability, LaunchAuthority},
    provider::AgentProvider,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

pub(super) struct ResolvedLaunch {
    pub binary: PathBuf,
    pub capability: AgentRuntimeCapability,
    pub env: BTreeMap<String, String>,
    profile: Option<String>,
    nonce: String,
}

impl ResolvedLaunch {
    pub(super) fn apply_environment(
        &self,
        env: &mut HashMap<String, String>,
        sandbox: BTreeMap<String, String>,
    ) {
        env.extend(self.env.clone());
        env.extend(sandbox);
    }

    pub(super) fn verifies(&self, svc: &crate::service::ServiceContext, tab: &str) -> bool {
        let mux = svc.mux.read();
        mux.get_tab(tab).is_some_and(|target| {
            target.session_binding_nonce.as_deref() == Some(self.nonce.as_str())
                && target
                    .agent_runtime_config
                    .as_ref()
                    .and_then(|config| config.get("launchProfile"))
                    == self
                        .profile
                        .as_ref()
                        .map(|profile| Value::String(profile.clone()))
                        .as_ref()
        })
    }
}

#[derive(Debug)]
struct SelectedLaunch {
    capability: AgentRuntimeCapability,
    env: BTreeMap<String, String>,
    profile: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchProfileError {
    BindingMismatch,
    InvalidReference,
    Declaration(String),
}
impl std::fmt::Display for LaunchProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Runtime launch profile: {self:?}")
    }
}

fn select(
    capability: &AgentRuntimeCapability,
    config: &Value,
    current_nonce: Option<&str>,
    expected_nonce: &str,
) -> Result<SelectedLaunch, LaunchProfileError> {
    if expected_nonce.is_empty() || current_nonce != Some(expected_nonce) {
        return Err(LaunchProfileError::BindingMismatch);
    }
    let profile = match config.get("launchProfile") {
        None => None,
        Some(Value::String(name)) if !name.is_empty() => Some(name.clone()),
        Some(_) => return Err(LaunchProfileError::InvalidReference),
    };
    let command = capability
        .profile_command(profile.as_deref())
        .map_err(|error| LaunchProfileError::Declaration(error.to_string()))?;
    let env = profile
        .as_ref()
        .and_then(|name| capability.launch_profiles.as_ref()?.get(name))
        .map(|profile| profile.env.clone())
        .unwrap_or_default();
    let mut resolved = capability.clone();
    resolved.command = command;
    Ok(SelectedLaunch {
        capability: resolved,
        env,
        profile,
    })
}

pub(super) fn resolve(
    svc: &crate::service::ServiceContext,
    tab: &str,
    authority: &LaunchAuthority,
    adapter: &dyn AgentProvider,
    capability: &AgentRuntimeCapability,
) -> Result<ResolvedLaunch, RuntimeFailure> {
    let selected = {
        let mux = svc.mux.read();
        let target = mux.get_tab(tab).ok_or(RuntimeFailure::Superseded)?;
        if authority.binding().tab_id != tab
            || target.agent_type.as_deref() != Some(authority.binding().provider_id.as_str())
        {
            return Err(RuntimeFailure::Superseded);
        }
        select(
            capability,
            target.agent_runtime_config.as_ref().unwrap_or(&Value::Null),
            target.session_binding_nonce.as_deref(),
            &authority.binding().binding_nonce,
        )
        .map_err(|error| RuntimeFailure::Refused(error.to_string()))?
    };
    let binary = adapter
        .authorize_agent_runtime(authority, selected.profile.as_deref())
        .map_err(|error| RuntimeFailure::Refused(error.to_string()))?;
    Ok(ResolvedLaunch {
        binary,
        capability: selected.capability,
        env: selected.env,
        profile: selected.profile,
        nonce: authority.binding().binding_nonce.clone(),
    })
}

#[cfg(test)]
mod tests;
