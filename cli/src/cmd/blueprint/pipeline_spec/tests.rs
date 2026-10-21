use super::*;

#[test]
fn blueprint_spec_reader_parses_json_and_yaml_and_applies_owner() {
    let dir = tempfile::tempdir().unwrap();
    for (name, source) in [
        (
            "spec.json",
            r#"{"name":"echo","trigger":{"type":"schedule","every":"20s"},"steps":[]}"#,
        ),
        (
            "spec.yaml",
            "name: echo\ntrigger:\n  type: schedule\n  every: 20s\nsteps: []\n",
        ),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, source).unwrap();
        let value = load(Some(&path), None, &[], Some("team")).unwrap();
        assert_eq!(value["name"], "echo");
        assert_eq!(value["owner"], "team");
    }
}

#[test]
fn blueprint_recipe_set_replaces_declared_placeholders_once() {
    let mut value = serde_json::json!({"command": "printf '{{message}}'"});
    substitute(&mut value, &["message=ready".into()]).unwrap();
    assert_eq!(value["command"], "printf 'ready'");
    assert!(substitute(&mut value, &["missing=x".into()]).is_err());
}
#[test]
fn github_recipe_refuses_missing_and_non_numeric_inputs() {
    assert!(load(None, Some("github"), &[], None)
        .unwrap_err()
        .contains("missing recipe placeholders"));
    assert!(load(
        None,
        Some("github"),
        &[
            "repo=cli/cli".into(),
            "resource=run".into(),
            "id=oops".into()
        ],
        None
    )
    .unwrap_err()
    .contains("GitHub id"));
}
