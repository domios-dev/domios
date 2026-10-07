use super::*;

#[test]
fn tab_new_purpose_defaults_and_validates_values() {
    let parsed = TabOnly::try_parse_from(["tab", "new"]).unwrap();
    assert!(matches!(parsed.action, TabAction::New { purpose, .. } if purpose == "shell"));
    for value in ["build", "dev", "qa", "shell"] {
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
