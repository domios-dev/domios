use super::*;
use serde_json::json;

#[test]
fn declarations_require_an_explicit_recording_grant_and_preserve_revocation() {
    let manifest = json!({ "permissions": { "localRecording": true, "spawnAgents": true } });
    let mut grants = codeterm_util::plugin_grants::PluginGrants::default();
    assert_eq!(permission_states(&manifest, &grants)[0].allowed, false);
    assert_eq!(permission_states(&manifest, &grants)[1].allowed, true);
    grants.permissions.insert("local_recording".into(), true);
    grants.permissions.insert("spawn_agents".into(), false);
    let states = serde_json::json!(permission_states(&manifest, &grants));
    assert_eq!(states[0]["allowed"], true);
    assert_eq!(states[1]["allowed"], false);
}

#[test]
fn recording_capability_declares_permission_without_granting_it() {
    let manifest = json!({ "capabilities": { "localRecordingTrigger": { "provider": "input" } } });
    assert_eq!(declared_permissions(&manifest).len(), 1);
    assert!(validate_permission(&manifest, "local_recording").is_ok());
    assert!(validate_permission(&manifest, "spawn_agents").is_err());
    assert!(validate_permission(&manifest, "unknown").is_err());
    assert!(validate_permission(&manifest, "network:api.test").is_err());
    let network = json!({ "permissions": { "network": { "allow": [] } } });
    assert!(validate_permission(&network, "network:api.test:8443").is_ok());
    assert!(validate_permission(&network, "network:*.test").is_err());
}
