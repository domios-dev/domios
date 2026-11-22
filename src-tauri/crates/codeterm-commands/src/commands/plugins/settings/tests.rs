use super::*;

#[test]
fn ui_saved_structured_array_is_the_settings_json_the_host_reads() {
    let root = tempfile::tempdir().unwrap();
    let id = "settings-store-fixture";
    let dir = root.path().join("plugins").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        serde_json::json!({
            "id": id, "version": "0.1.0", "displayName": "Fixture",
            "settingsSchema": "settings.schema.json"
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.join("settings.schema.json"),
        serde_json::json!([
            {"kind": "text", "key": "language", "label": "Language"},
            {"kind": "pairs", "key": "dictionary", "label": "Custom dictionary"}
        ])
        .to_string(),
    )
    .unwrap();
    std::fs::write(dir.join("config.yaml"), "# Owner notes\nlanguage: auto\n").unwrap();
    let dictionary = serde_json::json!([{ "from": "Что это", "to": "DOMIOS_ARRAY_OK" }]);
    let ctx = || PluginCtx {
        data_root: root.path().to_path_buf(),
        version: None,
    };

    write_with(
        ctx(),
        id,
        Map::from_iter([("dictionary".to_string(), dictionary.clone())]),
    )
    .unwrap();

    let host_view = settings_response(&dir, id);
    assert_eq!(host_view["settings"]["dictionary"], dictionary);
    assert_eq!(host_view["settings"]["language"], "auto");
    let resolved = codeterm_plugin::js_runtime::plugin_config::resolve_settings_json(&dir, id);
    assert_eq!(
        serde_json::from_str::<Value>(&resolved).unwrap(),
        host_view["settings"]
    );
    let vm = codeterm_plugin::js_runtime::PluginVm::from_source_with(
        id,
        r#"module.exports.default = { readSettings() { return JSON.parse(host.settingsJson()); } };"#
            .into(),
        None,
    )
    .unwrap();
    vm.reload_settings(resolved);
    let from_host: Value =
        codeterm_plugin::js_runtime::call_json(&vm, "readSettings", &[]).unwrap();
    assert_eq!(from_host["dictionary"], dictionary);
    assert_eq!(from_host["language"], "auto");
    assert!(std::fs::read_to_string(dir.join("config.yaml"))
        .unwrap()
        .starts_with("# Owner notes\n"));
    let refused = write_with(
        ctx(),
        id,
        Map::from_iter([("undeclared".to_string(), Value::Bool(true))]),
    );
    assert!(refused.unwrap_err().contains("unknown setting"));
}
