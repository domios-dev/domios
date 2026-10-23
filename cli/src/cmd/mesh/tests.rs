use super::parse_mesh_response;

#[test]
fn mesh_listener_refusal_is_an_error_response() {
    let result = parse_mesh_response(
        reqwest::StatusCode::CONFLICT,
        "mesh_listener_not_remotely_ready",
    );

    let error = result.expect_err("an unavailable mesh listener must fail the CLI command");
    assert!(error.contains("HTTP 409"));
    assert!(error.contains("mesh_listener_not_remotely_ready"));
}

#[test]
fn mesh_link_parses_one_installation_and_maps_to_the_account_route() {
    use clap::Parser;
    let command = crate::Cli::try_parse_from(["codeterm", "mesh", "link", "inst-1"])
        .unwrap()
        .command;
    let (method, path, body) = crate::mesh_cli::api_map::cli_command_to_api(&command);
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/mesh/link");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body.unwrap()).unwrap(),
        serde_json::json!({"installation_id": "inst-1"})
    );
    for removed in [
        "connect",
        "approve",
        "reject",
        "discover",
        "broadcast",
        "stop-broadcast",
    ] {
        assert!(
            crate::Cli::try_parse_from(["codeterm", "mesh", removed, "x"]).is_err(),
            "{removed}"
        );
    }
}
