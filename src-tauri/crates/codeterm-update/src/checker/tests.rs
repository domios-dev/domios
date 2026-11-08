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
fn release_resolution_uses_published_asset_urls() {
    assert_eq!(
        super::super::channel::default_endpoint(super::super::Channel::Stable),
        "https://github.com/domios-dev/domios/releases/latest/download/domios-latest.json"
    );
}

#[tokio::test]
async fn discovery_reads_only_the_main_repository() {
    for feed_fails in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut calls = Vec::new();
        let result = resolve_with(
            &InstallMethod::Unknown,
            super::super::Channel::Stable,
            root.path().into(),
            |url| {
                calls.push(url.clone());
                std::future::ready(if feed_fails {
                    Err("offline".into())
                } else {
                    Ok(resolved("9.0.0").manifest)
                })
            },
        )
        .await;
        assert_eq!(
            calls,
            vec![
                "https://github.com/domios-dev/domios/releases/latest/download/domios-latest.json"
            ]
        );
        match result {
            Ok(result) => {
                assert!(!feed_fails);
                assert_eq!(result.release.repository, UPDATE_REPOSITORY);
                assert_eq!(result.manifest.version, "9.0.0");
            }
            Err(error) => {
                assert!(feed_fails);
                assert!(error.contains(UPDATE_REPOSITORY));
            }
        }
    }
}

const THROTTLE_START_MS: u64 = 1_800_000_000_000;

#[tokio::test]
async fn an_automatic_check_inside_its_interval_stays_off_the_network_even_after_a_restart() {
    let root = tempfile::tempdir().unwrap();
    let calls = std::sync::atomic::AtomicU32::new(0);
    let gated = |trigger: CheckTrigger, now: u64| {
        let root = root.path().to_path_buf();
        let calls = &calls;
        async move {
            gated_manifest(
                &InstallMethod::Unknown,
                super::super::Channel::Stable,
                trigger,
                root,
                now,
                |_endpoint| async move {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(resolved("9.0.0").manifest)
                },
            )
            .await
        }
    };
    let count = || calls.load(std::sync::atomic::Ordering::SeqCst);

    gated(CheckTrigger::Automatic, THROTTLE_START_MS)
        .await
        .unwrap();
    assert_eq!(count(), 1);

    // Every call rebuilds its state from disk, as a restarted process does.
    let cached = gated(CheckTrigger::Automatic, THROTTLE_START_MS + 60_000)
        .await
        .unwrap();
    assert_eq!(count(), 1);
    assert_eq!(cached.manifest.version, "9.0.0");

    gated(CheckTrigger::Explicit, THROTTLE_START_MS + 120_000)
        .await
        .unwrap();
    assert_eq!(count(), 2);

    gated(
        CheckTrigger::Automatic,
        THROTTLE_START_MS + 30 * 60_000 + 120_000,
    )
    .await
    .unwrap();
    assert_eq!(count(), 3);
}

#[tokio::test]
async fn a_failed_automatic_check_still_spends_its_slot_and_leaves_nothing_to_report() {
    let root = tempfile::tempdir().unwrap();
    let attempt = |now: u64| {
        gated_manifest(
            &InstallMethod::Unknown,
            super::super::Channel::Stable,
            CheckTrigger::Automatic,
            root.path().to_path_buf(),
            now,
            |_endpoint| async { Err("HTTP 504".to_string()) },
        )
    };
    assert!(attempt(THROTTLE_START_MS)
        .await
        .err()
        .is_some_and(|error| error.contains("HTTP 504")));
    assert_eq!(
        attempt(THROTTLE_START_MS + 60_000).await.err().as_deref(),
        Some(CHECK_DEFERRED)
    );
}

#[test]
fn failed_refresh_preserves_manifest_and_reports_failure() {
    let cached = ResolvedManifest {
        release: LatestRelease {
            repository: UPDATE_REPOSITORY,
            tag: "v9.0.0".into(),
        },
        manifest: serde_json::from_value(serde_json::json!({
            "version": "9.0.0", "platforms": {"darwin-aarch64": {
                "url": "https://example.com/app.tar.gz", "signature": "retained-signature"
            }}
        }))
        .unwrap(),
        refresh_error: None,
    };
    let result = reuse_manifest(None, Some(cached.clone()), &["offline".into()]).unwrap();
    assert_eq!(result.manifest, cached.manifest);
    assert_eq!(result.release, cached.release);
    assert_eq!(
        result.refresh_error.as_deref(),
        Some("Failed to fetch release: offline")
    );
    assert!(reuse_manifest(None, None, &["offline".into()]).is_err());
    let fresh = reuse_manifest(Some(cached), Some(result), &[]).unwrap();
    assert_eq!(fresh.refresh_error, None);
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

fn resolved(version: &str) -> ResolvedManifest {
    ResolvedManifest {
        release: LatestRelease {
            repository: UPDATE_REPOSITORY,
            tag: format!("v{version}"),
        },
        manifest: serde_json::from_value(serde_json::json!({
            "version": version, "platforms": {"linux-x86_64": {
                "url": "https://example.com/app.deb", "signature": "retained-signature"
            }}
        }))
        .unwrap(),
        refresh_error: None,
    }
}

#[test]
fn older_mirror_and_late_older_completion_cannot_erase_newer_offer() {
    let root = tempfile::tempdir().unwrap();
    let key = "domios-latest.json";
    cache::publish(root.path(), key, Some(resolved("9.0.0")), &[]).unwrap();
    let partial = cache::publish(
        root.path(),
        key,
        Some(resolved("8.0.0")),
        &["source offline".into()],
    )
    .unwrap();
    assert_eq!(partial.manifest.version, "9.0.0");
    assert!(partial.refresh_error.unwrap().contains("source offline"));
    let late = cache::publish(root.path(), key, Some(resolved("7.0.0")), &[]).unwrap();
    assert_eq!(late.manifest.version, "9.0.0");
    assert!(late.refresh_error.is_some());
    let equal = cache::publish(root.path(), key, Some(resolved("9.0.0")), &[]).unwrap();
    assert_eq!(equal.refresh_error, None);
    let newer = cache::publish(root.path(), key, Some(resolved("10.0.0")), &[]).unwrap();
    assert_eq!(newer.manifest.version, "10.0.0");
    assert_eq!(newer.refresh_error, None);
}

#[tokio::test]
async fn install_resolution_restores_exact_metadata_after_all_feed_reads_fail() {
    let root = tempfile::tempdir().unwrap();
    let original = resolved("9.0.0").manifest;
    let seeded = resolve_with(
        &InstallMethod::Deb,
        super::super::Channel::Stable,
        root.path().into(),
        |url| {
            assert!(url.ends_with("/domios-latest-deb.json"));
            std::future::ready(Ok(original.clone()))
        },
    )
    .await
    .unwrap();
    assert_eq!(seeded.refresh_error, None);
    let restored = resolve_with(
        &InstallMethod::Deb,
        super::super::Channel::Stable,
        root.path().into(),
        |_| std::future::ready(Err("offline".into())),
    )
    .await
    .unwrap();
    assert_eq!(restored.manifest, original);
    assert_eq!(
        restored.manifest.platforms["linux-x86_64"].url,
        "https://example.com/app.deb"
    );
    assert_eq!(
        restored.manifest.platforms["linux-x86_64"].signature,
        "retained-signature"
    );
    assert!(restored.refresh_error.is_some());
    assert!(resolve_with(
        &InstallMethod::Unknown,
        super::super::Channel::Stable,
        root.path().into(),
        |_| std::future::ready(Err("offline".into()))
    )
    .await
    .is_err());
    let empty = tempfile::tempdir().unwrap();
    assert!(resolve_with(
        &InstallMethod::Deb,
        super::super::Channel::Stable,
        empty.path().into(),
        |_| std::future::ready(Err("offline".into()))
    )
    .await
    .is_err());
}

#[test]
fn cache_contention_refuses_without_waiting() {
    let root = tempfile::tempdir().unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.path().join("domios-latest.json.lock"))
        .unwrap();
    lock.lock().unwrap();
    assert!(cache::publish(
        root.path(),
        "domios-latest.json",
        Some(resolved("9.0.0")),
        &[]
    )
    .is_err());
}

#[tokio::test]
async fn release_discovery_preserves_channel_and_newest_known_release_when_offline() {
    for channel in [
        crate::Channel::Stable,
        crate::Channel::Rc,
        crate::Channel::Nightly,
    ] {
        let root = tempfile::tempdir().unwrap();
        let expected = crate::channel::default_endpoint(channel);
        let fresh = resolve_with(
            &InstallMethod::Unknown,
            channel,
            root.path().into(),
            |endpoint| {
                assert_eq!(endpoint, expected);
                std::future::ready(Ok(resolved("9.0.0").manifest))
            },
        )
        .await
        .unwrap();
        assert_eq!(fresh.release.repository, UPDATE_REPOSITORY);
        assert_eq!(fresh.release.tag, "v9.0.0");
        let older = resolve_with(
            &InstallMethod::Unknown,
            channel,
            root.path().into(),
            |endpoint| {
                assert_eq!(endpoint, expected);
                std::future::ready(Ok(resolved("8.0.0").manifest))
            },
        )
        .await
        .unwrap();
        assert_eq!(older.release, fresh.release);
        assert_eq!(older.manifest, fresh.manifest);
        assert!(older.refresh_error.is_some());
        let cached = resolve_with(
            &InstallMethod::Unknown,
            channel,
            root.path().into(),
            |endpoint| {
                assert_eq!(endpoint, expected);
                std::future::ready(Err("offline".into()))
            },
        )
        .await
        .unwrap();
        assert_eq!(cached.release, fresh.release);
        assert_eq!(cached.manifest, fresh.manifest);
        assert!(cached.refresh_error.is_some());
    }
}

#[tokio::test]
async fn offline_discovery_never_restores_a_manifest_cached_from_another_repository() {
    let root = tempfile::tempdir().unwrap();
    let foreign = serde_json::json!({
        "repository": "rollacode/codeterm-releases",
        "manifest": serde_json::to_value(resolved("9.0.0").manifest).unwrap(),
    });
    std::fs::write(
        root.path().join("domios-latest.json"),
        serde_json::to_vec(&foreign).unwrap(),
    )
    .unwrap();
    assert!(resolve_with(
        &InstallMethod::Unknown,
        super::super::Channel::Stable,
        root.path().into(),
        |_| std::future::ready(Err("offline".into()))
    )
    .await
    .is_err());
}
