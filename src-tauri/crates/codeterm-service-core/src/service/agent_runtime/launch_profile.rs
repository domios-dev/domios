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

/// The launch profile this tab's runtime was started on; a launch derived from the tab needs it too.
pub fn of_tab(tab: &crate::multiplexer::types::Tab) -> Option<String> {
    tab.agent_runtime_config
        .as_ref()?
        .get("launchProfile")?
        .as_str()
        .filter(|profile| !profile.is_empty())
        .map(str::to_owned)
}

pub(super) struct ResolvedLaunch {
    pub binary: PathBuf,
    pub capability: AgentRuntimeCapability,
    pub env: BTreeMap<String, String>,
    working_directory: Option<String>,
    profile: Option<String>,
    nonce: String,
}

impl ResolvedLaunch {
    /// A profile-less launch of `capability`'s own command from `binary`.
    #[cfg(test)]
    pub(super) fn unprofiled(binary: &str, capability: AgentRuntimeCapability) -> Self {
        Self {
            binary: PathBuf::from(binary),
            capability,
            env: BTreeMap::new(),
            working_directory: None,
            profile: None,
            nonce: String::new(),
        }
    }

    /// The agent process's directory: the profile's host-owned one when declared, else the tab's.
    pub(super) fn process_cwd<'a>(&'a self, tab_cwd: &'a str) -> &'a str {
        self.working_directory.as_deref().unwrap_or(tab_cwd)
    }

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
    if let (Some(bridges), Some(client)) = (
        resolved.client_bridges.as_mut(),
        profile
            .as_ref()
            .and_then(|name| capability.launch_profiles.as_ref()?.get(name))
            .and_then(|profile| profile.client_capabilities.as_ref()),
    ) {
        client.restrict(bridges);
    }
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
    let mut selected = {
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
    let profile = selected
        .profile
        .as_deref()
        .and_then(|name| capability.launch_profiles.as_ref()?.get(name));
    let binary = if let (Some(name), Some(profile)) = (selected.profile.as_deref(), profile) {
        if profile.install.is_some() || profile.owner_configured {
            let root =
                super::launch_profile_install::profile_root(&authority.binding().provider_id)
                    .map_err(RuntimeFailure::Refused)?;
            let receipt = super::launch_profile_install::read_receipt(&root, name, profile)
                .map_err(RuntimeFailure::Refused)?
                .ok_or_else(|| {
                    RuntimeFailure::Refused(
                        "Profile requires an owner-confirmed install or local command receipt"
                            .into(),
                    )
                })?;
            selected.capability.command = receipt.command.clone();
            PathBuf::from(receipt.command.binary)
        } else {
            adapter
                .authorize_agent_runtime(authority, selected.profile.as_deref())
                .map_err(|error| RuntimeFailure::Refused(error.to_string()))?
        }
    } else {
        adapter
            .authorize_agent_runtime(authority, selected.profile.as_deref())
            .map_err(|error| RuntimeFailure::Refused(error.to_string()))?
    };
    let working_directory = match (selected.profile.as_deref(), profile) {
        (Some(name), Some(profile)) => {
            super::working_directory::resolve(&authority.binding().provider_id, name, profile)?
        }
        _ => None,
    };
    if let Some(profile) = profile {
        selected
            .env
            .extend(super::launch_profile_install::secret_environment(
                svc,
                &authority.binding().provider_id,
                profile,
            )?);
        selected.env.extend(super::auth_env::auth_environment(
            svc,
            &authority.binding().provider_id,
            profile,
        )?);
    }
    Ok(ResolvedLaunch {
        binary,
        capability: selected.capability,
        env: selected.env,
        working_directory,
        profile: selected.profile,
        nonce: authority.binding().binding_nonce.clone(),
    })
}

#[cfg(test)]
mod tests;
