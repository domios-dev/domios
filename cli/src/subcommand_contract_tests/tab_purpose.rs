use super::*;

#[test]
fn tab_new_purpose_defaults_and_validates_values() {
    let parsed = TabOnly::try_parse_from(["tab", "new"]).unwrap();
    assert!(matches!(parsed.action, TabAction::New { purpose, .. } if purpose == "shell"));
    for value in ["build", "dev", "qa", "deploy", "logs", "db", "shell"] {
        let parsed = TabOnly::try_parse_from(["tab", "new", "--purpose", value]).unwrap();
        assert!(matches!(parsed.action, TabAction::New { purpose, .. } if purpose == value));
    }
    assert!(TabOnly::try_parse_from(["tab", "new", "--purpose", "guess"]).is_err());
    assert!(TabOnly::try_parse_from([
        "tab",
        "new",
        "--purpose",
        "build",
        "--chat-backend",
        "local"
    ])
    .is_err());
}

#[test]
fn tab_new_description_and_icon_parse() {
    let parsed = TabOnly::try_parse_from([
        "tab",
        "new",
        "--purpose",
        "qa",
        "--description",
        "Runs focused tests",
        "--icon",
        "flask-conical",
    ])
    .unwrap();
    match parsed.action {
        TabAction::New {
            description, icon, ..
        } => {
            assert_eq!(description.as_deref(), Some("Runs focused tests"));
            assert_eq!(icon.as_deref(), Some("flask-conical"));
        }
        _ => panic!("expected tab new"),
    }
    assert!(
        TabOnly::try_parse_from(["tab", "new", "--icon", "bug", "--chat-backend", "local"])
            .is_err()
    );
}

#[test]
fn utility_shell_timeout_cli_contract() {
    let parsed = TabOnly::try_parse_from(["tab", "new", "--timeout", "30s"]).unwrap();
    assert!(matches!(parsed.action, TabAction::New { timeout: Some(value), .. } if value == "30s"));
    for value in ["20m", "none"] {
        let parsed =
            TabOnly::try_parse_from(["tab", "timeout", "--tab", "shell", "--timeout", value])
                .unwrap();
        assert!(
            matches!(parsed.action, TabAction::Timeout { tab: Some(id), timeout } if id == "shell" && timeout == value)
        );
    }
    assert!(TabOnly::try_parse_from([
        "tab",
        "new",
        "--chat-backend",
        "plugin",
        "--timeout",
        "30s"
    ])
    .is_err());
}

#[test]
fn tab_new_timeout_help_names_the_owning_agent_requirement() {
    let mut command = TabOnly::command();
    let help = command
        .find_subcommand_mut("new")
        .unwrap()
        .render_long_help()
        .to_string();
    assert!(help.contains("CODETERM_TAB_ID"));
    assert!(help.contains("omit for human shells"));
}
