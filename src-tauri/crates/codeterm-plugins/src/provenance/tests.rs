use super::*;

#[test]
fn record_free_bundled_plugin_is_system_channel() {
    let source = classify_plugin(None, None, Some(("browser", Some("1.12.4"))))
        .source
        .unwrap();
    assert!(
        matches!(source, InstallSource::SystemChannel { channel, path }
        if channel == SYSTEM_CHANNEL_NAME && path == "browser")
    );
    assert!(is_reserved_system_id("browser"));
}

#[test]
fn independent_bundled_version_is_not_the_host_version() {
    let provenance = classify_plugin(None, None, Some(("browser", Some("1.12.4"))));
    assert_eq!(provenance.version.as_deref(), Some("1.12.4"));
    let newer = classify_plugin(None, Some("2.0.0"), Some(("browser", Some("1.12.4"))));
    assert_eq!(newer.version.as_deref(), Some("2.0.0"));
}

#[test]
fn user_install_retains_recorded_provenance() {
    let source = InstallSource::Dir {
        path: "/custom/plugin".into(),
    };
    assert!(
        matches!(classify_plugin(Some(&source), Some("3.0.0"), None).source,
        Some(InstallSource::Dir { path }) if path == "/custom/plugin")
    );
}

#[test]
fn unknown_directory_stays_unknown() {
    assert!(classify_plugin(None, None, None).source.is_none());
    assert!(effective_install_source("unknown-fixture", None).is_none());
    assert_eq!(
        installed_update_status("unknown-fixture", Some("1.0.0"), None)["reason"],
        "unknown_source"
    );
}

#[test]
fn record_free_browser_inventory_uses_bundled_version_and_update_source() {
    let root = tempfile::tempdir().unwrap();
    let _context = enter_ctx(PluginCtx {
        data_root: root.path().into(),
        version: Some("9999.0.0".into()),
    });
    let (_, manifest) = bundled_metadata("browser").unwrap();
    let directory = root.path().join("plugins/browser");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("plugin.json"),
        serde_json::json!({ "id": "browser", "version": manifest.version }).to_string(),
    )
    .unwrap();
    let listed = list_installed().unwrap();
    let row = listed["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "browser")
        .unwrap();
    assert_eq!(row["system"], true);
    assert_eq!(row["state"], "bundled");
    assert_eq!(row["removable"], false);
    assert_eq!(row["version"].as_str(), manifest.version.as_deref());
    assert_eq!(row["source"]["kind"], "system_channel");
    assert_eq!(row["source"]["path"], "browser");
    assert_eq!(row["update"]["reason"], "same_version");
    assert_eq!(row["update"]["basis"], "bundled_manifest");
    let virtual_row = virtual_system_row("browser");
    assert_eq!(virtual_row.version, manifest.version);
    assert_eq!(virtual_row.update["reason"], "same_version");
}

#[test]
fn recorded_local_source_is_not_reclassified_as_core() {
    let source = InstallSource::Dir {
        path: "/local/browser".into(),
    };
    let classified = classify_plugin(
        Some(&source),
        Some("9.0.0"),
        Some(("browser", Some("1.12.4"))),
    );
    assert!(classified.system);
    assert_eq!(classified.version.as_deref(), Some("9.0.0"));
    assert!(matches!(classified.source, Some(InstallSource::Dir { .. })));
}

#[test]
fn record_free_browser_update_compares_plugin_versions() {
    let root = tempfile::tempdir().unwrap();
    let _context = enter_ctx(PluginCtx {
        data_root: root.path().into(),
        version: Some("9999.0.0".into()),
    });
    let (_, manifest) = bundled_metadata("browser").unwrap();
    let current = installed_update_status("browser", manifest.version.as_deref(), None);
    assert_eq!(current["reason"], "same_version");
    let old = installed_update_status("browser", Some("0.0.1"), None);
    assert_eq!(old["reason"], "newer_version");
    // Core plugins update silently, so a newer channel version is never offered.
    assert_eq!(old["available"], false);
    let newer = installed_update_status("browser", Some("9999.0.0"), None);
    assert_eq!(newer["reason"], "local_ahead");
    assert_eq!(newer["available"], false);
}
