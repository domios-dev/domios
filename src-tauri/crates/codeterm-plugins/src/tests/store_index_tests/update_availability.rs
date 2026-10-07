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
