use super::*;

#[test]
fn passive_projection_does_not_overwrite_a_later_user_addition() {
    let _lock = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let _ctx = crate::enter_ctx(crate::PluginCtx {
        data_root: root.path().into(),
        version: None,
    });
    let old = serde_json::json!({ "channels": { "codeterm-core": {
        "name": "codeterm-core", "url": "https://github.com/rollacode/codeterm.git#master:plugins",
        "kind": "git", "path": "old-cache"
    }}})
    .to_string();
    fs::write(channel_registry_path(), &old).unwrap();
    let snapshot = read_channel_registry().unwrap();
    assert_eq!(
        snapshot.channels[SYSTEM_CHANNEL_NAME].url,
        system_channel_url()
    );
    assert_eq!(fs::read_to_string(channel_registry_path()).unwrap(), old);
    mutate_channel_registry(|registry| {
        registry.channels.insert(
            "user-added".into(),
            ChannelRecord {
                name: "user-added".into(),
                url: "https://example.invalid/user.git".into(),
                path: "user-cache".into(),
                kind: ChannelKind::Git,
            },
        );
        Ok(())
    })
    .unwrap();
    mutate_channel_registry(|_| Ok(())).unwrap();
    let persisted: ChannelRegistry =
        serde_json::from_str(&fs::read_to_string(channel_registry_path()).unwrap()).unwrap();
    assert!(persisted.channels.contains_key("user-added"));
    assert_eq!(
        persisted.channels[SYSTEM_CHANNEL_NAME].url,
        system_channel_url()
    );
}

#[test]
fn persistence_refusal_preserves_original_bytes_and_passive_reads() {
    let _lock = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let _ctx = crate::enter_ctx(crate::PluginCtx {
        data_root: root.path().into(),
        version: None,
    });
    let original = r#"{"channels":{"codeterm-core":{"name":"codeterm-core","url":"https://github.com/rollacode/codeterm.git#master:plugins","kind":"git","path":"cache"}}}"#;
    fs::write(channel_registry_path(), original).unwrap();
    let result = mutate_channel_registry_with(|_| Ok(()), |_| Err("persistence refused".into()));
    assert_eq!(result.unwrap_err(), "persistence refused");
    assert_eq!(
        fs::read_to_string(channel_registry_path()).unwrap(),
        original
    );
    assert_eq!(
        read_channel_registry().unwrap().channels[SYSTEM_CHANNEL_NAME].url,
        system_channel_url()
    );
}

#[test]
fn stored_system_channel_migrates_without_touching_user_channels_or_cache() {
    let old = "https://github.com/rollacode/codeterm.git#master:plugins";
    let mut registry = ChannelRegistry::default();
    for name in [SYSTEM_CHANNEL_NAME, "user-channel"] {
        registry.channels.insert(
            name.into(),
            ChannelRecord {
                name: name.into(),
                url: old.into(),
                path: format!("cache/{name}"),
                kind: ChannelKind::Git,
            },
        );
    }
    assert!(migrate_system_channel(
        &mut registry,
        DEFAULT_SYSTEM_CHANNEL_URL
    ));
    let core = &registry.channels[SYSTEM_CHANNEL_NAME];
    assert_eq!(core.url, DEFAULT_SYSTEM_CHANNEL_URL);
    assert_eq!(core.kind, ChannelKind::Archive);
    assert_eq!(core.path, "cache/codeterm-core");
    assert_eq!(registry.channels["user-channel"].url, old);
    assert_eq!(registry.channels["user-channel"].kind, ChannelKind::Git);
    assert!(!migrate_system_channel(
        &mut registry,
        DEFAULT_SYSTEM_CHANNEL_URL
    ));
}

#[test]
fn system_channel_migration_honors_explicit_developer_source() {
    let mut registry = ChannelRegistry::default();
    assert!(!migrate_system_channel(
        &mut registry,
        DEFAULT_SYSTEM_CHANNEL_URL
    ));
    registry.channels.insert(
        SYSTEM_CHANNEL_NAME.into(),
        ChannelRecord {
            name: SYSTEM_CHANNEL_NAME.into(),
            url: DEFAULT_SYSTEM_CHANNEL_URL.into(),
            path: "cache".into(),
            kind: ChannelKind::Archive,
        },
    );
    assert!(migrate_system_channel(
        &mut registry,
        "file:///fixtures/plugins"
    ));
    assert_eq!(
        registry.channels[SYSTEM_CHANNEL_NAME].kind,
        ChannelKind::Local
    );
}
