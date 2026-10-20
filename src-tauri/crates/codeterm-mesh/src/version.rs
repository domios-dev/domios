pub const VERSION_HEADER: &str = "x-domios-version";
const UNINITIALIZED_APP_VERSION: &str = "1.15.0";

pub fn local_version() -> &'static str {
    codeterm_build_info::installed_version().unwrap_or(UNINITIALIZED_APP_VERSION)
}

pub fn peer_upgrade_message() -> String {
    peer_upgrade_message_for(local_version())
}

pub fn peer_upgrade_message_for(app_version: &str) -> String {
    format!(
        "Mesh requires both peers on Domios {app_version} or later; upgrade both peers through the updater."
    )
}

pub fn peer_version_error(version: Option<&str>) -> Option<String> {
    peer_version_error_for(local_version(), version)
}

pub fn peer_version_error_for(app_version: &str, version: Option<&str>) -> Option<String> {
    let app_version_parsed = semver::Version::parse(app_version).ok();
    let peer_version_parsed = version.and_then(|version| semver::Version::parse(version).ok());
    let compatible = app_version_parsed
        .zip(peer_version_parsed)
        .is_some_and(|(app_version, peer_version)| peer_version >= app_version);
    if compatible {
        None
    } else {
        let upgrade_message = peer_upgrade_message_for(app_version);
        Some(match version {
            Some(version) => format!("{upgrade_message} Peer reported version {version}."),
            None => upgrade_message,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_version_floor_refuses_1_14_9_with_an_updater_message() {
        let error = peer_version_error_for("1.15.0", Some("1.14.9")).unwrap();
        assert!(error.contains("both peers on Domios 1.15.0"));
        assert!(error.contains("through the updater"));
        assert!(error.contains("1.14.9"));
    }

    #[test]
    fn mesh_version_floor_accepts_current_and_newer_releases() {
        assert!(peer_version_error_for("1.15.0", Some("1.15.0")).is_none());
        assert!(peer_version_error_for("1.15.0", Some("1.16.0")).is_none());
        assert!(peer_version_error_for("1.15.0", Some("1.15.0-rc.1")).is_some());
        assert!(peer_version_error_for("1.15.0", None).is_some());
    }

    #[test]
    fn peer_version_floor_tracks_the_running_app_version() {
        let error = peer_version_error_for("1.16.0", Some("1.15.9")).unwrap();
        assert!(error.contains("Domios 1.16.0"));
        assert!(peer_version_error_for("1.16.0", Some("1.16.0")).is_none());
    }
}
