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
    let state = json!({"registry":{"rows":[{"display_name":"Laptop","active":true,"presence":"signed_out"}]}});
    assert!(super::account_mesh_status_lines(&state)
        .contains(&"  Laptop: Signed out (actions disabled)".to_owned()));
}
