use super::*;

#[test]
fn live_command_mode_and_templates_round_trip_through_manifest() {
    let mut manifest: serde_json::Value = serde_json::from_str(MIN_MANIFEST).unwrap();
    manifest["commands"]["modelSwitch"] = json!("live_command");
    manifest["commands"]["modelSwitchCommands"] = json!({
        "model": {"command": "/choose", "confirmation": "^accepted {model}$",
            "picker": {"screenRegex":"^picker", "selectedRegex":"selected (?P<position>\\d+)",
                "targetRegex":"(?P<position>\\d+) {model}", "position":"number",
                "previousKey":"up", "nextKey":"down", "responseKey":"s", "cancelKey":"escape"},
            "prompt": {"screenRegex": "^confirm {model}$", "responseKeys": ["y", "\r"], "cancelKeys": ["\u{1b}"]}},
        "effort": {"command": "/reason {effort}", "confirmation": "^accepted {effort}$"},
        "transcript": {"recordType": "opaque", "contentPath": ["payload"], "prefix": "<ok>", "suffix": "</ok>"}
    });
    let provider = load(&manifest.to_string(), "module.exports.default = {};");
    assert_eq!(
        provider.model_switch(),
        crate::provider::ModelSwitchMode::LiveCommand
    );
    let mut invalid = manifest.clone();
    invalid["commands"]["modelSwitchCommands"]["model"]["picker"]["position"] = json!("guess");
    assert!(serde_json::from_value::<PluginManifest>(invalid).is_err());
    let mut invalid = manifest.clone();
    invalid["commands"]["modelSwitchCommands"]["model"]["prompt"]["typo"] = json!(true);
    assert!(serde_json::from_value::<PluginManifest>(invalid).is_err());
    let mut invalid = manifest.clone();
    invalid["commands"]["modelSwitchCommands"]["model"]["prompt"]["responseKeys"] = json!("y");
    assert!(serde_json::from_value::<PluginManifest>(invalid).is_err());
    let mut invalid = manifest.clone();
    invalid["commands"]["modelSwitchCommands"]["model"]["prompt"]["cancelKeys"] = json!("escape");
    assert!(serde_json::from_value::<PluginManifest>(invalid).is_err());
    let commands = provider.model_switch_commands().unwrap();
    assert_eq!(
        serde_json::to_value(commands).unwrap(),
        manifest["commands"]["modelSwitchCommands"]
    );
    manifest["commands"]["modelSwitchCommands"]["model"]["typo"] = json!(true);
    assert!(serde_json::from_value::<PluginManifest>(manifest).is_err());
}
