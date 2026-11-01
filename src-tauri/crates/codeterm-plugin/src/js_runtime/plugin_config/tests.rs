use super::*;

#[test]
fn empty_comments_and_malformed_yaml_resolve_null() {
    for text in ["", "# comment\n", "items: ["] {
        assert_eq!(resolve_settings_inner(Some(text), "fixture"), "null");
    }
}

#[test]
fn yaml_to_json_maps_scalars_and_nested() {
    let json = yaml_to_json("engine: whisper\nport: 8080\nnested:\n  a: 1\n").unwrap();
    assert_eq!(json["engine"], "whisper");
    assert_eq!(json["port"], 8080);
    assert_eq!(json["nested"]["a"], 1);
}

#[test]
fn yaml_empty_is_null() {
    assert_eq!(yaml_to_json("").unwrap(), Value::Null);
    assert_eq!(yaml_to_json("   \n").unwrap(), Value::Null);
}

#[test]
fn config_yaml_settings_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let id = "transcribe-fixture-q9";
    std::fs::write(
        config_yaml_path(dir.path(), id),
        "# the engine to use\nengine: whisper\n",
    )
    .unwrap();
    let json = resolve_settings_json(dir.path(), id);
    let parsed: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["engine"], "whisper");
}

#[test]
fn missing_config_yaml_resolves_null() {
    let dir = tempfile::tempdir().unwrap();
    let json = resolve_settings_json(dir.path(), "no-such-plugin-xyz");
    assert_eq!(json, "null");
}

#[test]
fn workspace_bundle_reads_the_config_written_to_its_installed_copy() {
    let root = tempfile::tempdir().unwrap();
    let user = root.path().join("plugins");
    let installed = user.join("codex");
    let workspace_bundle = root.path().join("repo/plugins/provider-codex");
    std::fs::create_dir_all(&installed).unwrap();
    std::fs::create_dir_all(&workspace_bundle).unwrap();
    std::fs::write(installed.join("plugin.json"), "{}").unwrap();
    std::fs::write(installed.join("config.yaml"), "engine: user\n").unwrap();
    std::fs::write(workspace_bundle.join("config.yaml"), "engine: stale\n").unwrap();

    let path = config_yaml_path_in(&user, &workspace_bundle, "codex");

    assert_eq!(path, installed.join("config.yaml"));
    let text = std::fs::read_to_string(path).unwrap();
    assert_eq!(yaml_to_json(&text).unwrap()["engine"], "user");
}
