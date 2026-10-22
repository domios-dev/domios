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
    let stable = default_endpoints(Channel::Stable);
    assert_eq!(stable.len(), 1);
    assert_eq!(
        stable[0],
        "https://github.com/rollacode/codeterm-releases/releases/latest/download/domios-latest.json"
    );

    let rc = default_endpoints(Channel::Rc);
    assert!(rc[0].ends_with("/domios-latest-rc.json"));

    let nightly = default_endpoints(Channel::Nightly);
    assert!(nightly[0].ends_with("/domios-latest-nightly.json"));

    let deb = default_endpoints_for_install_method(Channel::Stable, &InstallMethod::Deb);
    assert_eq!(
        deb[0],
        "https://github.com/rollacode/codeterm-releases/releases/latest/download/domios-latest-deb.json"
    );
}

#[test]
fn immutable_endpoint_pins_the_discovered_release() {
    assert_eq!(
        immutable_endpoint_for_release("v1.7.7", Channel::Stable, &InstallMethod::Nsis),
        "https://github.com/rollacode/codeterm-releases/releases/download/v1.7.7/domios-latest.json"
    );
    assert_eq!(
        immutable_endpoint_for_release("v1.7.7", Channel::Stable, &InstallMethod::Deb),
        "https://github.com/rollacode/codeterm-releases/releases/download/v1.7.7/domios-latest-deb.json"
    );
}
