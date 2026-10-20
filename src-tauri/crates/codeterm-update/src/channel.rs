pub use codeterm_types::update::channel::Channel;

use super::checker::GITHUB_REPO;
use super::InstallMethod;

pub fn manifest_filename(c: Channel) -> &'static str {
    match c {
        Channel::Stable => "latest.json",
        Channel::Rc => "latest-rc.json",
        Channel::Nightly => "latest-nightly.json",
    }
}

pub fn manifest_filename_for_install_method(c: Channel, method: &InstallMethod) -> &'static str {
    match method {
        InstallMethod::Deb => match c {
            Channel::Stable => "latest-deb.json",
            Channel::Rc => "latest-rc-deb.json",
            Channel::Nightly => "latest-nightly-deb.json",
        },
        InstallMethod::Rpm => match c {
            Channel::Stable => "latest-rpm.json",
            Channel::Rc => "latest-rc-rpm.json",
            Channel::Nightly => "latest-nightly-rpm.json",
        },
        _ => manifest_filename(c),
    }
}

pub fn default_endpoints(c: Channel) -> Vec<String> {
    default_endpoints_for_install_method(c, &InstallMethod::Unknown)
}

pub fn default_endpoints_for_install_method(c: Channel, method: &InstallMethod) -> Vec<String> {
    vec![format!(
        "https://github.com/{}/releases/latest/download/{}",
        GITHUB_REPO,
        manifest_filename_for_install_method(c, method)
    )]
}

pub fn immutable_endpoint_for_release(
    tag: &str,
    channel: Channel,
    method: &InstallMethod,
) -> String {
    format!(
        "https://github.com/{}/releases/download/{}/{}",
        GITHUB_REPO,
        tag,
        manifest_filename_for_install_method(channel, method)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_filename_mapping() {
        assert_eq!(manifest_filename(Channel::Stable), "latest.json");
        assert_eq!(manifest_filename(Channel::Rc), "latest-rc.json");
        assert_eq!(manifest_filename(Channel::Nightly), "latest-nightly.json");
    }

    #[test]
    fn package_install_methods_use_package_specific_manifests() {
        assert_eq!(
            manifest_filename_for_install_method(Channel::Stable, &InstallMethod::Deb),
            "latest-deb.json"
        );
        assert_eq!(
            manifest_filename_for_install_method(Channel::Rc, &InstallMethod::Deb),
            "latest-rc-deb.json"
        );
        assert_eq!(
            manifest_filename_for_install_method(Channel::Nightly, &InstallMethod::Rpm),
            "latest-nightly-rpm.json"
        );
        assert_eq!(
            manifest_filename_for_install_method(Channel::Stable, &InstallMethod::AppImage),
            "latest.json"
        );
    }

    #[test]
    fn default_endpoints_format() {
        let stable = default_endpoints(Channel::Stable);
        assert_eq!(stable.len(), 1);
        assert_eq!(
            stable[0],
            "https://github.com/rollacode/codeterm-releases/releases/latest/download/latest.json"
        );

        let rc = default_endpoints(Channel::Rc);
        assert!(rc[0].ends_with("/latest-rc.json"));

        let nightly = default_endpoints(Channel::Nightly);
        assert!(nightly[0].ends_with("/latest-nightly.json"));

        let deb = default_endpoints_for_install_method(Channel::Stable, &InstallMethod::Deb);
        assert_eq!(
            deb[0],
            "https://github.com/rollacode/codeterm-releases/releases/latest/download/latest-deb.json"
        );
    }

    #[test]
    fn immutable_endpoint_pins_the_discovered_release() {
        assert_eq!(
            immutable_endpoint_for_release("v1.7.7", Channel::Stable, &InstallMethod::Nsis),
            "https://github.com/rollacode/codeterm-releases/releases/download/v1.7.7/latest.json"
        );
        assert_eq!(
            immutable_endpoint_for_release("v1.7.7", Channel::Stable, &InstallMethod::Deb),
            "https://github.com/rollacode/codeterm-releases/releases/download/v1.7.7/latest-deb.json"
        );
    }
}
