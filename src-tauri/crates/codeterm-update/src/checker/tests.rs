use super::*;

#[test]
fn package_manager_commands_select_domios_desktop_packages() {
    for (method, command) in [
        (
            InstallMethod::Brew,
            "brew upgrade --cask domios-dev/domios/domios",
        ),
        (InstallMethod::Scoop, "scoop update domios"),
    ] {
        assert_eq!(
            build_update_command(&method, "1.16.1", None).as_deref(),
            Some(command)
        );
    }
}

#[test]
fn latest_release_request_bypasses_mutable_github_cache() {
    let request = latest_release_request(&reqwest::Client::new(), "domios-dev/domios")
        .build()
        .unwrap();
    assert_eq!(
        request.headers()[reqwest::header::CACHE_CONTROL],
        "no-cache, no-store"
    );
    assert_eq!(request.headers()[reqwest::header::PRAGMA], "no-cache");
    assert!(request
        .url()
        .query_pairs()
        .any(|(key, value)| key == "codeterm_cache_bust" && !value.is_empty()));
}

#[test]
fn debian_discovery_never_advertises_an_unverified_install_command() {
    assert!(build_update_command(
        &InstallMethod::Deb,
        "1.15.2",
        Some("https://example/pkg.deb")
    )
    .is_none());
}

#[test]
fn manifest_url_override_requires_https_except_explicit_local_test_builds() {
    assert!(crate::manifest::validate_url("https://example/manifest.json").is_ok());
    for url in [
        "http://evil.example/manifest.json",
        "ftp://example/manifest.json",
        "http://localhost.evil/manifest.json",
        "http://localhost@evil/manifest.json",
    ] {
        assert!(crate::manifest::validate_url(url).is_err());
    }
    for url in [
        "http://localhost/manifest.json",
        "http://127.0.0.1/manifest.json",
        "file:///manifest.json",
    ] {
        assert_eq!(
            crate::manifest::validate_url(url).is_ok(),
            cfg!(feature = "dev-updater-allowlocal")
        );
    }
}
