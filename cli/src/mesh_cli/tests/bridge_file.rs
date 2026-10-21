use super::*;

#[test]
fn bridge_file_send_over_mesh_refuses_instead_of_dropping_attachment() {
    let (method, reason, body) = cli_command_to_api(&Commands::Bridge {
        action: BridgeAction::Send {
            text: Some("body".into()),
            from_pad: None,
            raw_text: false,
            service: "telegram".into(),
            to: 42,
            append: false,
            r#type: "text".into(),
            file: Some("report.md".into()),
            caption: Some("caption".into()),
            filename: Some("report.md".into()),
            buttons: None,
        },
    });
    assert_eq!(method, crate::mesh_cli::api_map::UNSUPPORTED_OVER_MESH);
    assert!(reason.contains("file uploads over mesh are unsupported"));
    assert_eq!(body, None);
}
