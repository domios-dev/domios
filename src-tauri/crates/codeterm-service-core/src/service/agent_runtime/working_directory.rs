//! A launch profile may start its agent process in a host-owned directory under the
//! provider's profile data while its sessions keep the tab's working directory, so
//! startup housekeeping an agent scopes to its process directory never reaches projects.

use super::model::RuntimeFailure;
use codeterm_plugin::agent_runtime::RuntimeLaunchProfile;

const DIRECTORY: &str = "launch-cwd";

pub(super) fn resolve(
    provider: &str,
    profile_name: &str,
    profile: &RuntimeLaunchProfile,
) -> Result<Option<String>, RuntimeFailure> {
    if !profile.host_working_directory {
        return Ok(None);
    }
    let directory = super::launch_profile_install::profile_root(provider)
        .map_err(RuntimeFailure::Refused)?
        .join(profile_name)
        .join(DIRECTORY);
    std::fs::create_dir_all(&directory).map_err(|_| {
        RuntimeFailure::Refused("Profile working directory could not be created".into())
    })?;
    Ok(Some(directory.to_string_lossy().into_owned()))
}
