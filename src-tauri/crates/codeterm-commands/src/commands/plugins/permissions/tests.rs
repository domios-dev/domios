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

#[test]
fn updated_declarations_only_leave_new_permissions_undecided() {
    let grants =
        serde_json::from_value(json!({ "local_recording": true, "spawn_agents": false })).unwrap();
    for (permissions, pending) in [
        (
            json!({ "localRecording": true, "spawnAgents": true }),
            vec![],
        ),
        (
            json!({ "localRecording": true, "spawnAgents": true, "projectWrite": true }),
            vec!["project_write"],
        ),
        (json!({ "spawnAgents": true }), vec![]),
    ] {
        let states = permission_states(
            &json!({ "version": "2.0.0", "permissions": permissions }),
            &grants,
        );
        assert_eq!(
            states
                .iter()
                .filter(|state| !state.decided)
                .map(|state| state.key.as_str())
                .collect::<Vec<_>>(),
            pending
        );
        assert!(states
            .iter()
            .filter(|state| state.key == "local_recording")
            .all(|state| state.allowed && state.decided));
        assert!(states
            .iter()
            .filter(|state| state.key == "spawn_agents")
            .all(|state| !state.allowed && state.decided));
        assert!(states
            .iter()
            .filter(|state| state.key == "project_write")
            .all(|state| !state.allowed && !state.decided));
    }
}
