use super::*;

#[test]
fn live_command_mode_and_templates_round_trip_through_manifest() {
    let mut manifest: serde_json::Value = serde_json::from_str(MIN_MANIFEST).unwrap();
    manifest["commands"]["modelSwitch"] = json!("live_command");
    manifest["commands"]["modelSwitchCommands"] = json!({
        "model": {"command": "/choose {model}", "confirmation": "^accepted {model}$"},
        "effort": {"command": "/reason {effort}", "confirmation": "^accepted {effort}$"},
        "transcript": {"recordType": "opaque", "contentPath": ["payload"], "prefix": "<ok>", "suffix": "</ok>"}
    });
    let provider = load(&manifest.to_string(), "module.exports.default = {};");
    assert_eq!(
        provider.model_switch(),
        crate::provider::ModelSwitchMode::LiveCommand
    );
    let commands = provider.model_switch_commands().unwrap();
    assert_eq!(
        serde_json::to_value(commands).unwrap(),
        manifest["commands"]["modelSwitchCommands"]
    );
    manifest["commands"]["modelSwitchCommands"]["model"]["typo"] = json!(true);
    assert!(serde_json::from_value::<PluginManifest>(manifest).is_err());
}
