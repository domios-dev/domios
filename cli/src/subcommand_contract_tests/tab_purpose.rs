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
