use codeterm_service_core::multiplexer::types::ShellPurpose;
use codeterm_service_core::service::spawn::{SpawnTabSpec, TabCommand};

#[test]
fn tab_purpose_rest_create_defaults_and_refuses_unknown() {
    use crate::tab_routes::CreateShellTabBody;
    let body: CreateShellTabBody = serde_json::from_str("{}").unwrap();
    assert_eq!(body.purpose, ShellPurpose::Shell);
    let body: CreateShellTabBody = serde_json::from_str(r#"{"purpose":"qa"}"#).unwrap();
    assert_eq!(body.purpose, ShellPurpose::Qa);
    assert!(serde_json::from_str::<CreateShellTabBody>(r#"{"purpose":"guess"}"#).is_err());
}

#[test]
fn tab_purpose_rest_description_icon_and_unknown_icon_refusal() {
    use crate::tab_routes::CreateShellTabBody;
    let body: CreateShellTabBody = serde_json::from_value(serde_json::json!({
        "purpose": "qa", "icon": "flask-conical", "description": "  Runs tests  "
    }))
    .unwrap();
    let mut spec = SpawnTabSpec {
        shell: None,
        purpose: body.purpose,
        icon: body.icon,
        description: body.description,
        ..Default::default()
    };
    codeterm_service_core::service::prepare_tab_identity_for_test(&mut spec).unwrap();
    assert_eq!(spec.description.as_deref(), Some("Runs tests"));
    assert_eq!(spec.icon.as_deref(), Some("flask-conical"));
    spec.icon = Some("missing-icon".into());
    assert!(
        codeterm_service_core::service::prepare_tab_identity_for_test(&mut spec)
            .unwrap_err()
            .starts_with("Unknown Lucide icon")
    );
    spec.icon = Some("bug".into());
    spec.command = TabCommand::ChatBackend {
        plugin_id: "local".into(),
        config: serde_json::Value::Null,
        role: None,
        role_profile: None,
        can_spawn: None,
        watcher: None,
    };
    assert!(codeterm_service_core::service::prepare_tab_identity_for_test(&mut spec).is_err());
}
