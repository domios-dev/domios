use super::*;

#[test]
fn tab_new_purpose_mesh_json_field() {
    let command = crate::Commands::Tab {
        action: crate::TabAction::New {
            purpose: "build".into(),
            description: Some("Runs focused tests".into()),
            icon: Some("flask-conical".into()),
            workspace: None,
            cwd: None,
            title: None,
            team: None,
            no_team: false,
            cmd: None,
            eager: false,
            chat_backend: None,
            config: None,
        },
    };
    let (method, route, body) = cli_command_to_api(&command);
    assert_eq!(method, "POST");
    assert_eq!(route, "/api/tabs");
    let value: serde_json::Value = serde_json::from_str(body.as_deref().unwrap()).unwrap();
    assert_eq!(value["purpose"], "build");
    assert_eq!(value["description"], "Runs focused tests");
    assert_eq!(value["icon"], "flask-conical");
}
