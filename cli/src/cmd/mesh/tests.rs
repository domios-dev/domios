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
fn mesh_connect_addr_parse_and_wire_body() {
    use clap::Parser;
    for address in ["100.84.159.37:7686", "[fd7a:115c:a1e0::1]:7686"] {
        let command = crate::Cli::try_parse_from([
            "codeterm", "mesh", "connect", "2895951d", "--addr", address,
        ])
        .unwrap()
        .command;
        let crate::Commands::Mesh {
            action:
                crate::mesh_cli::MeshAction::Connect {
                    ref instance_id,
                    ref addr,
                },
        } = command
        else {
            panic!("mesh connect must parse");
        };
        let body = super::connect_body(instance_id, addr.as_ref());
        assert_eq!(body["instance_id"], "2895951d");
        assert_eq!(body["addr"], address);
        let (method, path, relay_body) = crate::mesh_cli::api_map::cli_command_to_api(&command);
        assert_eq!(method, "POST");
        assert_eq!(path, "/api/mesh/connect");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&relay_body.unwrap()).unwrap(),
            body
        );
    }
    let command = crate::Cli::try_parse_from(["codeterm", "mesh", "connect", "peer-name"])
        .unwrap()
        .command;
    let crate::Commands::Mesh {
        action: crate::mesh_cli::MeshAction::Connect { instance_id, addr },
    } = command
    else {
        panic!("mesh connect must parse");
    };
    assert!(addr.is_none());
    assert_eq!(
        super::connect_body(&instance_id, None),
        serde_json::json!({"instance_id": "peer-name"})
    );
    assert!(crate::Cli::try_parse_from([
        "codeterm",
        "mesh",
        "connect",
        "peer",
        "--addr",
        "host:7686"
    ])
    .is_err());
    assert!(crate::Cli::try_parse_from(["codeterm", "mesh", "connect", "peer", "--addr"]).is_err());
}
