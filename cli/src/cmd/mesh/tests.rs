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
fn mesh_pairing_verbs_are_removed() {
    use clap::Parser;
    for removed in [
        "link",
        "disconnect",
        "unrevoke",
        "connect",
        "approve",
        "reject",
        "discover",
        "broadcast",
        "stop-broadcast",
    ] {
        assert!(
            crate::Cli::try_parse_from(["domios", "mesh", removed, "installation"]).is_err(),
            "{removed}"
        );
    }
}

#[test]
fn mesh_status_exposes_publication_refusal_and_retry_deadline() {
    let state = serde_json::json!({
        "publication": "published",
        "publication_failure": {"reason": "rate-limited", "retry_after_seconds": 7},
        "retry_at": 107
    });
    let lines = super::account_mesh_status_lines(&state).join("\n");
    assert!(lines.contains("published"));
    assert!(lines.contains("rate-limited"));
    assert!(lines.contains("107"));
    assert!(lines.contains("waiting to retry"));
}
#[test]
fn mesh_status_explains_required_reauthentication() {
    let lines = super::account_mesh_status_lines(&serde_json::json!({
        "enrollment": "refused",
        "last_refusal": { "reason": "reauth_required" },
        "retry_at": null
    }));
    assert_eq!(lines, ["Sign in again to add this machine to your mesh"]);
}

#[test]
fn mesh_status_says_plainly_when_the_build_cannot_join() {
    let lines = super::account_mesh_status_lines(&serde_json::json!({
        "publication": "none",
        "last_refusal": { "reason": "proof_invalid" },
    }));
    assert!(lines[0].starts_with("This build cannot join this account's mesh"));
    assert!(lines.iter().any(|line| line.contains("proof_invalid")));
}

#[test]
fn mesh_account_presence_is_distinct_from_offline_and_expired() {
    use serde_json::json;
    let mut row =
        json!({"active":true,"presence":"signed_out","membership_expires_at":1,"endpoint":null});
    assert_eq!(
        super::account_machine_status(&row, 10),
        "Signed out (actions disabled)"
    );
    row["presence"] = json!("present");
    assert_eq!(
        super::account_machine_status(&row, 10),
        "Membership expired"
    );
    row["membership_expires_at"] = json!(20);
    assert_eq!(super::account_machine_status(&row, 10), "Offline");
    row["endpoint"] = json!({"expires_at":20});
    assert_eq!(super::account_machine_status(&row, 10), "Present");
    row["active"] = json!(false);
    assert_eq!(super::account_machine_status(&row, 10), "Removed");
    let state = json!({"registry":{"rows":[
        {"display_name":"Laptop","active":true,"presence":"signed_out"},
        {"display_name":"Old","active":false,"eligibility":"revoked","presence":"present"}
    ]}});
    let status_lines = super::account_mesh_status_lines(&state);
    assert!(status_lines.contains(&"  Laptop: Signed out (actions disabled)".to_owned()));
    assert!(!status_lines.iter().any(|line| line.contains("Old")));
}

#[test]
fn mesh_remove_parses_machine_target_and_blocks_mesh_relay() {
    use clap::Parser;
    let parsed = crate::Cli::try_parse_from(["domios", "mesh", "remove", "BLACKSTAR #4"])
        .expect("mesh remove parses");
    match &parsed.command {
        crate::Commands::Mesh {
            action: crate::MeshAction::Remove { machine },
        } => assert_eq!(machine, "BLACKSTAR #4"),
        _ => panic!("expected MeshAction::Remove"),
    }
    let (method, _, _) = crate::mesh_cli::api_map::cli_command_to_api(&parsed.command);
    assert_eq!(method, crate::mesh_cli::api_map::UNSUPPORTED_OVER_MESH);
}

#[test]
fn mesh_identity_set_accepts_kind_only_and_rejects_color() {
    use clap::Parser;
    let parsed =
        crate::Cli::try_parse_from(["domios", "mesh", "identity", "set", "--kind", "laptop"])
            .expect("mesh identity set --kind parses");
    let (method, path, body) = crate::mesh_cli::api_map::cli_command_to_api(&parsed.command);
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/mesh/identity");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(body.as_deref().unwrap()).unwrap(),
        serde_json::json!({ "kind": "laptop", "color": "graphite" })
    );

    crate::Cli::try_parse_from([
        "domios", "mesh", "identity", "set", "--kind", "server", "--peer", "ubunta",
    ])
    .expect("mesh identity set --peer parses");

    assert!(crate::Cli::try_parse_from([
        "domios", "mesh", "identity", "set", "--kind", "laptop", "--color", "teal",
    ])
    .is_err());
}

#[test]
fn mesh_status_agrees_with_live_connection_when_cloud_endpoint_is_stale() {
    use serde_json::json;
    let state = json!({
        "local_installation_id": "inst-local",
        "publication": "published",
        "membership_expires_at": u64::MAX,
        "pairs": [
            {
                "instance_id": "66c926f1-peer",
                "installation_id": "7a835b57-r161",
                "online": true,
                "name": "acceptance-r161"
            }
        ],
        "registry": {
            "rows": [
                {
                    "installation_id": "inst-local",
                    "display_name": "w1221",
                    "active": true,
                    "presence": "present",
                    "membership_expires_at": 1,
                    "endpoint": null
                },
                {
                    "installation_id": "7a835b57-r161",
                    "display_name": "acceptance-r161",
                    "active": true,
                    "presence": "present",
                    "membership_expires_at": u64::MAX,
                    "endpoint": { "expires_at": 1 }
                },
                {
                    "installation_id": "inst-offline",
                    "display_name": "dormant",
                    "active": true,
                    "presence": "present",
                    "membership_expires_at": u64::MAX,
                    "endpoint": null
                }
            ]
        }
    });
    let lines = super::account_mesh_status_lines(&state);
    assert!(
        lines.contains(&"  acceptance-r161: Connected".to_owned()),
        "live connected peer must not appear Offline: {lines:?}"
    );
    assert!(
        lines.contains(&"  w1221: This machine".to_owned()),
        "published local machine must not appear Offline: {lines:?}"
    );
    assert!(
        lines.contains(&"  dormant: Offline".to_owned()),
        "unconnected machine without fresh endpoint remains Offline: {lines:?}"
    );
}
