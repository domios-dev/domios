use crate::service::agent_runtime::launch_profile_install::{self, InstallState};
use codeterm_plugin::{
    agent_runtime::AgentRuntimeCapability,
    provider_cli::{status_from_profiles, ProviderCliStatus, ProviderLaunchProfileStatus},
};

use parking_lot::Mutex;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::LazyLock,
};

static REVISIONS: LazyLock<Mutex<BTreeMap<PathBuf, u64>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
pub(super) type ProfileCache = super::catalogue_cache::RevisionCache<
    (codeterm_plugin::agent_runtime::RuntimeLaunchProfile, u64),
    Option<bool>,
>;

pub(crate) fn revision(root: &Path) -> u64 {
    REVISIONS.lock().get(root).copied().unwrap_or_default()
}

pub(super) fn provider_revisions(mut ids: Vec<String>) -> Vec<(String, u64)> {
    ids.sort();
    ids.into_iter()
        .map(|id| {
            let generation = launch_profile_install::profile_root(&id)
                .map(|root| revision(&root))
                .unwrap_or_default();
            (id, generation)
        })
        .collect()
}

pub(crate) fn invalidate(root: &Path) {
    let mut revisions = REVISIONS.lock();
    let generation = revisions.entry(root.to_path_buf()).or_default();
    *generation = generation.wrapping_add(1);
}

fn cached_profile(
    cache: &Mutex<ProfileCache>,
    provider: &str,
    id: &str,
    declaration: &codeterm_plugin::agent_runtime::RuntimeLaunchProfile,
    generation: u64,
    observe: impl FnOnce() -> Option<bool>,
) -> Option<bool> {
    let key = format!("{provider}:{id}");
    let revision = (declaration.clone(), generation);
    if let Some(installed) = cache.lock().get(&key, &revision) {
        return installed;
    }
    let installed = observe();
    cache.lock().insert(key, revision, installed);
    installed
}

pub(super) fn observe(
    id: &str,
    capability: &AgentRuntimeCapability,
    cache: &Mutex<ProfileCache>,
) -> Option<ProviderCliStatus> {
    observe_from_root(
        id,
        capability,
        launch_profile_install::profile_root(id),
        cache,
    )
}

fn observe_from_root(
    id: &str,
    capability: &AgentRuntimeCapability,
    root: Result<std::path::PathBuf, String>,
    cache: &Mutex<ProfileCache>,
) -> Option<ProviderCliStatus> {
    let profiles = capability.launch_profiles.as_ref()?;
    let generation = root.as_deref().map(revision).unwrap_or_default();
    Some(status_from_profiles(
        profiles
            .iter()
            .map(|(name, profile)| {
                let installed = cached_profile(cache, id, name, profile, generation, || {
                    observe_install(&root, name, profile)
                });
                ProviderLaunchProfileStatus {
                    id: name.clone(),
                    installed,
                }
            })
            .collect(),
    ))
}

fn observe_install(
    root: &Result<std::path::PathBuf, String>,
    id: &str,
    profile: &codeterm_plugin::agent_runtime::RuntimeLaunchProfile,
) -> Option<bool> {
    match root
        .as_ref()
        .map(|root| launch_profile_install::detect(root, id, profile))
    {
        Ok(InstallState::Installed | InstallState::Detected) => Some(true),
        Ok(InstallState::NotInstalled) => Some(false),
        Ok(InstallState::Unknown) | Err(_) => None,
    }
}

#[cfg(test)]
mod tests;
