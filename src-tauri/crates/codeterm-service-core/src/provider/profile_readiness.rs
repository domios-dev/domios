use crate::service::agent_runtime::launch_profile_install::{self, InstallState};
use codeterm_plugin::{
    agent_runtime::AgentRuntimeCapability,
    provider_cli::{status_from_profiles, ProviderCliStatus, ProviderLaunchProfileStatus},
};

pub(super) fn observe(id: &str, capability: &AgentRuntimeCapability) -> Option<ProviderCliStatus> {
    observe_from_root(capability, launch_profile_install::profile_root(id))
}

fn observe_from_root(
    capability: &AgentRuntimeCapability,
    root: Result<std::path::PathBuf, String>,
) -> Option<ProviderCliStatus> {
    let profiles = capability.launch_profiles.as_ref()?;
    Some(status_from_profiles(
        profiles
            .iter()
            .map(|(name, profile)| {
                let installed = match root
                    .as_ref()
                    .map(|root| launch_profile_install::detect(root, name, profile))
                {
                    Ok(InstallState::Installed | InstallState::Detected) => Some(true),
                    Ok(InstallState::NotInstalled) => Some(false),
                    Ok(InstallState::Unknown) | Err(_) => None,
                };
                ProviderLaunchProfileStatus {
                    id: name.clone(),
                    installed,
                }
            })
            .collect(),
    ))
}

pub(super) fn refresh_values(registry: &super::ProviderRegistry, values: &mut [serde_json::Value]) {
    for value in values {
        let Some(id) = value["id"].as_str() else {
            continue;
        };
        let Some(status) = registry.profile_status(id) else {
            continue;
        };
        value["availability"] = serde_json::to_value(status.availability).unwrap_or_default();
        value["cli_status"] = serde_json::to_value(status).unwrap_or_default();
    }
}

#[cfg(test)]
mod tests;
