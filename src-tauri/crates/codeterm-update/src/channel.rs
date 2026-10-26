pub use codeterm_types::update::channel::Channel;

use super::checker::UPDATE_REPOSITORIES;
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

/// One endpoint per update repository, public source repository first.
pub fn default_endpoints_for_install_method(c: Channel, method: &InstallMethod) -> Vec<String> {
    UPDATE_REPOSITORIES
        .iter()
        .map(|repository| {
            format!(
                "https://github.com/{repository}/releases/latest/download/{}",
                manifest_filename_for_install_method(c, method)
            )
        })
        .collect()
}

pub fn immutable_endpoint_for_release(
    repository: &str,
    tag: &str,
    channel: Channel,
    method: &InstallMethod,
) -> String {
    format!(
        "https://github.com/{repository}/releases/download/{tag}/{}",
        manifest_filename_for_install_method(channel, method)
    )
}

#[cfg(test)]
mod tests;
