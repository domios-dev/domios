use super::*;
use codeterm_plugin::agent_runtime::AgentRuntimeCommand;
use codeterm_plugin::provider::ProviderAvailability;
use serde_json::json;

#[test]
fn provider_readiness_uses_receipts_and_rechecks_changed_profiles() {
    let root = tempfile::tempdir().unwrap();
    let mut capability: AgentRuntimeCapability = serde_json::from_value(json!({
        "runtime":"stdio-jsonrpc",
        "command":{"binary":"provider-cli-that-does-not-exist","args":[]},
        "requestMethods":{}, "notificationMethods":[],
        "launchProfiles":{"configured":{"binary":"fixture-agent","args":[],"ownerConfigured":true}}
    }))
    .unwrap();
    let observe = |capability: &AgentRuntimeCapability| {
        observe_from_root(capability, Ok(root.path().to_path_buf())).unwrap()
    };
    assert_eq!(observe(&capability).installed, Some(false));
    let profile = &capability.launch_profiles.as_ref().unwrap()["configured"];
    launch_profile_install::confirm_local(
        root.path(),
        "configured",
        profile,
        AgentRuntimeCommand {
            binary: std::env::current_exe().unwrap().to_str().unwrap().into(),
            args: vec![],
        },
        true,
    )
    .unwrap();
    let ready = observe(&capability);
    assert_eq!(ready.installed, Some(true));
    assert_eq!(ready.availability, ProviderAvailability::Available);
    assert!(!ready.required);
    assert!(ready.binary.is_none());
    assert_eq!(ready.launch_profiles.unwrap()[0].id, "configured");
    capability
        .launch_profiles
        .as_mut()
        .unwrap()
        .get_mut("configured")
        .unwrap()
        .args
        .push("changed".into());
    assert_eq!(observe(&capability).installed, Some(false));
    std::fs::write(root.path().join("configured.json"), b"malformed receipt").unwrap();
    assert_eq!(observe(&capability).installed, None);
    assert_eq!(
        observe(&capability).availability,
        ProviderAvailability::Unknown
    );
    std::fs::remove_file(root.path().join("configured.json")).unwrap();
    assert_eq!(observe(&capability).installed, Some(false));
    capability.launch_profiles = None;
    assert!(observe_from_root(&capability, Ok(root.path().to_path_buf())).is_none());
}
