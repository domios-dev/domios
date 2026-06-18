use super::*;
mod store_index_tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        crate::test_env_lock()
    }

    fn restore_env(key: &str, value: Option<String>) {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    fn with_data_root<T>(f: impl FnOnce(PathBuf) -> T) -> T {
        let _guard = env_lock().lock().unwrap();
        let prev = std::env::var("CODETERM_DATA_DIR").ok();
        let root =
            std::env::temp_dir().join(format!("codeterm-plugin-test-{}", uuid::Uuid::new_v4()));
        std::env::set_var("CODETERM_DATA_DIR", &root);
        let result = f(root.clone());
        restore_env("CODETERM_DATA_DIR", prev);
        let _ = std::fs::remove_dir_all(root);
        result
    }

    fn write_channel(dir: &Path, name: &str, plugins: &[(&str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        let entries = plugins
            .iter()
            .map(|(id, path)| serde_json::json!({"id": id, "version": "1.0.0", "path": path}))
            .collect::<Vec<_>>();
        std::fs::write(
            dir.join("channel.json"),
            serde_json::json!({"schemaVersion": 1, "name": name, "plugins": entries}).to_string(),
        )
        .unwrap();
        for (id, path) in plugins {
            let plugin_dir = dir.join(path);
            std::fs::create_dir_all(&plugin_dir).unwrap();
            std::fs::write(
                plugin_dir.join("plugin.json"),
                serde_json::json!({
                    "id": id,
                    "version": "1.0.0",
                    "displayName": id,
                    "main": "plugin.js"
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(plugin_dir.join("plugin.js"), "module.exports = {};").unwrap();
        }
    }

    /// The repo's `plugins/` dir IS the `codeterm-core` channel (served at
    /// github.com/rollacode/codeterm.git#master:plugins). `channel.json` is its
    /// store index. This guards the index against drift with the actual plugin
    /// folders — the exact check the live channel-refresh does at runtime.
    #[test]
    fn channel_json_is_a_consistent_store_index() {
        let plugins_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins");
        let text = std::fs::read_to_string(plugins_dir.join("channel.json"))
            .expect("plugins/channel.json must exist (store index)");
        let manifest: ChannelManifest =
            serde_json::from_str(&text).expect("channel.json must parse as ChannelManifest");

        assert_eq!(manifest.name, "codeterm-core");
        assert_eq!(
            manifest.plugins.len(),
            3,
            "expected claude/codex/opencode in the core channel"
        );

        for p in &manifest.plugins {
            let rel = p.path.as_ref().expect("each channel plugin needs a path");
            let dir = plugins_dir.join(rel);
            assert!(
                dir.is_dir(),
                "declared plugin dir missing: {}",
                dir.display()
            );

            // channel.json#id must match the plugin's own plugin.json#id.
            let pj = std::fs::read_to_string(dir.join("plugin.json"))
                .unwrap_or_else(|_| panic!("plugin.json missing in {}", dir.display()));
            let pjv: serde_json::Value = serde_json::from_str(&pj).expect("plugin.json must parse");
            assert_eq!(
                pjv.get("id").and_then(|v| v.as_str()),
                Some(p.id.as_str()),
                "channel.json id '{}' must match {}/plugin.json#id",
                p.id,
                rel
            );
        }
    }

    #[test]
    fn default_system_channel_points_at_develop_plugins_index() {
        assert_eq!(
            DEFAULT_SYSTEM_CHANNEL_URL,
            "https://github.com/rollacode/codeterm.git#develop:plugins"
        );
    }

    #[test]
    fn install_reports_reload_required_for_requires_reload_plugin() {
        with_data_root(|root| {
            let source = root.join("src-needsreload");
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(
                source.join("plugin.json"),
                serde_json::json!({
                    "id": "needsreload",
                    "version": "1.0.0",
                    "displayName": "Needs Reload",
                    "requiresReload": true,
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(source.join("plugin.js"), "module.exports = {};").unwrap();

            let out = install_command(None, Some(source.display().to_string()), None, None)
                .expect("install must succeed");
            assert_eq!(
                out.get("reload_required").and_then(|v| v.as_bool()),
                Some(true)
            );
            assert!(out
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .contains("reload CodeTerm to activate"));
        });
    }

    #[test]
    fn install_omits_reload_hint_for_normal_plugin() {
        with_data_root(|root| {
            let source = root.join("src-normal");
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(
                source.join("plugin.json"),
                serde_json::json!({
                    "id": "normalplugin",
                    "version": "1.0.0",
                    "displayName": "Normal",
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(source.join("plugin.js"), "module.exports = {};").unwrap();

            let out = install_command(None, Some(source.display().to_string()), None, None)
                .expect("install must succeed");
            assert_eq!(
                out.get("reload_required").and_then(|v| v.as_bool()),
                Some(false)
            );
            assert!(!out
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .contains("reload CodeTerm"));
        });
    }

    #[test]
    fn install_message_offers_configuration_with_real_id() {
        with_data_root(|root| {
            let source = root.join("src-configurable");
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(
                source.join("plugin.json"),
                serde_json::json!({
                    "id": "configurable",
                    "version": "1.0.0",
                    "displayName": "Configurable",
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(source.join("plugin.js"), "module.exports = {};").unwrap();

            let out = install_command(None, Some(source.display().to_string()), None, None)
                .expect("install must succeed");
            let msg = out
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            assert!(msg.contains("codeterm plugin get configurable"), "{msg}");
            assert!(
                msg.contains("codeterm plugin config configurable --set key=value"),
                "{msg}"
            );
            assert!(msg.to_lowercase().contains("ask the user"), "{msg}");
            // m5: must not double-prefix "Installed".
            assert_eq!(msg.matches("Installed").count(), 1, "{msg}");
        });
    }

    #[test]
    fn plugin_storage_roots_follow_code_term_data_dir_override() {
        let _guard = env_lock().lock().unwrap();
        let prev = std::env::var("CODETERM_DATA_DIR").ok();
        let root =
            std::env::temp_dir().join(format!("codeterm-plugin-root-{}", uuid::Uuid::new_v4()));
        std::env::set_var("CODETERM_DATA_DIR", &root);

        assert_eq!(plugins_root(), root.join("plugins"));
        assert_eq!(channels_root(), root.join("channels"));
        assert_eq!(channel_registry_path(), root.join("plugin-channels.json"));

        restore_env("CODETERM_DATA_DIR", prev);
    }

    #[test]
    fn refresh_preserves_existing_channel_cache_when_new_manifest_is_malformed() {
        with_data_root(|root| {
            let bad_source = root.join("bad-source");
            std::fs::create_dir_all(&bad_source).unwrap();
            std::fs::write(bad_source.join("channel.json"), "{not-json").unwrap();

            let cache = channels_root().join("demo");
            write_channel(&cache, "demo", &[("demo-plugin", "provider-demo")]);
            write_channel_registry(&ChannelRegistry {
                channels: BTreeMap::from([(
                    "demo".to_string(),
                    ChannelRecord {
                        name: "demo".to_string(),
                        url: format!("file://{}", bad_source.display()),
                        path: cache.display().to_string(),
                        kind: ChannelKind::Local,
                    },
                )]),
            })
            .unwrap();

            let err = refresh_channel(Some("demo")).expect_err("malformed refresh must fail");
            assert!(err.contains("parse channel.json"), "{err}");
            let manifest = read_channel_manifest(&cache).expect("old cache must remain readable");
            assert_eq!(manifest.name, "demo");
            assert_eq!(manifest.plugins[0].id, "demo-plugin");
        });
    }

    #[test]
    fn update_system_installs_manifest_additions_and_warns_without_auto_removing_deletions() {
        with_data_root(|root| {
            let prev_system_url = std::env::var("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL").ok();
            let source = root.join("system-source");
            write_channel(
                &source,
                SYSTEM_CHANNEL_NAME,
                &[("extra-system", "provider-extra")],
            );
            std::env::set_var(
                "CODETERM_SYSTEM_PLUGIN_CHANNEL_URL",
                format!("file://{}", source.display()),
            );

            let first = update_system_command(None).expect("system manifest addition installs");
            assert_eq!(first["updated"], serde_json::json!(["extra-system"]));
            assert!(plugin_dir("extra-system").join("plugin.json").exists());
            let listed = list_installed().unwrap();
            let extra = listed["plugins"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == serde_json::json!("extra-system"))
                .expect("installed system plugin should be listed");
            assert_eq!(extra["verified"], serde_json::json!(true));

            write_channel(&source, SYSTEM_CHANNEL_NAME, &[]);
            let second = update_system_command(None).expect("removal warning does not uninstall");
            assert_eq!(second["updated"], serde_json::json!([]));
            assert!(second["warnings"][0]
                .as_str()
                .unwrap()
                .contains("left installed"));
            assert!(plugin_dir("extra-system").join("plugin.json").exists());

            restore_env("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL", prev_system_url);
        });
    }

    #[test]
    fn installed_ids_ignore_leftover_tmp_install_dirs() {
        with_data_root(|_root| {
            write_channel(&plugin_dir("real"), "unused", &[]);
            std::fs::create_dir_all(plugins_root().join("partial.tmp-install")).unwrap();
            let ids = installed_ids().unwrap();
            assert_eq!(ids, vec!["real".to_string()]);
            let listed = list_installed().unwrap();
            let rows = listed["plugins"].as_array().unwrap();
            // One real on-disk plugin + the always-present system intrinsics.
            let non_system: Vec<_> = rows
                .iter()
                .filter(|p| p["system"] != serde_json::json!(true))
                .collect();
            assert_eq!(non_system.len(), 1);
            assert_eq!(non_system[0]["id"], serde_json::json!("real"));
            assert!(rows.iter().any(|p| p["id"] == serde_json::json!("claude")
                && p["removable"] == serde_json::json!(false)));
        });
    }

    #[test]
    fn list_installed_injects_locked_system_rows_when_dir_empty() {
        with_data_root(|_root| {
            let listed = list_installed().unwrap();
            let rows = listed["plugins"].as_array().unwrap();
            for id in SYSTEM_PLUGIN_IDS {
                let row = rows
                    .iter()
                    .find(|p| p["id"] == serde_json::json!(id))
                    .unwrap_or_else(|| panic!("system plugin {id} must be listed"));
                assert_eq!(row["system"], serde_json::json!(true));
                assert_eq!(row["removable"], serde_json::json!(false));
                assert_eq!(row["state"], serde_json::json!("bundled"));
            }
        });
    }

    #[test]
    fn list_channels_injects_locked_system_channel_when_registry_empty() {
        with_data_root(|_root| {
            let listed = list_channels().unwrap();
            let core = listed["channels"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == serde_json::json!(SYSTEM_CHANNEL_NAME))
                .expect("system channel must be listed")
                .clone();
            assert_eq!(core["system"], serde_json::json!(true));
            assert_eq!(core["removable"], serde_json::json!(false));
        });
    }

    #[test]
    fn compat_check_blocks_only_when_app_older_than_min() {
        let _guard = env_lock().lock().unwrap();
        let prev = std::env::var("CODETERM_VERSION").ok();

        std::env::set_var("CODETERM_VERSION", "1.3.2");
        assert!(compat_check(Some("1.0.0")).is_ok());
        assert!(compat_check(Some("1.3.2")).is_ok());
        let err = compat_check(Some("2.0.0")).unwrap_err();
        assert!(err.contains("2.0.0") && err.contains("1.3.2"), "got: {err}");

        // Fail-open: unknown running version never blocks.
        std::env::remove_var("CODETERM_VERSION");
        assert!(compat_check(Some("99.0.0")).is_ok());
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
                serde_json::json!({"id":"needy","version":"1.0.0","minCodeterm":"2.0.0"})
                    .to_string(),
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
                !plugin_dir("needy").exists(),
                "incompatible plugin must not be installed"
            );

            restore_env("CODETERM_VERSION", prev_v);
        });
    }
}
