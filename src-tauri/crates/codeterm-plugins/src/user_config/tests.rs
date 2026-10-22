use super::*;

fn write(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn user_config_wins_over_packaged_and_grants_do_not_carry() {
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("installed");
    let staged = root.path().join("staged");
    write(&installed.join("config.yaml"), "engine: user\n");
    write(&installed.join("grants.yaml"), "spawn_agents: true\n");
    write(&installed.join("state.json"), "{}");
    write(&staged.join("config.yaml"), "engine: packaged\n");

    carry_user_config(&installed, &staged).unwrap();

    assert_eq!(
        fs::read_to_string(staged.join("config.yaml")).unwrap(),
        "engine: user\n"
    );
    assert!(codeterm_util::private_file::owner_only(&staged.join("config.yaml")).unwrap());
    assert!(!staged.join("grants.yaml").exists());
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
fn reinstall_from_dir_preserves_only_config_yaml() {
    let root = tempfile::tempdir().unwrap();
    let _ctx = enter_ctx(PluginCtx {
        data_root: root.path().join("data"),
        version: None,
    });
    let source = root.path().join("source");
    write(&source.join("plugin.json"), r#"{"id":"keep-config"}"#);
    write(&source.join("plugin.js"), "module.exports = {};");
    write(&source.join("config.yaml"), "engine: packaged\n");

    install_from_dir(&source, "keep-config", None).unwrap();
    let dest = plugin_dir(&PluginId::parse("keep-config".to_string()).unwrap());
    assert_eq!(
        fs::read_to_string(dest.join("config.yaml")).unwrap(),
        "engine: packaged\n"
    );
    write(&dest.join("config.yaml"), "engine: user\n");
    write(&dest.join("grants.yaml"), "spawn_agents: true\n");
    write(&source.join("plugin.js"), "module.exports = { v: 2 };");

    install_from_dir(&source, "keep-config", None).unwrap();

    assert_eq!(
        fs::read_to_string(dest.join("config.yaml")).unwrap(),
        "engine: user\n"
    );
    assert!(!dest.join("grants.yaml").exists());
    assert_eq!(
        fs::read_to_string(dest.join("plugin.js")).unwrap(),
        "module.exports = { v: 2 };"
    );
}
