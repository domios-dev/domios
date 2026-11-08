use super::*;
use serde_json::{json, Value};

fn manifest() -> Value {
    json!({"id":"runtime-fixture","version":"1.0.0","displayName":"Runtime fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":"runtime-engine","args":["serve"]},
                "requestMethods":{"initialize":["handshake"],"submitTurn":["turn.submit"]},"notificationMethods":["item.changed"]}},
        "permissions":{"subprocess":{"allow":["runtime-engine"]}}})
}

#[test]
fn runtime_manifest_refuses_invalid_declarations_with_typed_reason() {
    let accepted = PluginManifest::from_json_str(&manifest().to_string()).unwrap();
    assert_eq!(accepted.validate_agent_runtime(), Ok(()));
    for (pointer, value, expected) in [
        (
            "/capabilities/agentProvider",
            json!(false),
            RuntimeManifestError::CapabilityRequired,
        ),
        (
            "/capabilities/chatBackend/sessionConfig",
            json!(false),
            RuntimeManifestError::ChatContractRequired,
        ),
        (
            "/capabilities/agentRuntime/command/binary",
            json!(""),
            RuntimeManifestError::InvalidCommand,
        ),
        (
            "/permissions/subprocess/allow",
            json!([]),
            RuntimeManifestError::SubprocessDenied,
        ),
        (
            "/capabilities/agentRuntime/requestMethods",
            json!({"unknown":["any"]}),
            RuntimeManifestError::InvalidMethods,
        ),
        (
            "/capabilities/agentRuntime/requestMethods",
            json!({"initialize":["same","same"]}),
            RuntimeManifestError::InvalidMethods,
        ),
        (
            "/capabilities/agentRuntime/inheritedEnvironmentExclusions",
            json!(["CODETERM_TAB_ID"]),
            RuntimeManifestError::InvalidEnvironmentExclusion,
        ),
    ] {
        let mut raw = manifest();
        if pointer.ends_with("inheritedEnvironmentExclusions") {
            raw["capabilities"]["agentRuntime"]["inheritedEnvironmentExclusions"] = value;
        } else {
            *raw.pointer_mut(pointer).unwrap() = value;
        }
        let parsed: PluginManifest = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(parsed.validate_agent_runtime(), Err(expected));
        assert!(PluginManifest::from_json_str(&raw.to_string()).is_err());
    }
}

#[test]
fn runtime_manifest_schema_and_consent_fail_closed() {
    for patch in [
        json!({"shell":"run"}),
        json!({"args":"serve"}),
        json!({"binary":"engine","args":[],"unknown":true}),
    ] {
        let mut raw = manifest();
        raw["capabilities"]["agentRuntime"]["command"] = patch;
        assert!(PluginManifest::from_json_str(&raw.to_string()).is_err());
    }
    let accepted = PluginManifest::from_json_str(&manifest().to_string()).unwrap();
    assert_eq!(
        accepted
            .capabilities
            .agent_runtime
            .as_ref()
            .unwrap()
            .resolve_executable(),
        Err(RuntimeManifestError::BinaryUnavailable)
    );
    let mut raw = manifest();
    raw["capabilities"]["agentRuntime"]["runtime"] = json!("shell");
    assert!(PluginManifest::from_json_str(&raw.to_string()).is_err());
}

#[test]
fn runtime_transport_is_a_typed_capability() {
    let mut raw = manifest();
    raw["capabilities"]["agentRuntime"]["runtime"] = json!("http-sse");
    let parsed = PluginManifest::from_json_str(&raw.to_string()).unwrap();
    assert_eq!(
        parsed.capabilities.agent_runtime.unwrap().runtime,
        crate::agent_runtime::AgentRuntimeProtocol::HttpSse
    );
    raw["capabilities"]["agentRuntime"]["runtime"] = json!("websocket");
    assert!(PluginManifest::from_json_str(&raw.to_string()).is_err());
}

#[test]
fn runtime_login_declaration_confines_terminal_to_its_binary() {
    let mut value = manifest();
    value["capabilities"]["agentRuntime"]["login"] =
        json!({"methods":["browser","device","apiKey"],"terminalCommand":"runtime-engine login"});
    assert!(PluginManifest::from_json_str(&value.to_string())
        .unwrap()
        .validate_agent_runtime()
        .is_ok());
    for command in [
        "other-engine login",
        "runtime-engine login; env",
        "runtime-engine login\n",
    ] {
        value["capabilities"]["agentRuntime"]["login"]["terminalCommand"] = command.into();
        let refused = PluginManifest::from_json_str(&value.to_string())
            .map_or(true, |manifest| manifest.validate_agent_runtime().is_err());
        assert!(refused, "terminal command must be refused: {command:?}");
    }
}

#[test]
fn launch_profiles_refuse_ungranted_shell_and_unknown_fields() {
    let mut raw = manifest();
    raw["capabilities"]["agentRuntime"]["launchProfiles"] = json!({"selected":{
        "binary":"runtime-engine","args":["serve"],"env":{"PROFILE":"fixed"}
    }});
    assert!(PluginManifest::from_json_str(&raw.to_string()).is_ok());
    for (binary, expected) in [
        ("ungranted", RuntimeManifestError::SubprocessDenied),
        ("/bin/sh", RuntimeManifestError::InvalidCommand),
        ("C:\\Windows\\cmd.exe", RuntimeManifestError::InvalidCommand),
    ] {
        let mut invalid = raw.clone();
        invalid["capabilities"]["agentRuntime"]["launchProfiles"]["selected"]["binary"] =
            json!(binary);
        let parsed: PluginManifest = serde_json::from_value(invalid).unwrap();
        assert_eq!(parsed.validate_agent_runtime(), Err(expected));
    }
    for pointer in ["unknown", "shell"] {
        let mut invalid = raw.clone();
        invalid["capabilities"]["agentRuntime"]["launchProfiles"]["selected"][pointer] =
            json!(true);
        assert!(serde_json::from_value::<PluginManifest>(invalid).is_err());
    }
    raw["capabilities"]["agentRuntime"]["launchProfiles"]["selected"]["env"] =
        json!({"CODETERM_TAB_ID":"foreign"});
    let parsed: PluginManifest = serde_json::from_value(raw).unwrap();
    assert_eq!(
        parsed.validate_agent_runtime(),
        Err(RuntimeManifestError::InvalidProfile)
    );
}

#[test]
fn client_bridges_reject_malformed_and_empty_declarations() {
    for bridges in [
        json!({"fsRead":"yes","fsWrite":false,"terminal":false}),
        json!({"fsRead":true,"fsWrite":true,"terminal":true,"exec":true}),
    ] {
        let mut raw = manifest();
        raw["capabilities"]["agentRuntime"]["clientBridges"] = bridges;
        assert!(PluginManifest::from_json_str(&raw.to_string()).is_err());
    }
    let mut raw = manifest();
    raw["capabilities"]["agentRuntime"]["clientBridges"] =
        json!({"fsRead":false,"fsWrite":false,"terminal":false});
    assert!(PluginManifest::from_json_str(&raw.to_string()).is_err());
    raw["capabilities"]["agentRuntime"]["clientBridges"]["fsRead"] = json!(true);
    assert_eq!(
        PluginManifest::from_json_str(&raw.to_string())
            .unwrap()
            .validate_agent_runtime(),
        Ok(())
    );
}

#[test]
fn interrupt_before_first_turn_requires_explicit_true_and_a_declared_method() {
    let mut raw = manifest();
    for value in [
        json!({"beforeFirstTurn":"yes"}),
        json!({"beforeFirstTurn":true,"always":true}),
    ] {
        raw["capabilities"]["agentRuntime"]["interrupt"] = value;
        assert!(PluginManifest::from_json_str(&raw.to_string()).is_err());
    }
    raw["capabilities"]["agentRuntime"]["interrupt"] = json!({"beforeFirstTurn":true});
    assert!(PluginManifest::from_json_str(&raw.to_string()).is_err());
    raw["capabilities"]["agentRuntime"]["requestMethods"]["interrupt"] = json!(["session/cancel"]);
    assert_eq!(
        PluginManifest::from_json_str(&raw.to_string())
            .unwrap()
            .validate_agent_runtime(),
        Ok(())
    );
    raw["capabilities"]["agentRuntime"]["interrupt"]["beforeFirstTurn"] = json!(false);
    assert!(PluginManifest::from_json_str(&raw.to_string()).is_err());
}

#[test]
fn launch_profiles_require_pinned_packages_and_protect_secret_references() {
    let mut raw = manifest();
    raw["permissions"]["subprocess"]["allow"] = json!(["runtime-engine", "npx"]);
    raw["capabilities"]["agentRuntime"]["launchProfiles"] = json!({"selected":{
        "binary":"npx","args":["@fixture/agent@1.2.3","--acp"],"install":{"package":"@fixture/agent@1.2.3"},
        "secretEnv":{"AGENT_API_KEY":"domios:provider:runtime-fixture:key"}
    }});
    assert!(PluginManifest::from_json_str(&raw.to_string())
        .unwrap()
        .validate_agent_runtime()
        .is_ok());
    for package in [
        "@fixture/agent",
        "@fixture/agent@latest",
        "@fixture/agent@^1.2.3",
    ] {
        let mut invalid = raw.clone();
        invalid["capabilities"]["agentRuntime"]["launchProfiles"]["selected"]["install"]
            ["package"] = json!(package);
        assert!(PluginManifest::from_json_str(&invalid.to_string())
            .map_or(true, |manifest| manifest.validate_agent_runtime().is_err()));
    }
    raw["capabilities"]["agentRuntime"]["launchProfiles"]["selected"]["secretEnv"] =
        json!({"CODETERM_API_TOKEN":"foreign"});
    assert!(PluginManifest::from_json_str(&raw.to_string())
        .map_or(true, |manifest| manifest.validate_agent_runtime().is_err()));
}

#[test]
fn optional_auth_env_binds_bounded_variables_to_distinct_declared_methods() {
    let with_auth_env = |auth_env: serde_json::Value| {
        let mut raw = manifest();
        raw["permissions"]["subprocess"]["allow"] = json!(["runtime-engine", "npx"]);
        raw["capabilities"]["agentRuntime"]["launchProfiles"] = json!({"selected":{
            "binary":"runtime-engine","args":["--acp"],"authEnv":auth_env
        }});
        PluginManifest::from_json_str(&raw.to_string())
            .is_ok_and(|manifest| manifest.validate_agent_runtime().is_ok())
    };
    assert!(with_auth_env(json!({
        "AGENT_API_KEY":{"secret":"domios:provider:runtime-fixture:key","methods":["opaque-key"]}
    })));
    for invalid in [
        json!({"CODETERM_API_TOKEN":{"secret":"domios:provider:runtime-fixture:key","methods":["opaque-key"]}}),
        json!({"AGENT-KEY":{"secret":"domios:provider:runtime-fixture:key","methods":["opaque-key"]}}),
        json!({"AGENT_API_KEY":{"secret":"opaque secret","methods":["opaque-key"]}}),
        json!({"AGENT_API_KEY":{"secret":"domios:provider:runtime-fixture:key","methods":[]}}),
        json!({"AGENT_API_KEY":{"secret":"domios:provider:runtime-fixture:key","methods":["opaque key"]}}),
        json!({
            "AGENT_API_KEY":{"secret":"domios:provider:runtime-fixture:key","methods":["opaque-key"]},
            "AGENT_ALT_KEY":{"secret":"domios:provider:runtime-fixture:alt","methods":["opaque-key"]}
        }),
        json!({"AGENT_API_KEY":{"secret":"domios:provider:runtime-fixture:key"}}),
    ] {
        assert!(!with_auth_env(invalid.clone()), "{invalid}");
    }
}

#[test]
fn runtime_envelope_is_explicit_and_unknown_envelopes_are_rejected() {
    let mut raw = manifest();
    let default = PluginManifest::from_json_str(&raw.to_string()).unwrap();
    assert!(default
        .capabilities
        .agent_runtime
        .unwrap()
        .envelope
        .is_none());
    raw["capabilities"]["agentRuntime"]["envelope"] = json!("jsonrpc2");
    let strict = PluginManifest::from_json_str(&raw.to_string()).unwrap();
    assert_eq!(
        strict.capabilities.agent_runtime.unwrap().envelope,
        Some(crate::agent_runtime::AgentRuntimeEnvelope::Jsonrpc2)
    );
    for invalid in [json!("jsonrpc1"), json!(true), json!({})] {
        raw["capabilities"]["agentRuntime"]["envelope"] = invalid;
        assert!(PluginManifest::from_json_str(&raw.to_string()).is_err());
    }
}

#[test]
fn turn_scoped_requests_require_a_declared_submit_operation() {
    for operations in [
        json!(["submitTurn", "submitTurn"]),
        json!(["initialize"]),
        json!(["unknown"]),
    ] {
        let mut raw = manifest();
        raw["capabilities"]["agentRuntime"]["turnScopedRequests"] = operations;
        assert!(PluginManifest::from_json_str(&raw.to_string()).is_err());
    }
    let mut raw = manifest();
    raw["capabilities"]["agentRuntime"]["turnScopedRequests"] = json!(["submitTurn"]);
    let parsed = PluginManifest::from_json_str(&raw.to_string()).unwrap();
    assert_eq!(
        parsed
            .capabilities
            .agent_runtime
            .unwrap()
            .turn_scoped_requests,
        vec!["submitTurn"]
    );
}

#[test]
fn launch_profile_client_capabilities_are_typed_and_strict() {
    let mut raw = manifest();
    raw["capabilities"]["agentRuntime"]["launchProfiles"] = json!({"selected":{
        "binary":"runtime-engine","args":[],
        "clientCapabilities":{"fs":{"readTextFile":false}}
    }});
    let parsed = PluginManifest::from_json_str(&raw.to_string()).unwrap();
    assert_eq!(parsed.validate_agent_runtime(), Ok(()));
    let runtime = parsed.capabilities.agent_runtime.unwrap();
    let client = runtime.launch_profiles.unwrap()["selected"]
        .client_capabilities
        .clone()
        .unwrap();
    assert_eq!(client.fs.unwrap().read_text_file, Some(false));
    assert_eq!(client.terminal, None);
    for invalid in [
        json!(false),
        json!({"fs":false}),
        json!({"fs":{"readTextFile":"false"}}),
        json!({"fs":{"readTextFile":false,"unknown":true}}),
        json!({"unknown":false}),
        json!({"terminal":0}),
    ] {
        let mut invalid_raw = raw.clone();
        invalid_raw["capabilities"]["agentRuntime"]["launchProfiles"]["selected"]
            ["clientCapabilities"] = invalid;
        assert!(PluginManifest::from_json_str(&invalid_raw.to_string()).is_err());
    }
}
