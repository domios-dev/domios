use super::*;

fn write(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn user_config_and_unchanged_permission_grants_win_over_packaged() {
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("installed");
    let staged = root.path().join("staged");
    for dir in [&installed, &staged] {
        write(
            &dir.join("plugin.json"),
            r#"{"id":"same","permissions":{"spawn_agents":true}}"#,
        );
    }
    write(&installed.join("config.yaml"), "engine: user\n");
    write(&installed.join("grants.yaml"), "spawn_agents: true\n");
    write(&installed.join("state.json"), "{}");
    write(&staged.join("config.yaml"), "engine: packaged\n");
    write(&staged.join("grants.yaml"), "secrets: true\n");

    carry_user_config(&installed, &staged).unwrap();

    assert_eq!(
        fs::read_to_string(staged.join("config.yaml")).unwrap(),
        "engine: user\n"
    );
    assert!(codeterm_util::private_file::owner_only(&staged.join("config.yaml")).unwrap());
    assert_eq!(
        fs::read_to_string(staged.join("grants.yaml")).unwrap(),
        "spawn_agents: true\n"
    );
    assert!(codeterm_util::private_file::owner_only(&staged.join("grants.yaml")).unwrap());
    assert!(!staged.join("state.json").exists());
}

#[test]
fn missing_user_config_keeps_packaged_one() {
    let root = tempfile::tempdir().unwrap();
    let staged = root.path().join("staged");
    write(&staged.join("config.yaml"), "engine: packaged\n");

    carry_user_config(&root.path().join("absent"), &staged).unwrap();

    assert_eq!(
        fs::read_to_string(staged.join("config.yaml")).unwrap(),
        "engine: packaged\n"
    );
}

#[test]
fn reinstall_from_dir_preserves_config_and_grants_across_version_upgrade() {
    let root = tempfile::tempdir().unwrap();
    let _ctx = enter_ctx(PluginCtx {
        data_root: root.path().join("data"),
        version: None,
    });
    let source = root.path().join("source");
    write(
        &source.join("plugin.json"),
        r#"{"id":"keep-config","version":"0.1.0","permissions":{"local_recording":true}}"#,
    );
    write(&source.join("plugin.js"), "module.exports = {};");
    write(&source.join("config.yaml"), "engine: packaged\n");

    install_from_dir(&source, "keep-config", None).unwrap();
    let dest = plugin_dir(&PluginId::parse("keep-config".to_string()).unwrap());
    assert_eq!(
        fs::read_to_string(dest.join("config.yaml")).unwrap(),
        "engine: packaged\n"
    );
    write(&dest.join("config.yaml"), "engine: user\n");
    write(
        &dest.join("grants.yaml"),
        "local_recording: true\nspawn_agents: false\n",
    );
    write(
        &source.join("plugin.json"),
        r#"{"id":"keep-config","version":"0.1.3","permissions":{"local_recording":true}}"#,
    );
    write(&source.join("plugin.js"), "module.exports = { v: 2 };");

    install_from_dir(&source, "keep-config", None).unwrap();

    assert_eq!(
        fs::read_to_string(dest.join("config.yaml")).unwrap(),
        "engine: user\n"
    );
    assert_eq!(
        fs::read_to_string(dest.join("grants.yaml")).unwrap(),
        "local_recording: true\nspawn_agents: false\n"
    );
    assert_eq!(
        fs::read_to_string(dest.join("plugin.js")).unwrap(),
        "module.exports = { v: 2 };"
    );
}

#[test]
fn permission_changes_keep_grants_and_revocations_by_plugin_identity() {
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("installed");
    write(
        &installed.join("plugin.json"),
        r#"{"id":"same","permissions":{"local_recording":true}}"#,
    );
    write(
        &installed.join("grants.yaml"),
        "local_recording: true\nspawn_agents: false\n",
    );
    for (index, manifest) in [
        r#"{"id":"same","permissions":{"local_recording":true,"secrets":true}}"#,
        r#"{"id":"same","permissions":{}}"#,
        r#"{"id":"same","permissions":{"local_recording":true},"capabilities":{"localRecordingTrigger":{}}}"#,
    ].iter().enumerate() {
        let staged = root.path().join(index.to_string());
        write(&staged.join("plugin.json"), manifest);
        carry_user_config(&installed, &staged).unwrap();
        let grants: codeterm_util::plugin_grants::PluginGrants =
            serde_yaml::from_str(&fs::read_to_string(staged.join("grants.yaml")).unwrap()).unwrap();
        assert_eq!(grants.permissions.get("local_recording"), Some(&true));
        assert_eq!(grants.permissions.get("spawn_agents"), Some(&false));
        assert!(!grants.permissions.contains_key("secrets"));
    }
}

#[test]
fn packaged_grants_never_supply_owner_decisions() {
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("installed");
    write(&installed.join("plugin.json"), r#"{"id":"original"}"#);
    write(&installed.join("grants.yaml"), "local_recording: true\n");
    for (index, id) in ["other", "original"].iter().enumerate() {
        let staged = root.path().join(index.to_string());
        write(&staged.join("plugin.json"), &format!(r#"{{"id":"{id}"}}"#));
        write(&staged.join("grants.yaml"), "secrets: true\n");
        let previous = if index == 0 {
            installed.clone()
        } else {
            root.path().join("absent")
        };
        carry_user_config(&previous, &staged).unwrap();
        assert!(!staged.join("grants.yaml").exists());
    }
}
