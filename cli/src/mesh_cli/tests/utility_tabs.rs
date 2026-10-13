use super::*;

#[test]
fn tab_new_purpose_mesh_json_field() {
    let command = crate::Commands::Tab {
        action: crate::TabAction::New {
            timeout: None,
            shell: None,
            list_shells: false,
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

#[test]
fn tab_new_shell_profile_mesh_wire_and_list_read_only_route() {
    use clap::Parser;
    let parsed = crate::Cli::try_parse_from([
        "codeterm",
        "tab",
        "new",
        "--shell",
        "wsl:Ubuntu",
        "--cmd",
        "uname -a",
    ])
    .unwrap();
    let (method, route, body) = cli_command_to_api(&parsed.command);
    assert_eq!((method.as_str(), route.as_str()), ("POST", "/api/tabs"));
    let value: serde_json::Value = serde_json::from_str(body.as_deref().unwrap()).unwrap();
    assert_eq!(value["shell"], "wsl:Ubuntu");
    assert_eq!(value["cmd"], "uname -a");
    let parsed = crate::Cli::try_parse_from(["codeterm", "tab", "new", "--list-shells"]).unwrap();
    let (method, route, body) = cli_command_to_api(&parsed.command);
    assert_eq!((method.as_str(), route.as_str()), ("GET", "/api/shells"));
    assert!(body.is_none());
}

#[test]
fn utility_timeouts_refuse_mesh_without_a_local_owner() {
    use clap::Parser;
    for argv in [
        vec!["codeterm", "tab", "new", "--timeout", "30s"],
        vec![
            "codeterm",
            "tab",
            "timeout",
            "--tab",
            "shell",
            "--timeout",
            "30s",
        ],
    ] {
        let command = crate::Cli::try_parse_from(argv).unwrap().command;
        let (method, reason, body) = cli_command_to_api(&command);
        assert_eq!(method, UNSUPPORTED_OVER_MESH);
        assert!(reason.contains("local owning agent"));
        assert!(body.is_none());
        assert!(matches!(
            crate::transport::transport_for(&command).api_verdict,
            crate::transport::ApiVerdict::Unsupported(_)
        ));
    }
}
