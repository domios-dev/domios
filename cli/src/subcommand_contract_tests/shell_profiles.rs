use super::*;

#[test]
fn tab_new_shell_and_list_shells_parse_without_changing_defaults() {
    let parsed = TabOnly::try_parse_from([
        "tab",
        "new",
        "--shell",
        "wsl:Ubuntu",
        "--purpose",
        "qa",
        "--cmd",
        "uname -a",
    ])
    .unwrap();
    assert!(
        matches!(parsed.action, TabAction::New { shell: Some(id), list_shells: false, .. } if id == "wsl:Ubuntu")
    );
    let parsed = TabOnly::try_parse_from(["tab", "new", "--list-shells"]).unwrap();
    assert!(matches!(
        parsed.action,
        TabAction::New {
            shell: None,
            list_shells: true,
            ..
        }
    ));
    let parsed = TabOnly::try_parse_from(["tab", "new"]).unwrap();
    assert!(matches!(
        parsed.action,
        TabAction::New {
            shell: None,
            list_shells: false,
            ..
        }
    ));
}

#[test]
fn shell_profiles_refuse_chat_backend_and_list_creation_conflicts() {
    for flags in [
        vec!["--shell", "bash", "--chat-backend", "chat"],
        vec!["--list-shells", "--cmd", "echo x"],
        vec!["--list-shells", "--shell", "bash"],
    ] {
        let mut args = vec!["tab", "new"];
        args.extend(flags);
        assert!(TabOnly::try_parse_from(args).is_err());
    }
}
