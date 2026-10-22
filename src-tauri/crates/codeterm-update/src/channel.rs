pub use codeterm_types::update::channel::Channel;

use super::checker::GITHUB_REPO;
use super::InstallMethod;

pub fn manifest_filename(c: Channel) -> &'static str {
    match c {
        Channel::Stable => "domios-latest.json",
        Channel::Rc => "domios-latest-rc.json",
        Channel::Nightly => "domios-latest-nightly.json",
    }
}

pub fn manifest_filename_for_install_method(c: Channel, method: &InstallMethod) -> &'static str {
    match method {
        InstallMethod::Deb => match c {
            Channel::Stable => "domios-latest-deb.json",
            Channel::Rc => "domios-latest-rc-deb.json",
            Channel::Nightly => "domios-latest-nightly-deb.json",
        },
        InstallMethod::Rpm => match c {
            Channel::Stable => "domios-latest-rpm.json",
            Channel::Rc => "domios-latest-rc-rpm.json",
            Channel::Nightly => "domios-latest-nightly-rpm.json",
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
mod tests;
