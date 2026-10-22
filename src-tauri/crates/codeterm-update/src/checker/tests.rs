use super::*;

#[test]
fn latest_release_request_bypasses_mutable_github_cache() {
    let request = latest_release_request(&reqwest::Client::new())
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
fn debian_command_uses_manifest_asset_for_either_architecture() {
    for arch in ["amd64", "arm64"] {
        let url = format!("https://mirror.example/v1.15.2/domios_{arch}.deb");
        let command = build_update_command(&InstallMethod::Deb, "1.15.2", Some(&url)).unwrap();
        assert_eq!(command, format!("curl --fail --location --output domios-update.deb -- '{url}' && sudo dpkg -i ./domios-update.deb"));
    }
    assert!(build_update_command(&InstallMethod::Deb, "1.15.2", None).is_none());
    assert!(build_update_command(
        &InstallMethod::AppImage,
        "1.15.2",
        Some("https://example/app")
    )
    .is_none());
}

fn info(method: InstallMethod, available: bool) -> UpdateInfo {
    UpdateInfo {
        availability: codeterm_plugins::AvailabilityItem::release(
            "app",
            codeterm_plugins::UpdateKind::App,
            "1.15.0",
            "1.15.2",
            true,
        ),
        current_version: "1.15.0".into(),
        latest_version: "1.15.2".into(),
        update_available: available,
        update_command: build_update_command(
            &method,
            "1.15.2",
            Some("https://mirror.example/pkg.deb"),
        ),
        install_method: method,
        download_url: Some("https://mirror.example/pkg.deb".into()),
        release_url: "https://mirror.example/release".into(),
        release_notes: None,
        published_at: None,
    }
}

#[test]
fn package_guidance_requires_a_debian_update_and_a_real_asset() {
    let (command, message) = debian_install_guidance(&info(InstallMethod::Deb, true))
        .unwrap()
        .unwrap();
    assert!(message.contains(&command));
    assert!(debian_install_guidance(&info(InstallMethod::Deb, false))
        .unwrap()
        .is_none());
    assert!(
        debian_install_guidance(&info(InstallMethod::AppImage, true))
            .unwrap()
            .is_none()
    );
    let mut missing = info(InstallMethod::Deb, true);
    missing.update_command = None;
    assert!(debian_install_guidance(&missing).is_err());
}

#[test]
fn debian_command_quotes_remote_shell_metacharacters() {
    let command = build_update_command(
        &InstallMethod::Deb,
        "1.15.2",
        Some("https://example/a'$(touch bad).deb"),
    )
    .unwrap();
    assert!(command.contains(r#"'https://example/a'\''$(touch bad).deb'"#));
}
