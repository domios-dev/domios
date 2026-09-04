//! A relayed send is successful only when the PEER confirms delivery. The relay
//! hop's own `ok` attests acceptance, which is not the same claim.

use super::*;
use codeterm_types::mesh::delivery;

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
    .expect_err("an unconfirmed submit fails");
    assert_eq!(landed.bytes_written, Some(true));
    assert_eq!(
        landed.to_json()["error"]["bytes_written"].as_bool(),
        Some(true)
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

/// Only a send's success is decided by the peer's body. Other relayed routes
/// legitimately answer 200 with a domain-level `ok: false` (bridge migrate,
/// hook posts) and must still be handed back as data.
#[test]
fn a_domain_level_ok_false_on_a_non_send_route_is_still_data() {
    let envelope = serde_json::json!({
        "ok": true,
        "status": 200,
        "error": serde_json::Value::Null,
        "data": { "ok": false, "claimed_lock": false },
    });
    let data = classify(("POST", "/api/bridge/migrate"), envelope)
        .expect("a non-send route keeps its own ok semantics");
    assert_eq!(data["claimed_lock"].as_bool(), Some(false));
}

#[test]
fn an_unconfirmed_outcome_is_never_hinted_as_a_free_retry() {
    let landed = classify(
        SEND,
        accepted_envelope_with_peer_refusal(delivery::SUBMIT_UNCONFIRMED, Some(true)),
    )
    .expect_err("an unconfirmed submit fails");
    let safe = classify(
        SEND,
        accepted_envelope_with_peer_refusal(delivery::TAB_NOT_REACHABLE_ON_PEER, Some(false)),
    )
    .expect_err("a refused send fails");
    assert_ne!(
        landed.retry_hint, safe.retry_hint,
        "a payload that may already have landed cannot share the safe-retry hint"
    );
}
