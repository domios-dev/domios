use super::*;

#[test]
fn changed_provider_settings_reports_only_changed_plugin_and_agent_entries() {
    let mut before = AppConfig::default();
    before
        .plugins
        .insert("plugin-a".into(), serde_json::json!({ "model": "one" }));
    before.plugins.insert(
        "plugin-stable".into(),
        serde_json::json!({ "enabled": true }),
    );
    before
        .agents
        .insert("agent-removed".into(), schema::ProviderConfig::default());
    let mut after = before.clone();
    after
        .plugins
        .insert("plugin-a".into(), serde_json::json!({ "model": "two" }));
    after
        .plugins
        .insert("plugin-new".into(), serde_json::json!({ "model": "one" }));
    after.agents.remove("agent-removed");

    assert_eq!(
        changed_provider_settings(&before, &after),
        vec![
            "agent-removed".to_string(),
            "plugin-a".to_string(),
            "plugin-new".to_string(),
        ]
    );
}

#[test]
fn unchanged_provider_settings_do_not_invalidate_catalogues() {
    let config = AppConfig::default();

    assert!(changed_provider_settings(&config, &config).is_empty());
}

#[test]
fn provider_resume_setting_uses_override_then_global_and_defaults_to_enabled() {
    let mut config = schema::AppConfig::default();
    config.agent.resume_sessions = true;
    config.agents.insert(
        "codex".into(),
        schema::ProviderConfig {
            resume_sessions: Some(false),
            ..Default::default()
        },
    );

    assert_eq!(
        provider_config_from(&config, "codex").resume_sessions,
        Some(false)
    );
    assert_eq!(
        provider_config_from(&config, "claude").resume_sessions,
        Some(true)
    );

    config.agent.resume_sessions = false;
    assert_eq!(
        provider_config_from(&config, "claude").resume_sessions,
        Some(false)
    );
    assert_eq!(
        provider_config_from(&schema::AppConfig::default(), "codex").resume_sessions,
        Some(true)
    );
}

#[test]
fn concurrent_load_on_tokenless_config_yields_one_stable_token() {
    // The racy version generated a different token per caller, so a tab spawned with a sibling-generated token 401'd against the auth-held token.
    const N: usize = 16;
    let tmp = tempfile::tempdir().unwrap();
    let path = std::sync::Arc::new(tmp.path().join("config.toml"));
    std::fs::write(&*path, "[api]\nenabled = true\n").unwrap();

    let cache: std::sync::Arc<ConfigCache> = std::sync::Arc::new(Mutex::new(None));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(N));
    let handles: Vec<_> = (0..N)
        .map(|_| {
            let c = cache.clone();
            let p = path.clone();
            let b = barrier.clone();
            std::thread::spawn(move || {
                b.wait();
                load_config_cached(&p, &c).api.token
            })
        })
        .collect();
    let tokens: Vec<Option<String>> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    let first = tokens[0].clone();
    assert!(
        first.as_ref().map_or(false, |t| !t.is_empty()),
        "a token must be auto-generated for a tokenless config"
    );
    for t in &tokens {
        assert_eq!(
            t, &first,
            "every concurrent caller must observe the SAME auto-generated token"
        );
    }
    let on_disk: AppConfig = toml::from_str(&std::fs::read_to_string(&*path).unwrap()).unwrap();
    assert!(on_disk.api.token.is_none());
    assert_eq!(
        codeterm_util::instance_token::read_at(tmp.path()).unwrap(),
        first
    );
}

#[test]
fn api_callback_url_normalizes_wildcard_host_to_loopback() {
    let mut api = schema::ApiConfig::default();
    api.host = "0.0.0.0".to_string();
    api.port = 7686;
    let (url, port) = api_callback_url(&api);
    assert_eq!(url, "http://127.0.0.1:7686");
    assert_eq!(port, "7686");

    api.host = "192.168.1.5".to_string();
    api.port = 7691;
    let (url, port) = api_callback_url(&api);
    assert_eq!(url, "http://192.168.1.5:7691");
    assert_eq!(port, "7691");
}

/// The chime answers survive the app's own read path. Reconciliation runs on
/// every load and rebuilds the row list, so an answer dropped there would be
/// re-defaulted on restart and the user's choice would silently expire.
#[test]
fn notification_level_sound_answers_survive_the_config_read() {
    use codeterm_notify_types::NotifyLevel;

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
[notifications]
sound = true

[[notifications.agent_levels]]
level = "verbose"
sound = true

[[notifications.agent_levels]]
level = "action_needed"
sound = false
"#,
    )
    .unwrap();

    let cache: ConfigCache = Mutex::new(None);
    let config = load_config_cached(&path, &cache);
    let row = |level: NotifyLevel| {
        config
            .notifications
            .agent_levels
            .iter()
            .find(|candidate| candidate.level == level.as_str())
            .unwrap_or_else(|| panic!("{} has a row", level.as_str()))
    };

    assert!(config.notifications.sound, "the master mute is read back");
    assert!(
        row(NotifyLevel::Verbose).sounds(),
        "progress chimes because the file says so, against the shipped rule"
    );
    assert!(
        !row(NotifyLevel::ActionNeeded).sounds(),
        "and the level the shipped rule chimed for stays silent because the file says so"
    );
    assert!(
        row(NotifyLevel::Failure).sounds(),
        "a level the file never mentions is reconciled in with the shipped chime"
    );
    assert_eq!(
        row(NotifyLevel::Failure).sound,
        None,
        "and stays unanswered rather than being frozen to the fallback"
    );
    for stored in &config.notifications.agent_levels {
        assert!(
            !stored.channels.contains_key("sound"),
            "no sound key reached the flattened channel map: {:?}",
            stored.channels
        );
    }
}

#[test]
fn test_default_config_parses() {
    let toml_str = default_config_toml();
    let config: AppConfig = toml::from_str(&toml_str).expect("default TOML should parse");
    assert_eq!(config.font.size, 14);
    assert_eq!(config.terminal.scrollback, 10000);
    assert!(config.api.enabled);
    assert_eq!(config.api.host, "127.0.0.1");
    assert!(!config.api.cors);
    assert_eq!(config.api.port, 7685);
}

#[test]
fn api_loopback_spellings_have_bindable_addresses_and_valid_callback_urls() {
    for (host, bind, callback) in [
        ("localhost", "127.0.0.1:7686", "http://127.0.0.1:7686"),
        ("::1", "[::1]:7686", "http://[::1]:7686"),
        ("[::1]", "[::1]:7686", "http://[::1]:7686"),
    ] {
        let api = schema::ApiConfig {
            host: host.into(),
            port: 7686,
            ..schema::ApiConfig::default()
        };
        assert_eq!(canonical_api_bind_addr(&api).unwrap().to_string(), bind);
        assert_eq!(api_callback_url(&api).0, callback);
        assert!(validate_api_trust_boundary(&api).is_ok());
    }

    let unsupported = schema::ApiConfig {
        host: "domios.internal".into(),
        ..schema::ApiConfig::default()
    };
    assert!(validate_api_trust_boundary(&unsupported).is_err());
}

#[test]
fn api_defaults_are_loopback_authenticated_and_cors_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let config = load_config_cached(&tmp.path().join("config.toml"), &Mutex::new(None));
    assert_eq!(config.api.host, "127.0.0.1");
    assert!(!config.api.cors);
    assert!(config.api.cors_allow_origins.is_empty());
    assert!(config
        .api
        .token
        .as_ref()
        .is_some_and(|token| !token.is_empty()));
    assert!(validate_api_trust_boundary(&config.api).is_ok());

    let mut unsafe_remote = schema::ApiConfig::default();
    unsafe_remote.host = "0.0.0.0".into();
    unsafe_remote.token = None;
    assert!(validate_api_trust_boundary(&unsafe_remote).is_err());

    unsafe_remote.token = Some("explicit-network-token".into());
    assert!(validate_api_trust_boundary(&unsafe_remote).is_ok());

    let mut permissive_cors = schema::ApiConfig {
        cors: true,
        ..schema::ApiConfig::default()
    };
    assert!(validate_api_trust_boundary(&permissive_cors).is_ok());

    permissive_cors.cors_allow_origins = vec!["*".into()];
    assert!(validate_api_trust_boundary(&permissive_cors).is_err());
    for invalid in [
        "https://*.example",
        "https://trusted.example/path",
        "https://user:secret@trusted.example",
        "https://trusted.example?token=secret",
    ] {
        permissive_cors.cors_allow_origins = vec![invalid.into()];
        assert!(validate_api_trust_boundary(&permissive_cors).is_err());
    }
}

#[test]
fn test_config_roundtrip() {
    let config = AppConfig::default();
    let serialized = toml::to_string_pretty(&config).expect("serialize");
    let deserialized: AppConfig = toml::from_str(&serialized).expect("deserialize");
    assert_eq!(config.theme.name, deserialized.theme.name);
    assert_eq!(config.font.family, deserialized.font.family);
    assert_eq!(config.keybindings.new_tab, deserialized.keybindings.new_tab);
    assert_eq!(config.api.port, deserialized.api.port);
}

#[test]
fn test_config_partial_toml_uses_defaults() {
    let partial = r#"
[theme]
name = "dracula"

[font]
size = 16
"#;
    let config: AppConfig = toml::from_str(partial).expect("partial TOML should parse");
    assert_eq!(config.theme.name, "dracula");
    assert_eq!(config.font.size, 16);
    assert_eq!(config.terminal.scrollback, 10000);
    assert!(config.api.enabled);
    assert_eq!(config.keybindings.new_tab, "Ctrl+T");
}

#[test]
fn missing_mesh_instance_name_normalizes_to_hostname() {
    let raw = r#"
[mesh]
enabled = true
"#;
    let mut config: AppConfig = toml::from_str(raw).expect("config should parse");
    assert!(config.mesh.instance_name.is_empty());

    normalize_runtime_defaults(&mut config);

    assert!(config
        .mesh
        .instance_name
        .starts_with(&default_mesh_instance_name()));
    if codeterm_paths::is_dev_instance() {
        assert!(config.mesh.instance_name.ends_with(']'));
    } else {
        assert_eq!(config.mesh.instance_name, default_mesh_instance_name());
    }
    assert!(!config.mesh.instance_name.is_empty());
}

#[test]
fn test_resolve_theme_applies_builtin() {
    let mut config = AppConfig::default();
    config.theme.name = "dracula".to_string();
    resolve_theme(&mut config);
    assert_ne!(config.theme.background, "#0d1117");
    assert_eq!(config.theme.name, "dracula");
}

#[test]
fn test_save_and_load_config() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");

    let mut config = AppConfig::default();
    config.font.size = 18;
    config.api.port = 9999;

    let toml_str = toml::to_string_pretty(&config).unwrap();
    std::fs::write(&path, &toml_str).unwrap();

    let loaded: AppConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(loaded.font.size, 18);
    assert_eq!(loaded.api.port, 9999);
}

#[test]
fn cleanup_removed_plugin_config_drops_plugin_and_resets_active_transcriber() {
    let mut config = AppConfig::default();
    config.plugins.insert(
        "transcriber".to_string(),
        serde_json::json!({"model": "large-v3"}),
    );
    config
        .plugins
        .insert("other".to_string(), serde_json::json!({"ok": true}));
    config.integrations.voice_transcribe.active = "transcriber".to_string();

    cleanup_removed_plugin_config_in_memory(&mut config, "transcriber");

    assert!(!config.plugins.contains_key("transcriber"));
    assert!(config.plugins.contains_key("other"));
    assert_eq!(
        config.integrations.voice_transcribe.active,
        schema::default_transcribe_active()
    );
}

#[test]
fn serialized_plugin_settings_drop_legacy_pin_and_preserve_other_fields() {
    let mut config = AppConfig::default();
    config.plugins.insert(
        "bitwarden".to_string(),
        serde_json::json!({
            "pinned": true,
            "serverUrl": "https://vault.example",
            "defaultOrganization": "personal"
        }),
    );

    let serialized = serialize_config(&config).expect("config should serialize");
    let reloaded: AppConfig = toml::from_str(&serialized).expect("serialized config should parse");
    let settings = reloaded
        .plugins
        .get("bitwarden")
        .expect("plugin settings should remain");

    assert_eq!(settings["serverUrl"], "https://vault.example");
    assert_eq!(settings["defaultOrganization"], "personal");
    assert!(settings.get("pinned").is_none());
    assert!(config.plugins["bitwarden"].get("pinned").is_some());
}

/// General config invariant: a key the current schema does not recognise
/// (a future field, or one dropped in this codebase's own history) is not
/// silently destroyed by a load-modify-save round trip. Per-provider
/// settings use `#[serde(flatten)] extra: HashMap<String, toml::Value>`
/// specifically so plugin-declared keys survive a schema that predates
/// them; this pins that the flatten bag actually round-trips.
#[test]
fn load_modify_save_round_trip_preserves_unrecognized_provider_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(
            &path,
            "[agents.claude]\nresume_sessions = true\nfuture_unknown_key = \"kept\"\nintegration_consent_granted = true\nintegration_consent_recorded_at = 123\nfixture_extension_flag = true\n",
        )
        .unwrap();

    let cache: ConfigCache = Mutex::new(None);
    let mut config = load_config_cached(&path, &cache);
    assert_eq!(
        config
            .agents
            .get("claude")
            .and_then(|p| p.extra.get("future_unknown_key"))
            .and_then(|v| v.as_str()),
        Some("kept"),
        "an unrecognized key must survive the initial load"
    );
    let extra = &config.agents["claude"].extra;
    assert_eq!(
        extra
            .get(crate::schema::INTEGRATION_CONSENT_GRANTED)
            .and_then(toml::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        extra
            .get("integration_consent_recorded_at")
            .and_then(toml::Value::as_integer),
        Some(123)
    );
    assert_eq!(
        extra
            .get("fixture_extension_flag")
            .and_then(toml::Value::as_bool),
        Some(true)
    );

    config.font.size = 20; // unrelated modification the round trip must also keep
    let toml_str = toml::to_string_pretty(&config).unwrap();
    std::fs::write(&path, &toml_str).unwrap();

    let reloaded: AppConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(reloaded.font.size, 20);
    assert_eq!(
        reloaded
            .agents
            .get("claude")
            .and_then(|p| p.extra.get("future_unknown_key"))
            .and_then(|v| v.as_str()),
        Some("kept"),
        "the unrecognized key must still be present after save + reload"
    );
    let extra = &reloaded.agents["claude"].extra;
    assert_eq!(
        extra
            .get(crate::schema::INTEGRATION_CONSENT_GRANTED)
            .and_then(toml::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        extra
            .get("integration_consent_recorded_at")
            .and_then(toml::Value::as_integer),
        Some(123)
    );
    assert_eq!(
        extra
            .get("fixture_extension_flag")
            .and_then(toml::Value::as_bool),
        Some(true)
    );
}

#[test]
fn test_config_path_is_valid() {
    let path = config_path();
    assert!(path.is_ok());
    let p = path.unwrap();
    assert!(p.to_string_lossy().contains("domios"));
    assert!(p.to_string_lossy().contains("config.toml"));
}

#[test]
fn held_config_refuses_update_without_changing_memory_or_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    let initial = AppConfig::default();
    let disk_before = load::mark_salvaged_config(&serialize_config(&initial).unwrap());
    std::fs::write(&path, &disk_before).unwrap();
    load::config_file(&path).hold_for_repair();

    let state = Mutex::new(initial);
    let mut update = AppConfig::default();
    update.terminal.scrollback = 10001;
    let error = save_config_and_apply_at(&path, &state, update).unwrap_err();

    assert!(error.contains("Config is held"));
    assert!(error.contains("remove"));
    assert!(error.contains("restore"));
    assert_eq!(state.lock().terminal.scrollback, 10000);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), disk_before);
    codeterm_util::persisted_state::release_hold(&path);
}

#[test]
fn runtime_credential_cannot_enter_serialized_config_or_change_on_settings_write() {
    let mut config = AppConfig::default();
    config.api.token = Some("runtime-authority".into());
    assert!(!serialize_config(&config)
        .unwrap()
        .contains("runtime-authority"));
    assert!(serde_json::to_value(&config).unwrap()["api"]
        .get("token")
        .is_none());
    let tmp = tempfile::tempdir().unwrap();
    let state = Mutex::new(config);
    let mut update = AppConfig::default();
    update.font.size = 18;
    save_config_and_apply_at(&tmp.path().join("config.toml"), &state, update).unwrap();
    assert_eq!(state.lock().api.token.as_deref(), Some("runtime-authority"));
    assert_eq!(state.lock().font.size, 18);
}
