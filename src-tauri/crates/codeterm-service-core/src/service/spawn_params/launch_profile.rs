use super::ServiceContext;
use crate::service::agent_runtime::launch_profile_install::{self, InstallState};
use codeterm_plugin::agent_runtime::AgentRuntimeCapability;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LaunchProfileRefusal {
    pub code: String,
    pub provider: String,
    pub requested_profile: Option<String>,
    pub declared_profile_ids: Vec<String>,
    pub message: String,
}

impl LaunchProfileRefusal {
    fn new(code: &str, provider: &str, requested: Option<&str>, declared: Vec<String>) -> Self {
        let message = format!(
            "{code}: provider '{provider}', launch profile {}; declared profile ids: [{}]",
            requested.unwrap_or("<missing>"),
            declared.join(", ")
        );
        Self {
            code: code.into(),
            provider: provider.into(),
            requested_profile: requested.map(str::to_owned),
            declared_profile_ids: declared,
            message,
        }
    }

    pub fn data(&self) -> serde_json::Value {
        serde_json::json!({
            "type": self.code,
            "code": self.code,
            "provider": self.provider,
            "requested_profile": self.requested_profile,
            "declared_profile_ids": self.declared_profile_ids,
            "message": self.message,
        })
    }
}

impl std::fmt::Display for LaunchProfileRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpawnTargetRefusal {
    LaunchProfile(LaunchProfileRefusal),
    ExecutionSandbox { provider: String, message: String },
    Other(String),
}

impl SpawnTargetRefusal {
    pub fn data(&self) -> Option<serde_json::Value> {
        match self {
            Self::LaunchProfile(error) => Some(error.data()),
            Self::ExecutionSandbox { provider, message } => Some(serde_json::json!({
                "type":"execution_sandbox_unsupported", "code":"execution_sandbox_unsupported",
                "provider":provider, "message":message,
            })),
            Self::Other(_) => None,
        }
    }
}

impl std::fmt::Display for SpawnTargetRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LaunchProfile(error) => std::fmt::Display::fmt(error, f),
            Self::ExecutionSandbox { message, .. } | Self::Other(message) => f.write_str(message),
        }
    }
}

impl From<LaunchProfileRefusal> for SpawnTargetRefusal {
    fn from(error: LaunchProfileRefusal) -> Self {
        Self::LaunchProfile(error)
    }
}

impl From<String> for SpawnTargetRefusal {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<crate::service::spawn_capability::SpawnRejection> for SpawnTargetRefusal {
    fn from(error: crate::service::spawn_capability::SpawnRejection) -> Self {
        Self::Other(error.into())
    }
}

impl From<&str> for SpawnTargetRefusal {
    fn from(message: &str) -> Self {
        Self::Other(message.into())
    }
}

impl From<crate::service::CwdRefusal> for SpawnTargetRefusal {
    fn from(error: crate::service::CwdRefusal) -> Self {
        Self::Other(error.to_string())
    }
}

impl From<SpawnTargetRefusal> for String {
    fn from(error: SpawnTargetRefusal) -> Self {
        error.to_string()
    }
}

fn validate(
    provider: &str,
    capability: Option<&AgentRuntimeCapability>,
    requested: Option<&str>,
    installed: impl FnOnce(&str, &codeterm_plugin::agent_runtime::RuntimeLaunchProfile) -> bool,
) -> Result<(), LaunchProfileRefusal> {
    let profiles = capability.and_then(|capability| capability.launch_profiles.as_ref());
    let mut declared = profiles
        .map(|profiles| profiles.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    declared.sort();
    let refusal = |code| LaunchProfileRefusal::new(code, provider, requested, declared.clone());
    let Some(id) = requested else {
        return if profiles.is_some() {
            Err(refusal("launch_profile_required"))
        } else {
            Ok(())
        };
    };
    let profile = profiles
        .and_then(|profiles| profiles.get(id))
        .ok_or_else(|| refusal("unknown_launch_profile"))?;
    if !installed(id, profile) {
        return Err(refusal("launch_profile_not_installed"));
    }
    Ok(())
}

impl ServiceContext {
    pub fn validate_spawn_options(
        &self,
        provider: &str,
        params: &crate::service::SpawnAgentParams,
    ) -> Result<(), SpawnTargetRefusal> {
        self.validate_launch_profile(provider, params.launch_profile.as_deref())?;
        self.resolve_spawn_profile_options(provider, &mut params.clone())?;
        crate::service::execution_sandbox::preflight(
            &self.agent_registry,
            provider,
            params,
            crate::service::agent_runtime::engine::requested(self, provider),
        )
    }

    pub fn validate_launch_profile(
        &self,
        provider: &str,
        requested: Option<&str>,
    ) -> Result<(), LaunchProfileRefusal> {
        let capability = self
            .agent_registry
            .with_provider(provider, |provider| provider.agent_runtime())
            .flatten();
        validate(provider, capability.as_ref(), requested, |id, profile| {
            launch_profile_install::profile_root(provider).is_ok_and(|root| {
                matches!(
                    launch_profile_install::detect(&root, id, profile),
                    InstallState::Installed | InstallState::Detected
                )
            })
        })
    }
}

#[cfg(test)]
mod tests;
