//! A launch profile may declare JSON settings that its agent reads from a file
//! named by an environment variable. The host owns those files under the
//! provider's profile data, so the owner's own agent settings stay untouched.

use super::model::RuntimeFailure;
use codeterm_plugin::agent_runtime::RuntimeLaunchProfile;
use std::{collections::BTreeMap, path::Path};

pub(super) fn environment(
    provider: &str,
    profile_name: &str,
    profile: &RuntimeLaunchProfile,
) -> Result<BTreeMap<String, String>, RuntimeFailure> {
    if profile.settings_files.is_empty() {
        return Ok(BTreeMap::new());
    }
    let root = super::launch_profile_install::profile_root(provider)
        .map_err(RuntimeFailure::Refused)?
        .join(profile_name)
        .join("settings");
    write_all(&root, profile)
}

fn write_all(
    root: &Path,
    profile: &RuntimeLaunchProfile,
) -> Result<BTreeMap<String, String>, RuntimeFailure> {
    let failed = |_| RuntimeFailure::Refused("Profile settings file could not be written".into());
    std::fs::create_dir_all(root).map_err(failed)?;
    profile
        .settings_files
        .iter()
        .map(|(variable, settings)| {
            let path = root.join(format!("{variable}.json"));
            let bytes = serde_json::to_vec_pretty(settings).map_err(|_| {
                RuntimeFailure::Refused("Profile settings file is not valid JSON".into())
            })?;
            write_if_changed(&path, &bytes).map_err(failed)?;
            Ok((variable.clone(), path.to_string_lossy().into_owned()))
        })
        .collect()
}

/// Concurrent launches write identical bytes; a rename that loses to a reader
/// still succeeds when the file already holds them.
fn write_if_changed(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let current = |path: &Path| std::fs::read(path).is_ok_and(|current| current == bytes);
    if current(path) {
        return Ok(());
    }
    let staging = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    std::fs::write(&staging, bytes)?;
    let renamed = std::fs::rename(&staging, path);
    if renamed.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    match renamed {
        Err(_) if current(path) => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests;
