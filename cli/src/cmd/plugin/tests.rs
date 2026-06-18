use super::*;
mod store_index_tests {
    use super::*;
    use std::collections::BTreeMap;
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

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(not(unix))]
    fn make_executable(_path: &Path) {}

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
    fn remove_with_remove_data_deletes_convention_dir_and_manifest_data_paths() {
        with_data_root(|root| {
            let plugin = plugins_root().join("transcriber");
            std::fs::create_dir_all(&plugin).unwrap();
            let custom_data = root.join("models").join("transcriber");
            std::fs::create_dir_all(&custom_data).unwrap();
            std::fs::write(custom_data.join("model.bin"), "model").unwrap();
            let convention_data = root.join("transcriber");
            std::fs::create_dir_all(&convention_data).unwrap();
            std::fs::write(convention_data.join("cache.bin"), "cache").unwrap();
            std::fs::write(
                plugin.join("plugin.json"),
                serde_json::json!({
                    "id": "transcriber",
                    "version": "1.0.0",
                    "dataPaths": [
                        "~/models/transcriber"
                    ]
                })
                .to_string(),
            )
            .unwrap();

            remove_installed("transcriber", false, true).expect("remove must succeed");

            assert!(!plugin.exists());
            assert!(!convention_data.exists());
            assert!(!custom_data.exists());
        });
    }

    #[test]
    fn remove_without_remove_data_keeps_plugin_data() {
        with_data_root(|root| {
            let plugin = plugins_root().join("demo");
            std::fs::create_dir_all(&plugin).unwrap();
            let data = root.join("demo");
            std::fs::create_dir_all(&data).unwrap();
            std::fs::write(
                plugin.join("plugin.json"),
                serde_json::json!({"id": "demo", "version": "1.0.0"}).to_string(),
            )
            .unwrap();

            remove_installed("demo", false, false).expect("remove must succeed");

            assert!(!plugin.exists());
            assert!(data.exists());
        });
    }

    #[test]
    fn remove_data_rejects_reserved_data_ids() {
        with_data_root(|_root| {
            let plugin = plugins_root().join("logs");
            std::fs::create_dir_all(&plugin).unwrap();
            std::fs::write(
                plugin.join("plugin.json"),
                serde_json::json!({"id": "logs", "version": "1.0.0"}).to_string(),
            )
            .unwrap();

            let err = remove_installed("logs", false, true).expect_err("reserved id rejected");

            assert!(err.contains("reserved plugin data id"), "{err}");
            assert!(plugin.exists());
        });
    }

    #[test]
    fn remove_data_rejects_manifest_paths_outside_home_or_with_parent_segments() {
        with_data_root(|root| {
            let outside = std::env::temp_dir()
                .join(format!("codeterm-outside-data-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&outside).unwrap();
            let plugin = plugins_root().join("demo");
            std::fs::create_dir_all(&plugin).unwrap();
            std::fs::write(
                plugin.join("plugin.json"),
                serde_json::json!({
                    "id": "demo",
                    "version": "1.0.0",
                    "dataPaths": [outside.display().to_string()]
                })
                .to_string(),
            )
            .unwrap();

            let err = remove_installed("demo", false, true).expect_err("outside path rejected");
            assert!(err.contains("outside home"), "{err}");
            assert!(plugin.exists());

            std::fs::write(
                plugin.join("plugin.json"),
                serde_json::json!({
                    "id": "demo",
                    "version": "1.0.0",
                    "dataPaths": [root.join("demo/../other").display().to_string()]
                })
                .to_string(),
            )
            .unwrap();

            let err = remove_installed("demo", false, true).expect_err("parent path rejected");
            assert!(err.contains("parent segment"), "{err}");
            assert!(plugin.exists());
            let _ = std::fs::remove_dir_all(outside);
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
    fn install_from_git_channel_refreshes_before_resolving() {
        with_data_root(|root| {
            let prev_path = std::env::var("PATH").ok();
            let prev_fresh = std::env::var("CODETERM_TEST_FRESH_CHANNEL").ok();
            let prev_log = std::env::var("CODETERM_TEST_GIT_LOG").ok();

            let cache = channels_root().join("demo");
            write_channel(&cache, "demo", &[("fresh", "provider-fresh")]);

            let fresh_channel = root.join("fresh-channel");
            write_channel(&fresh_channel, "demo", &[("fresh", "provider-fresh")]);
            std::fs::write(
                fresh_channel.join("channel.json"),
                serde_json::json!({
                    "schemaVersion": 1,
                    "name": "demo",
                    "plugins": [{"id": "fresh", "version": "2.0.0", "path": "provider-fresh"}]
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(
                fresh_channel.join("provider-fresh/plugin.json"),
                serde_json::json!({
                    "id": "fresh",
                    "version": "2.0.0",
                    "displayName": "Fresh",
                    "main": "plugin.js"
                })
                .to_string(),
            )
            .unwrap();

            write_channel_registry(&ChannelRegistry {
                channels: BTreeMap::from([(
                    "demo".to_string(),
                    ChannelRecord {
                        name: "demo".to_string(),
                        url: "https://example.invalid/demo.git#main".to_string(),
                        path: cache.display().to_string(),
                        kind: ChannelKind::Git,
                    },
                )]),
            })
            .unwrap();

            let bin = root.join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let git = bin.join("git");
            std::fs::write(
                &git,
                r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$CODETERM_TEST_GIT_LOG"
if [ "$1" = "clone" ]; then
  dest="$5"
  mkdir -p "$dest"
  cp -R "$CODETERM_TEST_FRESH_CHANNEL/." "$dest/"
fi
exit 0
"#,
            )
            .unwrap();
            make_executable(&git);

            let log = root.join("git.log");
            std::env::set_var("CODETERM_TEST_FRESH_CHANNEL", &fresh_channel);
            std::env::set_var("CODETERM_TEST_GIT_LOG", &log);
            let mut path_entries = vec![bin.clone()];
            if let Some(path) = &prev_path {
                path_entries.extend(std::env::split_paths(path));
            }
            std::env::set_var("PATH", std::env::join_paths(path_entries).unwrap());

            let out = install_command(Some("fresh".to_string()), None, None, None)
                .expect("install should refresh channel and install");

            assert_eq!(
                manifest_version(&plugin_dir("fresh")).unwrap(),
                Some("2.0.0".to_string())
            );
            assert_eq!(out["warnings"], serde_json::json!([]));
            let git_log = std::fs::read_to_string(log).expect("git stub should be invoked");
            assert!(git_log.contains("clone --depth 1 https://example.invalid/demo.git"));
            assert!(git_log.contains("fetch --depth 1 origin main"));

            restore_env("PATH", prev_path);
            restore_env("CODETERM_TEST_FRESH_CHANNEL", prev_fresh);
            restore_env("CODETERM_TEST_GIT_LOG", prev_log);
        });
    }

    #[test]
    fn install_from_channel_warns_and_uses_cache_when_refresh_fails() {
        with_data_root(|root| {
            let bad_source = root.join("bad-source");
            std::fs::create_dir_all(&bad_source).unwrap();
            std::fs::write(bad_source.join("channel.json"), "{not-json").unwrap();

            let cache = channels_root().join("demo");
            write_channel(&cache, "demo", &[("cached", "provider-cached")]);
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

            let out = install_command(
                Some("cached".to_string()),
                None,
                None,
                Some("demo".to_string()),
            )
            .expect("refresh failure should not block cached install");

            assert!(plugin_dir("cached").join("plugin.json").exists());
            let warnings = out["warnings"].as_array().expect("warnings array");
            assert_eq!(warnings.len(), 1);
            let warning = warnings[0].as_str().unwrap();
            assert!(
                warning.contains("refresh channel 'demo' failed"),
                "{warning}"
            );
            assert!(warning.contains("cached channel"), "{warning}");
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
