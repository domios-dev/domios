use super::*;
use crate::mesh_cli::api_map::UNSUPPORTED_OVER_MESH;

#[test]
fn send_text_is_fire_and_forget_on_mesh() {
    let cmd = Commands::SendText {
        text: Some("ping".into()),
        tab: Some("abc123".into()),
        from_file: None,
        from_pad: None,
        attach: Vec::new(),
        copy: false,
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

#[test]
fn mesh_pad_attachment_refusal_points_to_file_card_transfer() {
    use clap::Parser;

    let command = crate::Cli::try_parse_from([
        "domios",
        "--mesh",
        "peer",
        "send",
        "--tab",
        "recipient",
        "--attach",
        "board.canvas.json",
        "--copy",
    ])
    .unwrap()
    .command;
    let (method, reason, body) = cli_command_to_api(&command);
    assert_eq!(method, UNSUPPORTED_OVER_MESH);
    assert!(reason.contains("file card"));
    assert!(reason.contains("absolute sender pad path"));
    assert!(reason.contains("--from-pad"));
    assert!(body.is_none());
}
