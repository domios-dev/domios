use super::*;

mod error_paths;
mod id_validation;

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
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
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

    /// The repo's `plugins/` dir IS the `codeterm-core` channel; `channel.json` is its store index.
    /// Guards that index against drift with the real plugin folders, as the live refresh does.
    #[test]
    fn channel_json_is_a_consistent_store_index() {
        let plugins_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let text = std::fs::read_to_string(plugins_dir.join("channel.json"))
            .expect("plugins/channel.json must exist (store index)");
        let manifest: ChannelManifest =
            serde_json::from_str(&text).expect("channel.json must parse as ChannelManifest");

        assert_eq!(manifest.name, "codeterm-core");
        assert_eq!(
            manifest.plugins.len(),
            6,
            "expected claude/codex/opencode/cursor/chat-preprocessor/analytics in the core channel"
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

    // rel#62: `minCodeterm` frozen while `version` bumps is a no-op gate — it can never block an
    // app older than the build it ships with, so every app is served the newest plugin.
    #[test]
    fn system_plugin_min_codeterm_tracks_its_own_version() {
        let plugins_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let text = std::fs::read_to_string(plugins_dir.join("channel.json"))
            .expect("plugins/channel.json must exist (store index)");
        let manifest: ChannelManifest =
            serde_json::from_str(&text).expect("channel.json must parse as ChannelManifest");

        for id in SYSTEM_PLUGIN_IDS {
            let plugin = manifest
                .plugins
                .iter()
                .find(|p| p.id == id)
                .unwrap_or_else(|| panic!("{id} missing from the codeterm-core channel"));
            assert_eq!(
                plugin.min_codeterm.as_deref(),
                plugin.version.as_deref(),
                "{id}'s minCodeterm must track its own version — otherwise an app \
                 older than this build is served a plugin it cannot run"
            );

            let rel = plugin
                .path
                .as_ref()
                .expect("each channel plugin needs a path");
            let pj = std::fs::read_to_string(plugins_dir.join(rel).join("plugin.json"))
                .unwrap_or_else(|_| panic!("plugin.json missing for {id}"));
            let pjv: serde_json::Value = serde_json::from_str(&pj).expect("plugin.json parses");
            assert_eq!(
                pjv.get("minCodeterm").and_then(|v| v.as_str()),
                pjv.get("version").and_then(|v| v.as_str()),
                "{id}/plugin.json's own minCodeterm must also track its version"
            );
        }
    }

    #[test]
    fn default_system_channel_points_at_a_released_plugins_index() {
        assert_eq!(
            DEFAULT_SYSTEM_CHANNEL_URL,
            "https://github.com/rollacode/codeterm.git#master:plugins"
        );
        let git_ref = DEFAULT_SYSTEM_CHANNEL_URL
            .rsplit_once('#')
            .expect("system channel url names a ref")
            .1
            .split_once(':')
            .expect("system channel url names a subpath in that ref")
            .0;
        assert_ne!(
            git_ref, "develop",
            "the default system channel installs as System trust; it must not track unreleased code"
        );
    }

    #[test]
    fn id_override_renames_runtime_consistently() {
        with_data_root(|root| {
            let source = root.join("override-source");
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(
                source.join("plugin.json"),
                serde_json::json!({"id":"source-id","version":"1.0.0","requiresReload":true})
                    .to_string(),
            )
            .unwrap();
            std::fs::write(source.join("plugin.js"), "module.exports = {};").unwrap();
            let result = install_command(
                None,
                Some(source.display().to_string()),
                Some("runtime-id".into()),
                None,
            )
            .unwrap();
            assert_eq!(result["id"], "runtime-id");
            let folder = plugin_dir(&PluginId::parse("runtime-id").unwrap());
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(folder.join("plugin.json")).unwrap())
                    .unwrap();
            assert_eq!(manifest["id"], "runtime-id");
            let rows = list_installed().unwrap();
            assert!(rows["plugins"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == "runtime-id"));
            assert_eq!(
                config::get_command("runtime-id").unwrap()["id"],
                "runtime-id"
            );
            assert_eq!(result["reload_required"], true);
        });
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
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
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

    // One unreadable channel must not blank the whole marketplace: the aggregate diff keeps the
    // healthy channel and skips the broken one.
    #[test]
    fn aggregate_diff_skips_failed_channel_but_keeps_healthy_local() {
        with_data_root(|_root| {
            let healthy = channels_root().join("healthy");
            write_channel(&healthy, "healthy", &[("good-plugin", "provider-good")]);
            let missing = channels_root().join("broken-cache-does-not-exist");
            write_channel_registry(&ChannelRegistry {
                channels: BTreeMap::from([
                    (
                        "healthy".to_string(),
                        ChannelRecord {
                            name: "healthy".to_string(),
                            url: format!("file://{}", healthy.display()),
                            path: healthy.display().to_string(),
                            kind: ChannelKind::Local,
                        },
                    ),
                    (
                        "broken".to_string(),
                        ChannelRecord {
                            name: "broken".to_string(),
                            url: format!("file://{}/nope", channels_root().display()),
                            path: missing.display().to_string(),
                            kind: ChannelKind::Local,
                        },
                    ),
                ]),
            })
            .unwrap();

            let diff =
                channel_diff(None).expect("aggregate diff must not abort on one bad channel");
            let ids: Vec<String> = diff["diff"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["id"].as_str().unwrap().to_string())
                .collect();
            assert!(
                ids.iter().any(|id| id == "good-plugin"),
                "healthy channel was dropped: {ids:?}"
            );
            assert!(
                diff["diff"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|r| r["channel"].as_str() != Some("broken")),
                "broken channel should be skipped, not surfaced"
            );
        });
    }

    // A named single-channel diff stays strict: the caller asked for that channel specifically,
    // so an unreadable one propagates the error instead of returning an empty diff.
    #[test]
    fn named_single_channel_diff_stays_strict() {
        with_data_root(|_root| {
            let missing = channels_root().join("broken-cache-does-not-exist");
            write_channel_registry(&ChannelRegistry {
                channels: BTreeMap::from([(
                    "broken".to_string(),
                    ChannelRecord {
                        name: "broken".to_string(),
                        url: format!("file://{}/nope", channels_root().display()),
                        path: missing.display().to_string(),
                        kind: ChannelKind::Local,
                    },
                )]),
            })
            .unwrap();

            channel_diff(Some("broken"))
                .expect_err("named diff of an unresolvable channel must error, not return empty");
        });
    }

    // A local channel's diff reads the stable cached copy, not `source_dir()`, so it survives a
    // transiently absent file:// source.
    #[test]
    fn local_channel_diff_reads_cache_when_source_is_gone() {
        with_data_root(|root| {
            let source = root.join("volatile-source");
            write_channel(
                &source.join("plugins"),
                "cached",
                &[("cached-plugin", "provider-cached")],
            );
            let cache = channels_root().join("cached");
            write_channel(&cache, "cached", &[("cached-plugin", "provider-cached")]);
            write_channel_registry(&ChannelRegistry {
                channels: BTreeMap::from([(
                    "cached".to_string(),
                    ChannelRecord {
                        name: "cached".to_string(),
                        url: format!("file://{}/plugins", source.display()),
                        path: cache.display().to_string(),
                        kind: ChannelKind::Local,
                    },
                )]),
            })
            .unwrap();

            // The volatile source disappears; the stable cache must carry the diff.
            std::fs::remove_dir_all(&source).unwrap();
            let diff = channel_diff(Some("cached"))
                .expect("diff must read the cache when the source is gone");
            let ids: Vec<String> = diff["diff"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["id"].as_str().unwrap().to_string())
                .collect();
            assert!(
                ids.iter().any(|id| id == "cached-plugin"),
                "diff should surface the cached plugin: {ids:?}"
            );
        });
    }

    // The refresh swap must keep the cache continuously readable; the old remove_dir_all + rename
    // left `ch.path` absent mid-publish.
    #[test]
    fn refresh_swap_keeps_local_cache_readable_and_updates_it() {
        with_data_root(|_root| {
            let source = channels_root().join("src-live");
            write_channel(
                &source,
                "live",
                &[("v2-plugin", "provider-v2"), ("extra", "provider-extra")],
            );
            let cache = channels_root().join("live");
            write_channel(&cache, "live", &[("v1-plugin", "provider-v1")]);
            write_channel_registry(&ChannelRegistry {
                channels: BTreeMap::from([(
                    "live".to_string(),
                    ChannelRecord {
                        name: "live".to_string(),
                        url: format!("file://{}", source.display()),
                        path: cache.display().to_string(),
                        kind: ChannelKind::Local,
                    },
                )]),
            })
            .unwrap();

            refresh_channel(Some("live")).expect("refresh from a valid source must succeed");
            // Cache is present and now reflects the source content (atomic swap published it).
            let manifest =
                read_channel_manifest(&cache).expect("cache must stay readable post-swap");
            let ids: Vec<String> = manifest.plugins.iter().map(|p| p.id.clone()).collect();
            assert!(
                ids.iter().any(|id| id == "v2-plugin"),
                "swap did not update cache: {ids:?}"
            );
            let leftover_backups = std::fs::read_dir(channels_root())
                .unwrap()
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with(".live-old-"));
            assert!(
                !leftover_backups,
                "refresh swap left a stale backup dir behind"
            );
        });
    }

    // `clone_git_channel` needs a unique scratch dir per invocation: a fixed `<cache>.git-clone`
    // let concurrent clones of the same channel corrupt each other's git metadata.
    #[test]
    fn concurrent_git_channel_clones_do_not_corrupt_shared_temp() {
        with_data_root(|root| {
            let prev_git_bin = std::env::var("CODETERM_TEST_GIT_BIN").ok();
            std::env::remove_var("CODETERM_TEST_GIT_BIN"); // use the real git binary

            let git = |args: &[&str], cwd: Option<&Path>| {
                let mut c = std::process::Command::new("git");
                if let Some(d) = cwd {
                    c.current_dir(d);
                }
                let out = c.args(args).output().expect("git runs");
                assert!(
                    out.status.success(),
                    "git {args:?}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            };
            let origin = root.join("origin.git");
            let work = root.join("work");
            git(&["init", "--bare", origin.to_str().unwrap()], None);
            git(
                &["clone", origin.to_str().unwrap(), work.to_str().unwrap()],
                None,
            );
            write_channel(&work, "conc", &[("conc-plugin", "provider-conc")]);
            git(&["add", "-A"], Some(&work));
            git(
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "commit",
                    "-m",
                    "seed",
                ],
                Some(&work),
            );
            let cache = channels_root().join("conc");
            // No #ref fragment: clone work's default branch directly. The concurrency under
            // test is the clone-into-scratch-dir step (where the fixed temp used to corrupt).
            let url = format!("file://{}", work.display());
            let errors: Vec<String> = std::thread::scope(|s| {
                let handles: Vec<_> = (0..8)
                    .map(|_| {
                        let url = url.clone();
                        let cache = cache.clone();
                        s.spawn(move || {
                            clone_git_channel(&parse_channel_source(&url), &cache).err()
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .filter_map(|h| h.join().unwrap())
                    .collect()
            });

            for e in &errors {
                // Concurrent-clone corruption shows up as these git errors. Same-target publish races are
                // acceptable; production dedups those via ensure_default's lock.
                assert!(
                    !e.contains("could not lock")
                        && !e.contains("refs/files-backend")
                        && !e.contains("shallow file"),
                    "concurrent clone corrupted a shared temp: {e}"
                );
            }
            let manifest = read_channel_manifest(&cache)
                .expect("cache must be readable after concurrent clones");
            assert_eq!(manifest.plugins[0].id, "conc-plugin");

            restore_env("CODETERM_TEST_GIT_BIN", prev_git_bin);
        });
    }

    #[test]
    fn install_from_git_channel_refreshes_before_resolving() {
        with_data_root(|root| {
            let prev_path = std::env::var("PATH").ok();
            let prev_git_bin = std::env::var("CODETERM_TEST_GIT_BIN").ok();
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
  for arg do dest="$arg"; done
  mkdir -p "$dest"
  cp -R "$CODETERM_TEST_FRESH_CHANNEL/." "$dest/"
fi
exit 0
"#,
            )
            .unwrap();
            make_executable(&git);
            std::fs::write(
                bin.join("git.cmd"),
                r#"@echo off
echo %*>>"%CODETERM_TEST_GIT_LOG%"
if not "%~1"=="clone" exit /b 0
for %%a in (%*) do set "dest=%%~a"
mkdir "%dest%" 2>nul
"%SystemRoot%\System32\robocopy.exe" "%CODETERM_TEST_FRESH_CHANNEL%" "%dest%" /E >nul
if %errorlevel% geq 8 exit /b 1
exit /b 0
"#,
            )
            .unwrap();

            let log = root.join("git.log");
            std::env::set_var("CODETERM_TEST_FRESH_CHANNEL", &fresh_channel);
            std::env::set_var("CODETERM_TEST_GIT_LOG", &log);
            std::env::set_var(
                "CODETERM_TEST_GIT_BIN",
                if cfg!(windows) {
                    bin.join("git.cmd")
                } else {
                    git.clone()
                },
            );
            let mut path_entries = vec![bin.clone()];
            if let Some(path) = &prev_path {
                path_entries.extend(std::env::split_paths(path));
            }
            std::env::set_var("PATH", std::env::join_paths(path_entries).unwrap());

            let out = install_command(Some("fresh".to_string()), None, None, None)
                .expect("install should refresh channel and install");

            assert_eq!(
                manifest_version(&plugin_dir(&PluginId::validated("fresh"))).unwrap(),
                Some("2.0.0".to_string())
            );
            assert_eq!(out["warnings"], serde_json::json!([]));
            let git_log = std::fs::read_to_string(log).expect("git stub should be invoked");
            assert!(git_log.contains("clone"));
            assert!(git_log.contains("core.longpaths=true"));
            assert!(git_log.contains("--depth 1"));
            assert!(git_log.contains("--filter=blob:none"));
            assert!(git_log.contains("https://example.invalid/demo.git"));
            assert!(git_log.contains("fetch --depth 1 origin main"));

            restore_env("PATH", prev_path);
            restore_env("CODETERM_TEST_GIT_BIN", prev_git_bin);
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

            assert!(plugin_dir(&PluginId::validated("cached"))
                .join("plugin.json")
                .exists());
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
            assert!(plugin_dir(&PluginId::validated("extra-system"))
                .join("plugin.json")
                .exists());
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
            assert!(plugin_dir(&PluginId::validated("extra-system"))
                .join("plugin.json")
                .exists());

            restore_env("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL", prev_system_url);
        });
    }

    #[test]
    fn installed_ids_ignore_leftover_tmp_install_dirs() {
        with_data_root(|_root| {
            write_channel(&plugin_dir(&PluginId::validated("real")), "unused", &[]);
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
}
