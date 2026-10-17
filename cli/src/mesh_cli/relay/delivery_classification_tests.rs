//! A relayed send is successful only when the PEER confirms delivery. The relay
//! hop's own `ok` attests acceptance, which is not the same claim.

use super::*;
use codeterm_mesh::contract::delivery;

const SEND: (&str, &str) = ("POST", "/api/tabs/e35319d7/send");

fn classify(
    relayed: (&str, &str),
    body: serde_json::Value,
) -> Result<serde_json::Value, MeshSpawnFailure> {
    classify_relay_response(
        "mac-air",
        relayed,
        reqwest::StatusCode::OK,
        body.to_string().as_bytes(),
    )
}

/// The relay accepted the send and answered 200; the peer's own body refused it.
/// An accepting envelope must never outrank the peer's delivery verdict.
fn accepted_envelope_with_peer_refusal(
    code: &str,
    bytes_written: Option<bool>,
) -> serde_json::Value {
    let mut error = serde_json::json!({ "code": code, "message": "peer delivery outcome" });
    if let Some(written) = bytes_written {
        error["bytes_written"] = serde_json::json!(written);
    }
    serde_json::json!({
        "ok": true,
        "status": 200,
        "error": serde_json::Value::Null,
        "data": { "ok": false, "error": error },
    })
}

#[test]
fn acceptance_without_peer_confirmation_is_not_a_successful_send() {
    let failure = classify(
        SEND,
        accepted_envelope_with_peer_refusal(delivery::TAB_BUSY_REFUSED, Some(false)),
    )
    .expect_err("an accepted-but-unconfirmed send must never classify as success");
    assert_eq!(failure.code.as_deref(), Some(delivery::TAB_BUSY_REFUSED));
    assert_eq!(failure.bytes_written, Some(false));
}

#[test]
fn every_peer_delivery_code_survives_the_relay_boundary_as_a_constant() {
    for code in [
        delivery::TAB_NOT_REACHABLE_ON_PEER,
        delivery::TAB_BUSY_REFUSED,
        delivery::PTY_UNAVAILABLE,
        delivery::SUBMIT_UNCONFIRMED,
    ] {
        let envelope = serde_json::json!({
            "ok": false,
            "status": 502,
            "error": "forwarded summary",
            "data": {
                "ok": false,
                "error": {
                    "code": code,
                    "message": "peer delivery outcome",
                    "bytes_written": false,
                },
            },
        });
        let failure = classify(SEND, envelope).expect_err("ok:false is always a failure");
        assert_eq!(failure.code.as_deref(), Some(code));
        assert_eq!(
            failure.to_json()["error"]["code"].as_str(),
            Some(code),
            "the machine-readable surface carries the code, not only the reason text"
        );
    }
}

#[test]
fn nothing_landed_and_landed_unconfirmed_stay_distinguishable() {
    let nothing = classify(
        SEND,
        accepted_envelope_with_peer_refusal(delivery::TAB_NOT_REACHABLE_ON_PEER, Some(false)),
    )
    .expect_err("a refused send fails");
    assert_eq!(nothing.bytes_written, Some(false));
    assert_eq!(
        nothing.to_json()["error"]["bytes_written"].as_bool(),
        Some(false)
    );

    let landed = classify(
        SEND,
        accepted_envelope_with_peer_refusal(delivery::SUBMIT_UNCONFIRMED, Some(true)),
    )
    .expect_err("submit_unconfirmed exits nonzero, as the local send does since #578");
    assert_eq!(landed.code.as_deref(), Some(delivery::SUBMIT_UNCONFIRMED));
    assert_eq!(landed.bytes_written, Some(true));
    assert_eq!(
        landed.to_json()["error"]["bytes_written"].as_bool(),
        Some(true),
        "landed bytes stay visible so no caller replays the send"
    );
}

/// Symptom B's shape: the relay abandoned its own loopback call, so nobody knows
/// whether the payload landed. Absent `bytes_written` must stay absent — reading
/// it as `false` would advertise a retry that can double-deliver.
#[test]
fn an_abandoned_relay_deadline_reports_unknown_never_nothing_landed() {
    let envelope = serde_json::json!({
        "ok": false,
        "status": 504,
        "error": "relay summary",
        "data": {
            "ok": false,
            "error": {
                "code": delivery::RELAY_DELIVERY_DEADLINE_EXCEEDED,
                "message": "local delivery route did not answer within the relay deadline",
            },
        },
    });
    let failure = classify(SEND, envelope).expect_err("an abandoned relay is a failure");
    assert_eq!(
        failure.code.as_deref(),
        Some(delivery::RELAY_DELIVERY_DEADLINE_EXCEEDED)
    );
    assert_eq!(failure.bytes_written, None);
    assert!(
        failure.to_json()["error"].get("bytes_written").is_none(),
        "an unknown outcome must not be serialized as a zero-byte one"
    );
}

#[test]
fn a_domain_level_ok_false_on_a_non_send_route_is_a_refusal() {
    let envelope = serde_json::json!({
        "ok": true,
        "status": 200,
        "error": serde_json::Value::Null,
        "data": { "ok": false, "claimed_lock": false },
    });
    classify(("POST", "/api/bridge/migrate"), envelope)
        .expect_err("a refused remote operation cannot succeed");
}

#[test]
fn remote_refusals_preserve_the_reason_across_routes() {
    let reason = "refusal-token-7731";
    for path in ["/api/tabs", "/api/agents/spawn", "/api/update/install"] {
        for error in [
            serde_json::json!(reason),
            serde_json::json!({"code": "refused", "message": reason}),
        ] {
            let failure = classify(
                ("POST", path),
                serde_json::json!({
                    "ok": true, "status": 200, "data": {"ok": false, "error": error}
                }),
            )
            .expect_err("peer refusal must fail the CLI");
            assert!(failure.reason.contains(reason));
        }
    }
}

#[test]
fn remote_rpc_error_is_not_success_data() {
    let failure = classify(
        ("POST", "/api/tabs"),
        serde_json::json!({
            "ok": true, "status": 200,
            "data": {"error": {"code": -32000, "message": "rpc-refusal-token"}}
        }),
    )
    .expect_err("an RPC error must fail even without ok:false");
    assert!(failure.reason.contains("rpc-refusal-token"));
}

#[test]
fn server_hint_wins_over_route_and_capability_fallbacks() {
    for path in [
        "/api/tabs",
        "/api/tabs/tab-1/send",
        "/api/tabs/tab-1/delayed-messages",
    ] {
        for key in ["retry_hint", "hint"] {
            let reason = "server-reason-token";
            let hint = "server-hint-token";
            let mut error = serde_json::json!({
                "code": "peer_lacks_delayed_recurrence", "message": reason
            });
            error[key] = serde_json::json!(hint);
            let failure = classify(
                ("POST", path),
                serde_json::json!({
                    "ok": true, "status": 200, "data": {"ok": false, "error": error}
                }),
            )
            .expect_err("refusal remains a failure");
            assert!(failure.reason.contains(reason));
            assert_eq!(failure.retry_hint, hint);
        }
    }
}

#[test]
fn refused_remote_result_exits_nonzero_with_reason() {
    const CHILD: &str = "CODETERM_TEST_MESH_REFUSAL_CHILD";
    const REASON: &str = "remote-refusal-token-8274";
    if let Ok(mode) = std::env::var(CHILD) {
        let failure = classify(
            ("POST", "/api/tabs"),
            serde_json::json!({
                "ok": true, "status": 200,
                "data": {"ok": false, "error": {"message": REASON}}
            }),
        )
        .expect_err("refused remote result");
        failure.print_and_exit(mode == "json");
    }
    for mode in ["human", "json"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "mesh_cli::relay::delivery_classification_tests::refused_remote_result_exits_nonzero_with_reason", "--nocapture"])
            .env(CHILD, mode)
            .output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        let stream = if mode == "json" {
            output.stdout
        } else {
            output.stderr
        };
        assert!(String::from_utf8(stream).unwrap().contains(REASON));
    }
}

#[test]
fn an_unknown_outcome_is_never_hinted_as_a_free_retry() {
    let unknown = classify(
        SEND,
        accepted_envelope_with_peer_refusal(delivery::RELAY_DELIVERY_DEADLINE_EXCEEDED, None),
    )
    .expect_err("an outcome nobody knows fails");
    let safe = classify(
        SEND,
        accepted_envelope_with_peer_refusal(delivery::TAB_BUSY_REFUSED, Some(false)),
    )
    .expect_err("a refused send fails");
    assert_ne!(
        unknown.retry_hint, safe.retry_hint,
        "a payload that may already have landed cannot share the safe-retry hint"
    );
}

#[test]
fn offline_peer_list_refusal_is_classified_as_peer_offline() {
    let body = serde_json::json!({
        "reason": "peer_name_no_online_record",
        "name": "peer-a",
        "candidates": [],
    });
    let failure = classify_relay_response(
        "peer-a",
        ("GET", "/api/tabs"),
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        body.to_string().as_bytes(),
    )
    .expect_err("an offline target must not become an empty tab list");

    assert!(failure.reason.starts_with("peer_offline:"));
    assert_eq!(failure.retry_hint, "bring the peer online, then retry");
}

#[test]
fn remote_get_text_without_pty_has_an_accurate_retry_hint() {
    let envelope = serde_json::json!({
        "ok": false,
        "status": 404,
        "error": "Tab not found or has no PTY",
        "data": null,
    });
    let failure = classify_relay_response(
        "peer-a",
        ("GET", "/api/tabs/remote-tab/text?formatted=false"),
        reqwest::StatusCode::OK,
        envelope.to_string().as_bytes(),
    )
    .expect_err("a tab without a PTY cannot provide terminal text");

    assert_eq!(
        failure.retry_hint,
        "the tab has no PTY; choose a PTY-backed tab"
    );
}

#[test]
fn a_json_http_refusal_is_not_misread_as_null_success() {
    let failure = classify_relay_response(
        "peer-a",
        ("GET", "/api/tabs"),
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        br#"{"error":"peer unavailable"}"#,
    )
    .expect_err("a non-success HTTP response must remain a CLI failure");

    assert!(failure.reason.contains("peer unavailable"));
}

#[test]
fn mesh_send_every_refuses_peer_without_recurrence_capability() {
    let refusal = serde_json::json!({
        "ok": false,
        "status": 426,
        "error": "peer_lacks_delayed_recurrence: Peer 'old-peer' (version 1.11.9) does not advertise delayed_recurrence; update the peer or send a one-shot explicitly",
        "data": {
            "ok": false,
            "error": {
                "code": "peer_lacks_delayed_recurrence",
                "message": "Peer 'old-peer' (version 1.11.9) does not advertise delayed_recurrence; update the peer or send a one-shot explicitly",
                "bytes_written": false
            }
        }
    });
    let failure = classify_relay_response(
        "old-peer",
        ("POST", "/api/tabs/tab-1/delayed-messages"),
        reqwest::StatusCode::OK,
        refusal.to_string().as_bytes(),
    )
    .expect_err("capability refusal must fail the CLI request");

    assert!(failure.reason.contains("old-peer"));
    assert!(failure.reason.contains("1.11.9"));
    assert!(!failure.bytes_written.unwrap_or(true));
    assert_eq!(
        failure.retry_hint,
        "update the peer or send a one-shot explicitly"
    );
}
