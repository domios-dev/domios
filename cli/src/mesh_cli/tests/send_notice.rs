use super::*;

#[test]
fn send_text_is_fire_and_forget_on_mesh() {
    let cmd = Commands::SendText {
        text: Some("ping".into()),
        tab: Some("abc123".into()),
        from_file: None,
        from_pad: None,
        stdin: false,
        actions: None,
        delay: None,
        at: None,
        every: None,
        until: None,
        count: None,
        require_agent: false,
        to_shell: false,
        system_context: Some("build".into()),
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/tabs/abc123/send");
    let body: serde_json::Value = serde_json::from_str(&b.expect("body")).unwrap();
    assert_eq!(body["text"], "ping");
    assert_eq!(body["system_context"], "build");
    assert!(
        body.get("verify").is_none(),
        "send must never carry verify — the flag has been removed (#101)"
    );
}
