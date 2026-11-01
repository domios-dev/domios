use super::*;
use serde_json::json;

const SENTINEL: &str = "SENTINEL-VALUE-7f3a";

fn nested_bw_list() -> String {
    json!({ "success": true, "data": { "object": "list", "data": [{
        "id": "i1",
        "name": "token",
        "revisionDate": "2026-01-01T00:00:00.000Z",
        "login": { "username": "u", "password": SENTINEL },
        "notes": SENTINEL,
        "fields": [{ "value": SENTINEL }],
    }]}})
    .to_string()
}

#[test]
fn a_daemon_echoing_a_vault_payload_yields_a_typed_exit_without_it() {
    let echoed = nested_bw_list();
    let bodies = [
        json!({ "error": { "kind": "backend", "message": echoed } }),
        json!({ "error": { "kind": { "kind": "backend", "message": echoed }, "message": echoed } }),
    ];
    for body in bodies {
        let (code, err) = secret_failure(503, &body).expect("failure");
        assert_eq!(code, 6);
        assert!(!format!("{} {:?}", err.message, err.data).contains(SENTINEL));
    }
}

#[test]
fn a_short_reason_and_known_kinds_survive() {
    let body =
        json!({ "error": { "kind": "backend", "message": "Bitwarden server is unreachable" } });
    assert_eq!(
        secret_failure(503, &body).unwrap().1.message,
        "Bitwarden server is unreachable"
    );
    let locked = json!({ "error": { "kind": "locked" } });
    assert_eq!(secret_failure(423, &locked).unwrap().0, 3);
    assert!(secret_failure(200, &json!({})).is_none());
}

#[test]
fn local_api_neighbor_secret_refusal_keeps_reason_and_message_without_vault_data() {
    let message = "neighbor_capability_denied: This concierge capability is not allowed for the Neighbor. Politely refuse the current request through the Neighbor channel with --kind refusal and --in-reply-to set to its message_id.";
    let rpc = crate::client::SocketCallError::Rpc(crate::client::RpcError {
        code: -32003,
        message: message.into(),
        data: Some(
            json!({"reason":"neighbor_capability_denied","value":SENTINEL,"foreign_tab":{"name":SENTINEL}}),
        ),
    });
    let api_error = crate::local_api::LocalApiError::from_rpc(rpc);
    let (status, body) = transport_failure(&api_error);
    assert_eq!(status, 403);
    let (exit, error) = secret_failure(status, &body).unwrap();
    assert_eq!(exit, 1);
    assert_eq!(error.code, Some(-32003));
    assert_eq!(error.message, message);
    for json_mode in [false, true] {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        crate::cmd::print::write_cli_error(json_mode, &error, &mut out, &mut err);
        let rendered = format!(
            "{}{}",
            String::from_utf8(out.clone()).unwrap(),
            String::from_utf8(err).unwrap()
        );
        assert!(rendered.contains("neighbor_capability_denied"));
        assert!(!rendered.contains(SENTINEL));
        if json_mode {
            let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
            assert_eq!(value["error"]["code"], "refused");
            assert_eq!(value["error"]["reason"], "neighbor_capability_denied");
            assert_eq!(value["error"]["message"], message);
        }
    }
}

#[test]
fn neighbor_secret_refusals_withhold_structured_payloads_and_transport_outages_stay_unavailable() {
    for (rpc_code, message, expected_exit, expected_code) in [
        (
            Some(-32003),
            format!("neighbor_capability_denied: {}", nested_bw_list()),
            1,
            "refused",
        ),
        (None, nested_bw_list(), 6, "unavailable"),
    ] {
        let error = match rpc_code {
            Some(code) => crate::client::SocketCallError::Rpc(crate::client::RpcError {
                code,
                message,
                data: None,
            }),
            None => crate::client::SocketCallError::Transport(message),
        };
        let (status, body) = transport_failure(&crate::local_api::LocalApiError::from_rpc(error));
        let (exit, error) = secret_failure(status, &body).unwrap();
        assert_eq!(exit, expected_exit);
        let rendered = crate::cmd::error_contract::error_envelope(
            &error.message,
            error.code,
            error.data.as_ref(),
        );
        assert_eq!(rendered["error"]["code"], expected_code);
        assert!(!rendered.to_string().contains(SENTINEL));
    }
}
