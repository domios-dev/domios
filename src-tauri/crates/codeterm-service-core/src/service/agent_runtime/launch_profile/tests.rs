use super::*;
use serde_json::json;

fn capability() -> AgentRuntimeCapability {
    serde_json::from_value(json!({
        "runtime":"stdio-jsonrpc", "command":{"binary":"fallback","args":[]},
        "launchProfiles":{"selected":{"binary":"fixed-engine","args":["serve","--stdio"],"env":{"PROFILE":"fixed"}}},
        "requestMethods":{"initialize":["initialize"]},"notificationMethods":[]
    })).unwrap()
}

#[test]
fn known_profile_resolves_exact_argv_env_and_ignores_config_commands() {
    let selected = select(&capability(), &json!({
        "launchProfile":"selected","args":["injected"],"binary":"other","env":{"PROFILE":"injected"}
    }), Some("nonce"), "nonce").unwrap();
    assert_eq!(selected.capability.command.binary, "fixed-engine");
    assert_eq!(selected.capability.command.args, ["serve", "--stdio"]);
    assert_eq!(
        selected.env,
        BTreeMap::from([("PROFILE".into(), "fixed".into())])
    );
    assert_eq!(selected.profile.as_deref(), Some("selected"));
}

#[test]
fn unknown_missing_and_invalid_profile_refuse() {
    for config in [
        json!({"launchProfile":"unknown"}),
        json!({}),
        json!({"launchProfile":{ "binary":"other"}}),
    ] {
        assert!(select(&capability(), &config, Some("nonce"), "nonce").is_err());
    }
}

#[test]
fn nonce_mismatch_refuses() {
    for nonce in [None, Some("foreign")] {
        assert_eq!(
            select(
                &capability(),
                &json!({"launchProfile":"selected"}),
                nonce,
                "nonce"
            )
            .unwrap_err(),
            LaunchProfileError::BindingMismatch
        );
    }
}

#[test]
fn sandbox_environment_overrides_profile_collision() {
    let launch = ResolvedLaunch {
        binary: PathBuf::from("fixed-engine"),
        capability: capability(),
        env: BTreeMap::from([
            ("SANDBOX_POLICY".into(), "profile-override".into()),
            ("PROFILE".into(), "fixed".into()),
        ]),
        working_directory: None,
        profile: Some("selected".into()),
        nonce: "nonce".into(),
    };
    let mut env = HashMap::from([("HOST".into(), "fixed".into())]);
    launch.apply_environment(
        &mut env,
        BTreeMap::from([("SANDBOX_POLICY".into(), "immutable".into())]),
    );
    assert_eq!(
        env.get("SANDBOX_POLICY").map(String::as_str),
        Some("immutable")
    );
    assert_eq!(env.get("PROFILE").map(String::as_str), Some("fixed"));
    assert_eq!(env.get("HOST").map(String::as_str), Some("fixed"));
}

#[test]
fn profile_client_capabilities_only_restrict_selected_bridges() {
    let mut capability = capability();
    capability.client_bridges = Some(codeterm_plugin::agent_runtime::RuntimeClientBridges {
        fs_read: true,
        fs_write: true,
        terminal: true,
    });
    let profiles = capability.launch_profiles.as_mut().unwrap();
    let default = profiles["selected"].clone();
    profiles.insert("default".into(), default);
    profiles.get_mut("selected").unwrap().client_capabilities =
        Some(serde_json::from_value(json!({"fs":{"readTextFile":false}})).unwrap());
    for (profile, expected_read) in [("selected", false), ("default", true)] {
        let selected = select(
            &capability,
            &json!({"launchProfile":profile}),
            Some("nonce"),
            "nonce",
        )
        .unwrap();
        let bridges = selected.capability.client_bridges.unwrap();
        assert_eq!(bridges.fs_read, expected_read);
        assert!(bridges.fs_write);
        assert!(bridges.terminal);
    }
    let selected = select(
        &capability,
        &json!({"launchProfile":"default","clientCapabilities":{"fs":{"readTextFile":false}}}),
        Some("nonce"),
        "nonce",
    )
    .unwrap();
    assert!(selected.capability.client_bridges.unwrap().fs_read);
    let client = capability
        .launch_profiles
        .as_mut()
        .unwrap()
        .get_mut("selected")
        .unwrap()
        .client_capabilities
        .as_mut()
        .unwrap();
    client.fs.as_mut().unwrap().read_text_file = Some(true);
    capability.client_bridges.as_mut().unwrap().fs_read = false;
    let selected = select(
        &capability,
        &json!({"launchProfile":"selected"}),
        Some("nonce"),
        "nonce",
    )
    .unwrap();
    assert!(!selected.capability.client_bridges.unwrap().fs_read);
    capability.client_bridges = None;
    let selected = select(
        &capability,
        &json!({"launchProfile":"selected"}),
        Some("nonce"),
        "nonce",
    )
    .unwrap();
    assert_eq!(selected.capability.client_bridges, None);
}
