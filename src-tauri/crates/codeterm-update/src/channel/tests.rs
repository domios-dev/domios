use super::*;

#[test]
fn manifest_filename_mapping() {
    assert_eq!(manifest_filename(Channel::Stable), "domios-latest.json");
    assert_eq!(manifest_filename(Channel::Rc), "domios-latest-rc.json");
    assert_eq!(
        manifest_filename(Channel::Nightly),
        "domios-latest-nightly.json"
    );
}

#[test]
fn package_install_methods_use_package_specific_manifests() {
    assert_eq!(
        manifest_filename_for_install_method(Channel::Stable, &InstallMethod::Deb),
        "domios-latest-deb.json"
    );
    assert_eq!(
        manifest_filename_for_install_method(Channel::Rc, &InstallMethod::Deb),
        "domios-latest-rc-deb.json"
    );
    assert_eq!(
        manifest_filename_for_install_method(Channel::Nightly, &InstallMethod::Rpm),
        "domios-latest-nightly-rpm.json"
    );
    assert_eq!(
        manifest_filename_for_install_method(Channel::Stable, &InstallMethod::AppImage),
        "domios-latest.json"
    );
}

#[test]
fn default_endpoints_format() {
    assert_eq!(
        default_endpoint(Channel::Stable),
        "https://github.com/domios-dev/domios/releases/latest/download/domios-latest.json"
    );
    assert!(default_endpoint(Channel::Rc).ends_with("/domios-latest-rc.json"));
    assert!(default_endpoint(Channel::Nightly).ends_with("/domios-latest-nightly.json"));
    assert_eq!(
        default_endpoint_for_install_method(Channel::Stable, &InstallMethod::Deb),
        "https://github.com/domios-dev/domios/releases/latest/download/domios-latest-deb.json"
    );
}

#[test]
fn immutable_endpoint_pins_the_discovered_release() {
    assert_eq!(
        immutable_endpoint_for_release(
            "domios-dev/domios",
            "v1.19.0",
            Channel::Stable,
            &InstallMethod::Nsis
        ),
        "https://github.com/domios-dev/domios/releases/download/v1.19.0/domios-latest.json"
    );
    assert_eq!(
        immutable_endpoint_for_release(
            "domios-dev/domios",
            "v1.16.0",
            Channel::Stable,
            &InstallMethod::Deb
        ),
        "https://github.com/domios-dev/domios/releases/download/v1.16.0/domios-latest-deb.json"
    );
}
