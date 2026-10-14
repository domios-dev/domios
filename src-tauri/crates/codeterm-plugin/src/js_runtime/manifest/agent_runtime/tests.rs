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
