use super::*;

#[test]
fn compat_check_blocks_only_when_app_older_than_min() {
    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    let prev = std::env::var("CODETERM_VERSION").ok();

    std::env::set_var("CODETERM_VERSION", "1.3.2");
    assert!(compat_check(Some("1.0.0")).is_ok());
    assert!(compat_check(Some("1.3.2")).is_ok());
    let err = compat_check(Some("2.0.0")).unwrap_err();
    assert!(err.contains("2.0.0") && err.contains("1.3.2"), "got: {err}");

    // Fail-open: unknown running version never blocks.
    std::env::remove_var("CODETERM_VERSION");
    assert!(compat_check(Some("99.0.0")).is_ok());
    std::env::set_var("CODETERM_VERSION", "1.0.0-rc.1");
    assert!(compat_check(Some("1.0.0")).is_err());
    assert!(compat_check(Some("1.0.0-beta.9")).is_ok());
    assert!(compat_check(Some("invalid")).is_err());
    // No declared minimum never blocks.
    std::env::set_var("CODETERM_VERSION", "1.0.0");
    assert!(compat_check(None).is_ok());

    restore_env("CODETERM_VERSION", prev);
}

#[test]
fn install_from_channel_refuses_incompatible_min_codeterm() {
    with_data_root(|_root| {
        // env_lock is already held by with_data_root — set directly, restore at end.
        let prev_v = std::env::var("CODETERM_VERSION").ok();
        std::env::set_var("CODETERM_VERSION", "1.0.0");

        let cache = channels_root().join("needychan");
        std::fs::create_dir_all(cache.join("provider-needy")).unwrap();
        std::fs::write(
            cache.join("channel.json"),
            serde_json::json!({
                "schemaVersion": 1,
                "name": "needychan",
                "plugins": [{"id":"needy","version":"1.0.0","path":"provider-needy","minCodeterm":"2.0.0"}]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            cache.join("provider-needy/plugin.json"),
            serde_json::json!({"id":"needy","version":"1.0.0","minCodeterm":"2.0.0"}).to_string(),
        )
        .unwrap();
        std::fs::write(cache.join("provider-needy/plugin.js"), "module.exports={};").unwrap();
        write_channel_registry(&ChannelRegistry {
            channels: BTreeMap::from([(
                "needychan".to_string(),
                ChannelRecord {
                    name: "needychan".to_string(),
                    url: format!("file://{}", cache.display()),
                    path: cache.display().to_string(),
                    kind: ChannelKind::Local,
                },
            )]),
        })
        .unwrap();

        let err = install_command(
            Some("needy".to_string()),
            None,
            None,
            Some("needychan".to_string()),
        )
        .unwrap_err();
        assert!(err.contains("requires CodeTerm"), "got: {err}");
        assert!(
            !plugin_dir(&PluginId::validated("needy")).exists(),
            "incompatible plugin must not be installed"
        );

        restore_env("CODETERM_VERSION", prev_v);
    });
}

fn write_system_channel(
    dir: &Path,
    plugins: &[(&str, &str, &str, &str)], // (id, path, version, min_codeterm)
) {
    std::fs::create_dir_all(dir).unwrap();
    let entries = plugins
        .iter()
        .map(|(id, path, version, min_codeterm)| {
            serde_json::json!({
                "id": id,
                "version": version,
                "path": path,
                "minCodeterm": min_codeterm,
            })
        })
        .collect::<Vec<_>>();
    std::fs::write(
        dir.join("channel.json"),
        serde_json::json!({
            "schemaVersion": 1,
            "name": SYSTEM_CHANNEL_NAME,
            "plugins": entries,
        })
        .to_string(),
    )
    .unwrap();
    for (id, path, version, min_codeterm) in plugins {
        let plugin_dir = dir.join(path);
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::json!({
                "id": id,
                "version": version,
                "displayName": id,
                "main": "plugin.js",
                "minCodeterm": min_codeterm,
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(plugin_dir.join("plugin.js"), "module.exports = {};").unwrap();
    }
}

// rel#62: an older app is served the newest manifest it can actually run, and one blocked from
// a newer build keeps the providers it had. Assert by plugin id, never by manifest wording.
#[test]
fn old_app_keeps_codex_and_opencode_when_the_channel_outruns_it() {
    with_data_root(|root| {
        let prev_system_url = std::env::var("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL").ok();
        let prev_version = std::env::var("CODETERM_VERSION").ok();

        let source = root.join("system-source");
        std::env::set_var(
            "CODETERM_SYSTEM_PLUGIN_CHANNEL_URL",
            format!("file://{}", source.display()),
        );
        std::env::set_var("CODETERM_VERSION", "1.9.6");

        // Baseline: the app's own bundled build — everything compatible.
        write_system_channel(
            &source,
            &[
                ("claude", "provider-claude", "1.0.0", "1.0.0"),
                ("codex", "provider-codex", "1.0.0", "1.0.0"),
                ("opencode", "provider-opencode", "1.0.0", "1.0.0"),
            ],
        );
        update_system_command(None).expect("baseline sync installs compatible providers");
        for id in ["claude", "codex", "opencode"] {
            assert!(
                plugin_dir(&PluginId::validated(id))
                    .join("plugin.json")
                    .exists(),
                "{id} should be installed at baseline"
            );
        }

        // The channel moves to 1.10.3 and declares codex and opencode need an app at least that new
        // (rel#62: minCodeterm tracks its own version). claude stays compatible.
        write_system_channel(
            &source,
            &[
                ("claude", "provider-claude", "1.10.3", "1.0.0"),
                ("codex", "provider-codex", "1.10.3", "1.10.3"),
                ("opencode", "provider-opencode", "1.10.3", "1.10.3"),
            ],
        );

        // The app is still 1.9.6: it is served the newest manifest it can
        // run (claude updates), and codex/opencode are never touched.
        let result = update_system_command(None)
            .expect("a partially-compatible sync must not error out entirely");
        assert_eq!(result["updated"], serde_json::json!(["claude"]));
        let warnings: Vec<String> = result["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w.as_str().unwrap().to_string())
            .collect();
        assert!(warnings.iter().any(|w| w.contains("codex")), "{warnings:?}");
        assert!(
            warnings.iter().any(|w| w.contains("opencode")),
            "{warnings:?}"
        );

        assert_eq!(
            manifest_version(&plugin_dir(&PluginId::validated("claude"))).unwrap(),
            Some("1.10.3".to_string()),
            "claude should take the compatible update"
        );
        assert_eq!(
            manifest_version(&plugin_dir(&PluginId::validated("codex"))).unwrap(),
            Some("1.0.0".to_string()),
            "codex must keep the version this app can actually run"
        );
        assert_eq!(
            manifest_version(&plugin_dir(&PluginId::validated("opencode"))).unwrap(),
            Some("1.0.0".to_string()),
            "opencode must keep the version this app can actually run"
        );

        // Assert by plugin id, not manifest wording, that the real on-disk codex and opencode entries
        // are still in the served provider set.
        let listed = list_installed().unwrap();
        let rows = listed["plugins"].as_array().unwrap();
        for id in ["codex", "opencode"] {
            let row = rows
                .iter()
                .find(|p| p["id"] == serde_json::json!(id))
                .unwrap_or_else(|| panic!("{id} missing from the served provider set"));
            assert_eq!(
                row["state"],
                serde_json::json!("installed"),
                "{id} should be a real install, not a virtual placeholder"
            );
        }

        restore_env("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL", prev_system_url);
        restore_env("CODETERM_VERSION", prev_version);
    });
}
