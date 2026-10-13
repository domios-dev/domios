use super::*;

#[test]
fn list_and_update_agree_and_equal_hash_drift_never_reinstalls() {
    with_data_root(|root| {
        let source = root.join("source");
        write_channel(&source, "store", &[("demo", "demo")]);
        add_channel(&source.to_string_lossy(), Some("store".into())).unwrap();
        install_command(Some("demo".into()), None, None, Some("store".into())).unwrap();
        let before = read_install_record("demo").unwrap().installed_at;
        let manifest_path = source.join("demo/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
        manifest["displayName"] = serde_json::json!("Changed metadata");
        fs::write(&manifest_path, manifest.to_string()).unwrap();
        refresh_channel(Some("store")).unwrap();
        assert_eq!(
            channel_diff(Some("store")).unwrap()["diff"][0]["drift"],
            "hash_drift"
        );
        let status = installed_update_status(
            "demo",
            Some("1.0.0"),
            Some(&read_install_record("demo").unwrap().source),
        );
        assert_eq!(status["available"], false);
        assert_eq!(status["reason"], "same_version");
        assert!(update_one("demo").unwrap_err().contains("same_version"));
        assert_eq!(read_install_record("demo").unwrap().installed_at, before);
        manifest["version"] = serde_json::json!("1.1.0");
        fs::write(&manifest_path, manifest.to_string()).unwrap();
        let mut channel: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(source.join("channel.json")).unwrap())
                .unwrap();
        channel["plugins"][0]["version"] = serde_json::json!("1.1.0");
        fs::write(source.join("channel.json"), channel.to_string()).unwrap();
        refresh_channel(Some("store")).unwrap();
        let listed = list_installed().unwrap();
        let demo = listed["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "demo")
            .unwrap();
        assert_eq!(demo["update"]["available"], true);
        update_one("demo").unwrap();
        assert_eq!(
            manifest_version(&plugin_dir(&PluginId::validated("demo")))
                .unwrap()
                .as_deref(),
            Some("1.1.0")
        );
        assert_eq!(
            installed_update_status(
                "demo",
                Some("1.1.0"),
                Some(&read_install_record("demo").unwrap().source)
            )["reason"],
            "same_version"
        );
    });
}

#[test]
fn local_and_unknown_channel_installs_are_not_updatable() {
    with_data_root(|root| {
        let source = root.join("source");
        write_channel(&source, "store", &[("demo", "demo")]);
        install_command(
            None,
            Some(source.join("demo").to_string_lossy().into_owned()),
            None,
            None,
        )
        .unwrap();
        let record = read_install_record("demo").unwrap();
        assert_eq!(
            installed_update_status("demo", Some("1.0.0"), Some(&record.source))["reason"],
            "local_install"
        );
        assert!(update_one("demo").unwrap_err().contains("local_install"));
        let source = InstallSource::Channel {
            channel: "missing".into(),
            path: "demo".into(),
        };
        assert_eq!(
            installed_update_status("demo", Some("1.0.0"), Some(&source))["reason"],
            "channel_unavailable"
        );
        let record = InstallRecord { source, ..record };
        let installed = plugin_dir(&PluginId::validated("demo"));
        fs::write(
            installed.join(".codeterm-install.json"),
            serde_json::to_string(&record).unwrap(),
        )
        .unwrap();
        assert!(update_one("demo").unwrap_err().contains("not registered"));
        fs::remove_file(installed.join(".codeterm-install.json")).unwrap();
        assert!(update_one("demo").unwrap_err().contains("unknown_source"));
    });
}

#[test]
fn system_sync_repairs_equal_versions_but_never_downgrades() {
    with_data_root(|root| {
        let source = root.join("system-source");
        write_channel(&source, "store", &[("demo", "demo")]);
        std::env::set_var("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL", &source);
        update_system_command(None).unwrap();
        let manifest_path = source.join("demo/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
        manifest["displayName"] = serde_json::json!("Repaired");
        fs::write(&manifest_path, manifest.to_string()).unwrap();
        update_system_command(None).unwrap();
        let installed = plugin_dir(&PluginId::validated("demo"));
        assert_eq!(
            read_plugin_manifest(&installed)
                .unwrap()
                .display_name
                .as_deref(),
            Some("Repaired")
        );
        let mut installed_manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(installed.join("plugin.json")).unwrap())
                .unwrap();
        installed_manifest["version"] = serde_json::json!("2.0.0");
        fs::write(
            installed.join("plugin.json"),
            installed_manifest.to_string(),
        )
        .unwrap();
        assert!(update_system_command(None)
            .unwrap_err()
            .contains("local_ahead"));
        assert_eq!(
            manifest_version(&installed).unwrap().as_deref(),
            Some("2.0.0")
        );
    });
}

#[test]
fn record_free_bundled_browser_updates_from_system_channel() {
    with_data_root(|root| {
        let source = root.join("system-source");
        write_channel(&source, SYSTEM_CHANNEL_NAME, &[("browser", "browser")]);
        std::env::set_var("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL", &source);
        let installed = plugins_root().join("browser");
        fs::create_dir_all(&installed).unwrap();
        fs::write(
            installed.join("plugin.json"),
            serde_json::json!({ "id": "browser", "version": "0.0.1", "entry": "plugin.js" })
                .to_string(),
        )
        .unwrap();
        fs::write(installed.join("plugin.js"), "module.exports = {};").unwrap();
        assert!(read_install_record("browser").is_err());
        update_one("browser").unwrap();
        assert_eq!(
            manifest_version(&installed).unwrap().as_deref(),
            Some("1.0.0")
        );
        let record = read_install_record("browser").unwrap();
        assert_eq!(record.id, "browser");
        assert!(
            matches!(record.source, InstallSource::SystemChannel { channel, path }
            if channel == SYSTEM_CHANNEL_NAME && path == "browser")
        );
    });
}

#[cfg(debug_assertions)]
#[test]
fn dev_fixture_refresh_update_and_persisted_up_to_date() {
    with_data_root(|root| {
        struct Restore(Option<std::ffi::OsString>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => std::env::set_var("CODETERM_DEV_UPDATE_FIXTURES", value),
                    None => std::env::remove_var("CODETERM_DEV_UPDATE_FIXTURES"),
                }
            }
        }
        let _restore = Restore(std::env::var_os("CODETERM_DEV_UPDATE_FIXTURES"));
        let fixtures = root.join("fixtures");
        let source = fixtures.join("channels/store");
        write_channel(&source, "store", &[("lark", "lark")]);
        std::env::set_var("CODETERM_DEV_UPDATE_FIXTURES", &fixtures);
        add_channel("https://fixture.invalid/channel.git", Some("store".into())).unwrap();
        install_command(Some("lark".into()), None, None, Some("store".into())).unwrap();
        let payload = source.join("lark/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&payload).unwrap()).unwrap();
        manifest["version"] = "1.0.1".into();
        fs::write(&payload, manifest.to_string()).unwrap();
        let channel = source.join("channel.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&channel).unwrap()).unwrap();
        manifest["plugins"][0]["version"] = "1.0.1".into();
        fs::write(channel, manifest.to_string()).unwrap();
        assert_eq!(
            installed_update_status(
                "lark",
                Some("1.0.0"),
                Some(&read_install_record("lark").unwrap().source)
            )["availability"]["state"],
            "up_to_date"
        );
        refresh_channel(Some("store")).unwrap();
        let listed = list_installed().unwrap();
        let item = listed["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "lark")
            .unwrap();
        assert_eq!(item["update"]["availability"]["state"], "update_available");
        assert_eq!(
            channel_diff(Some("store")).unwrap()["diff"][0]["availability"],
            item["update"]["availability"]
        );
        update_one("lark").unwrap();
        assert_eq!(
            read_plugin_manifest(&plugin_dir(&PluginId::validated("lark")))
                .unwrap()
                .version
                .as_deref(),
            Some("1.0.1")
        );
        let record = read_install_record("lark").unwrap();
        assert_eq!(
            record.last_change.unwrap().to_version.as_deref(),
            Some("1.0.1")
        );
        assert_eq!(
            installed_update_status("lark", Some("1.0.1"), Some(&record.source))["availability"]
                ["state"],
            "up_to_date"
        );
    });
}

#[test]
fn aggregate_refresh_preserves_other_channels_but_reports_failure() {
    with_data_root(|root| {
        let source = root.join("source");
        write_channel(&source, "store", &[("demo", "demo")]);
        add_channel(&source.to_string_lossy(), Some("store".into())).unwrap();
        fs::remove_dir_all(&source).unwrap();
        let error = refresh_channel(None).unwrap_err();
        assert!(error.contains("store"));
        assert_eq!(
            channel_diff(Some("store")).unwrap()["diff"][0]["remote_version"],
            "1.0.0"
        );
    });
}
