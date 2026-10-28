use super::*;
use serde_json::json;

fn command_patch(patch: Value) -> Value {
    let command: super::super::NeighborRuntimeCommand = serde_json::from_value(json!({
        "operation": "set_permission", "friendship_id": "friend", "permission": patch
    })).unwrap();
    let super::super::NeighborRuntimeCommand::SetPermission { permission, .. } = command else {
        panic!("Expected permission Apply");
    };
    permission
}

fn existing() -> Permission {
    Permission {
        allowed: true,
        scope: "Shared project".into(),
        allow_tab_requests: true,
        allow_interactive_content: true,
        allow_memory: true,
        allow_files: false,
        memory_areas: Some(vec!["project".into()]),
        workspaces: Some(vec!["workspace".into()]),
        ..Default::default()
    }
}

#[test]
fn permission_patch_memory_off_areas_all_and_partial_preservation() {
    let original = existing();
    let off = merge(original.clone(), command_patch(json!({"allow_memory": false}))).unwrap();
    let mut expected = original.clone();
    expected.allow_memory = false;
    assert_eq!(off, expected);
    let selected = merge(off, command_patch(json!({"allow_memory": true, "memory_areas": ["team"]}))).unwrap();
    expected.allow_memory = true;
    expected.memory_areas = Some(vec!["team".into()]);
    assert_eq!(selected, expected);
    let all = merge(selected, command_patch(json!({"memory_areas": null}))).unwrap();
    expected.memory_areas = None;
    assert_eq!(all, expected);
    assert_eq!(merge(original.clone(), command_patch(json!({}))).unwrap(), original);
}

#[test]
fn permission_patch_sets_every_field_and_clears_nullable_fields() {
    let folder = std::env::temp_dir().to_string_lossy().into_owned();
    let full = json!({
        "allowed": true, "scope": "Only approved work",
        "allow_tab_requests": false, "allow_interactive_content": false,
        "allow_memory": false, "allow_files": true,
        "folder": folder, "machine_access": "folder",
        "memory_areas": [], "workspaces": []
    });
    let permission = merge(existing(), command_patch(full.clone())).unwrap();
    assert_eq!(serde_json::to_value(&permission).unwrap(), full);
    let cleared = merge(permission, command_patch(json!({
        "allowed": false, "scope": "", "folder": null,
        "machine_access": "read_only", "workspaces": null
    }))).unwrap();
    assert!(!cleared.allowed);
    assert_eq!(cleared.scope, "");
    assert_eq!(cleared.folder, None);
    assert_eq!(cleared.workspaces, None);
    assert_eq!(cleared.memory_areas, Some(vec![]));
    for access in ["none", "read_only", "full"] {
        assert!(merge(existing(), command_patch(json!({"machine_access":access}))).is_ok());
    }
}

#[test]
fn permission_patch_refuses_unknown_fields_values_and_invalid_combinations() {
    for patch in [
        json!(null), json!([]), json!({"unknown": true}),
        json!({"allowed": null}), json!({"scope": null}),
        json!({"allow_memory": "false"}), json!({"allow_files": 0}),
        json!({"allow_tab_requests": null}), json!({"allow_interactive_content": "yes"}),
        json!({"machine_access": "admin"}), json!({"machine_access": null}),
        json!({"machine_access": "folder", "folder": null}),
        json!({"folder": "relative"}), json!({"memory_areas": "all"}),
        json!({"memory_areas": [""]}), json!({"workspaces": [3]}),
        json!({"workspaces": [""]}), json!({"allowed": true, "scope": ""}),
        json!({"scope": "x".repeat(8193)}),
        json!({"memory_areas": vec!["area"; 65]}),
        json!({"workspaces": vec!["workspace"; 65]}),
    ] {
        let error = merge(existing(), command_patch(patch)).unwrap_err();
        assert!(matches!(error, Error::InvalidRequest));
        assert!(error.to_string().starts_with("invalid_request:"));
    }
}
