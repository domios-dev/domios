use super::*;

fn profile(settings: serde_json::Value) -> RuntimeLaunchProfile {
    serde_json::from_value(serde_json::json!({
        "binary": "opaque-agent",
        "args": [],
        "settingsFiles": {"OPAQUE_SETTINGS_PATH": settings}
    }))
    .unwrap()
}

#[test]
fn declared_settings_reach_a_host_owned_file_named_by_their_variable() {
    let root = tempfile::tempdir().unwrap();
    let settings = serde_json::json!({"opaque": {"nested": false}});
    let env = write_all(root.path(), &profile(settings.clone())).unwrap();

    let path = std::path::PathBuf::from(&env["OPAQUE_SETTINGS_PATH"]);
    assert!(path.starts_with(root.path()));
    let written: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(written, settings);

    let changed = serde_json::json!({"opaque": {"nested": true}});
    let again = write_all(root.path(), &profile(changed.clone())).unwrap();
    assert_eq!(again, env);
    let rewritten: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(rewritten, changed);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
