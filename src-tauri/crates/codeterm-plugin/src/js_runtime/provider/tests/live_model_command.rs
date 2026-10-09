use super::*;

#[test]
fn session_identity_manifest_needs_no_transcript_confirmation_regex() {
    let mut manifest: serde_json::Value = serde_json::from_str(MIN_MANIFEST).unwrap();
    manifest["commands"]["modelSwitch"] = json!("live_command");
    manifest["commands"]["modelSwitchCommands"] = json!({
        "confirmationSource": {"kind": "session_identity"},
        "model": {"command": "/model {model}"},
        "effort": {"command": "/effort {effort}"}
    });
    let provider = load(&manifest.to_string(), "module.exports.default = {};");
    let commands = provider.model_switch_commands().unwrap();
    assert_eq!(
        commands.confirmation_source,
        crate::provider::ModelSwitchConfirmationSource::SessionIdentity
    );
    manifest["commands"]["modelSwitchCommands"]["turnCompletion"] = json!({
        "eventPointer": "/params/update/sessionUpdate", "eventValue": "turn_completed", "sessionPointer": "/params/sessionId",
        "startEventValue": "user_message_chunk", "turnPointer": "/params/update/prompt_id", "activeTurnPointer": "/params/_meta/promptId"
    });
    let provider = load(&manifest.to_string(), "module.exports.default = {};");
    let commands = provider.model_switch_commands().unwrap();
    assert_eq!(
        serde_json::to_value(commands.turn_completion.unwrap()).unwrap(),
        manifest["commands"]["modelSwitchCommands"]["turnCompletion"]
    );
    let root = tempfile::tempdir().unwrap();
    let foreign = tempfile::NamedTempFile::new().unwrap();
    manifest["sessions"] = json!({"rootDir": root.path().to_string_lossy(), "cwdEncoding": "none"});
    let script = format!(
        "module.exports.default = {{ sessionJsonlPath() {{ return {}; }} }};",
        serde_json::to_string(&foreign.path().to_string_lossy()).unwrap()
    );
    let provider = load(&manifest.to_string(), &script);
    assert!(provider.session_jsonl_path("/work", "own").is_none());
    manifest["commands"]["modelSwitchCommands"]["confirmationSource"]["kind"] = json!("guess");
    assert!(serde_json::from_value::<PluginManifest>(manifest).is_err());
}

#[test]
fn live_command_mode_and_templates_round_trip_through_manifest() {
    let mut manifest: serde_json::Value = serde_json::from_str(MIN_MANIFEST).unwrap();
    manifest["commands"]["modelSwitch"] = json!("live_command");
    manifest["commands"]["modelSwitchCommands"] = json!({
        "confirmationSource": {"kind": "transcript"},
        "model": {"command": "/choose", "confirmation": "^accepted {model}$",
            "picker": {"screenRegex":"^picker", "selectedRegex":"selected (?P<position>\\d+)",
                "targetRegex":"(?P<position>\\d+) {model}", "position":"number",
                "previousKey":"up", "nextKey":"down", "responseKey":"s", "cancelKey":"escape"},
            "prompt": {"screenRegex": "^confirm {model}$", "responseKeys": ["y", "\r"], "cancelKeys": ["\u{1b}"]}},
        "effort": {"command": "/reason {effort}", "confirmation": "^accepted {effort}$"},
        "transcript": {"recordType": "opaque", "contentPath": ["payload"], "prefix": "<ok>", "suffix": "</ok>", "ignoredContentRegexes": ["^opaque-command-caveat$"]}
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
