use super::*;

#[test]
fn plugin_id_rejects_traversal_empty_reserved_and_system_ids() {
    for id in ["", "../evil", "a/b", "CON", "nul.txt", "COM9.log"] {
        assert!(PluginId::parse(id.to_string()).is_err(), "accepted {id:?}");
    }
    assert!(PluginId::parse("x".repeat(65)).is_err());
    let root = std::env::temp_dir().join(format!("plugin-id-validation-{}", uuid::Uuid::new_v4()));
    for id in ["", "../outside", "CON", "a/b"] {
        let err = run(
            PluginCtx {
                data_root: root.clone(),
                version: None,
            },
            PluginAction::Remove {
                id: id.into(),
                force: true,
                remove_data: false,
            },
        )
        .unwrap_err();
        assert!(!root.exists(), "invalid id reached filesystem: {id:?}");
        assert!(!err.is_empty());
    }
    for id in ["", "../outside", "CON", "a/b"] {
        let err = run(
            PluginCtx {
                data_root: root.clone(),
                version: None,
            },
            PluginAction::Install {
                id: None,
                from_dir: Some("missing-source".into()),
                id_override: Some(id.into()),
                channel: None,
            },
        )
        .unwrap_err();
        assert!(
            !root.exists(),
            "invalid override reached filesystem: {id:?}"
        );
        assert!(!err.is_empty());
    }
}

#[test]
fn remove_never_leaves_plugins_root() {
    for id in ["", "../outside", "CON"] {
        assert!(
            remove_installed(id, true, false).is_err(),
            "accepted {id:?}"
        );
    }
}

#[test]
fn system_ids_are_refused_from_directory_sources() {
    let _guard = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let prev = std::env::var("CODETERM_DATA_DIR").ok();
    let root = std::env::temp_dir().join(format!("codeterm-plugin-test-{}", uuid::Uuid::new_v4()));
    std::env::set_var("CODETERM_DATA_DIR", &root);
    let source = root.join("impostor-source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("plugin.json"),
        serde_json::json!({"id":"codex","version":"1.0.0"}).to_string(),
    )
    .unwrap();
    std::fs::write(source.join("plugin.js"), "module.exports = {};").unwrap();
    let dir = source.display().to_string();
    let by_manifest = install_command(None, Some(dir.clone()), None, None);
    let by_override = install_command(None, Some(dir), Some("codex".into()), None);
    let installed = plugin_dir(&PluginId::validated("codex")).exists();
    match prev {
        Some(value) => std::env::set_var("CODETERM_DATA_DIR", value),
        None => std::env::remove_var("CODETERM_DATA_DIR"),
    }
    let _ = std::fs::remove_dir_all(&root);
    assert!(by_manifest.is_err());
    assert!(by_override.is_err());
    assert!(!installed);
}
