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

    pub fn from_message(message: &str) -> Option<Self> {
        let (code, body) = message.split_once(": ")?;
        if !matches!(
            code,
            "unknown_launch_profile" | "launch_profile_required" | "launch_profile_not_installed"
        ) {
            return None;
        }
        let refusal: Self = serde_json::from_str(body).ok()?;
        (refusal.code == code).then_some(refusal)
    }
}

impl std::fmt::Display for LaunchProfileRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.data())
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
