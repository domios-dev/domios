//! The CLI reports the peer's own verdict, names who answered, and classifies
//! refusals by mechanism (codes and status), never by message text.

use super::classify::{classify_relay_answer, refusal_class, RefusalClass};
use super::*;
use codeterm_types::mesh::delivery;

const SEND: (&str, &str) = ("POST", "/api/tabs/e35319d7/send");
const PEER_ID: &str = "dc737738-aaaa-bbbb-cccc-000000000001";

fn answer(
    relayed: (&str, &str),
    status: reqwest::StatusCode,
    body: serde_json::Value,
) -> Result<RelayAnswer, MeshSpawnFailure> {
    classify_relay_answer("typed-name", relayed, status, body.to_string().as_bytes())
}

fn envelope(peer: bool, mut fields: serde_json::Value) -> serde_json::Value {
    if peer {
        fields["peer"] = serde_json::json!({
            "instance_id": PEER_ID,
            "instance_name": "BLACKSTAR-peer [dev-meshpeer]",
        });
    }
    fields
}

fn submit_unconfirmed_envelope() -> serde_json::Value {
    envelope(
        true,
        serde_json::json!({
            "ok": false,
            "status": 502,
            "error": "submit_unconfirmed",
            "data": {
                "ok": false,
                "error": {
                    "code": delivery::SUBMIT_UNCONFIRMED,
                    "message": "submit not confirmed",
                    "bytes_written": true,
                },
            },
        }),
    )
}

#[test]
fn mesh_send_no_payload_echo() {
    let payload = "payload-token-7731";
    let (method, path, body) = crate::mesh_cli::api_map::send_text_to_api(
        &Some(payload.to_string()),
        &Some("tab-1".to_string()),
        &None,
        &None,
        false,
        &None,
        &mut std::io::empty(),
    );
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/tabs/tab-1/send");
    let body: serde_json::Value = serde_json::from_str(&body.expect("body")).unwrap();
    assert_eq!(body["text"], payload);
    assert_eq!(body["enter"], true);
    let mentions = body.to_string().matches(payload).count();
    assert_eq!(mentions, 1, "the payload travels once, in `text` only");
}

#[test]
fn mesh_relay_large_payload_verdict() {
    let landed = answer(SEND, reqwest::StatusCode::OK, submit_unconfirmed_envelope())
        .expect("bytes that reached the PTY are an accepted send, as on the local path");
    assert_eq!(landed.data["ok"], true);
    assert_eq!(landed.data["delivery"], "pending");
    assert_eq!(landed.data["bytes_written"], true);
    assert_eq!(landed.peer.expect("answering peer").instance_id, PEER_ID);

    let unknown = envelope(
        true,
        serde_json::json!({
            "ok": false,
            "status": 504,
            "error": "relay summary",
            "data": {"ok": false, "error": {"code": delivery::RELAY_DELIVERY_DEADLINE_EXCEEDED}},
        }),
    );
    let failure = answer(SEND, reqwest::StatusCode::OK, unknown)
        .expect_err("an unknown outcome is never reported as delivered");
    assert_eq!(failure.bytes_written, None);

    let nothing = envelope(
        true,
        serde_json::json!({
            "ok": true,
            "status": 200,
            "data": {"ok": false, "error": {"code": delivery::TAB_BUSY_REFUSED, "bytes_written": false}},
        }),
    );
    answer(SEND, reqwest::StatusCode::OK, nothing).expect_err("nothing landed is a failure");
}

#[test]
fn mesh_relay_landed_verdict_is_scoped_to_sends() {
    let mut not_a_send = submit_unconfirmed_envelope();
    not_a_send["data"]["ok"] = serde_json::json!(false);
    let failure = answer(("POST", "/api/agents/spawn"), reqwest::StatusCode::OK, not_a_send)
        .expect_err("only a tab send may turn landed bytes into an accepted send");
    assert_eq!(failure.bytes_written, Some(true));
}

#[test]
fn mesh_peer_resolution_table() {
    let ambiguous_body = serde_json::json!({
        "reason": "peer_name_ambiguous",
        "name": "ROG",
        "candidates": [
            {"instance_id": "id-1", "instance_name": "ROG", "online": true, "relay_protocol_version": 1},
            {"instance_id": "id-2", "instance_name": "ROG", "online": true, "relay_protocol_version": 1},
        ],
    });
    let failure = answer(("GET", "/api/tabs"), reqwest::StatusCode::CONFLICT, ambiguous_body)
        .expect_err("an ambiguous name is refused");
    assert!(failure.reason.contains("id-1") && failure.reason.contains("id-2"));
    assert_eq!(failure.class, Some(RefusalClass::Deterministic));
    assert_eq!(failure.to_json()["error"]["class"], "deterministic");

    let table: [(&str, Option<&str>); 2] = [
        ("with a resolved peer the failure names it", Some("BLACKSTAR-peer [dev-meshpeer]")),
        ("without one it keeps the typed target", None),
    ];
    for (case, resolved) in table {
        let body = envelope(
            resolved.is_some(),
            serde_json::json!({
                "ok": false,
                "status": 403,
                "error": "denied",
                "data": {"ok": false, "error": {"code": delivery::PEER_RELAY_NOT_AUTHORIZED, "bytes_written": false}},
            }),
        );
        let failure = answer(SEND, reqwest::StatusCode::OK, body).expect_err(case);
        assert_eq!(failure.peer, resolved.unwrap_or("typed-name"), "{case}");
        assert_eq!(failure.peer_id.as_deref(), resolved.map(|_| PEER_ID), "{case}");
    }

    let ok = answer(
        ("GET", "/api/tabs"),
        reqwest::StatusCode::OK,
        envelope(true, serde_json::json!({"ok": true, "status": 200, "data": [{"id": "t"}]})),
    )
    .expect("success");
    let named = with_answering_peer(ok.data.clone(), ok.peer.as_ref());
    assert!(named.is_array(), "a bare-array result is never wrapped or rewritten");
    let object = with_answering_peer(serde_json::json!({"tabs": []}), ok.peer.as_ref());
    assert_eq!(object["answered_by"]["instance_id"], PEER_ID);
}

#[test]
fn mesh_refusal_classification() {
    let table: [(Option<&str>, u16, RefusalClass); 9] = [
        (Some(delivery::PEER_RELAY_NOT_AUTHORIZED), 403, RefusalClass::Deterministic),
        (Some(delivery::PEER_RELAY_PROTOCOL_MISMATCH), 426, RefusalClass::Deterministic),
        (Some(delivery::TAB_NOT_REACHABLE_ON_PEER), 404, RefusalClass::Deterministic),
        (Some(delivery::TAB_BUSY_REFUSED), 409, RefusalClass::Transient),
        (Some(delivery::MESH_DELIVERY_WINDOW_SATURATED), 503, RefusalClass::Transient),
        (Some("spawn_failed"), 500, RefusalClass::Unknown),
        (None, 400, RefusalClass::Deterministic),
        (None, 503, RefusalClass::Transient),
        (None, 500, RefusalClass::Unknown),
    ];
    for (code, status, expected) in table {
        assert_eq!(refusal_class(code, status), expected, "{code:?} {status}");
    }

    let refused = envelope(
        true,
        serde_json::json!({
            "ok": false,
            "status": 403,
            "error": "denied",
            "data": {"ok": false, "error": {"code": delivery::PEER_RELAY_NOT_AUTHORIZED, "bytes_written": false}},
        }),
    );
    let deterministic = answer(SEND, reqwest::StatusCode::OK, refused).expect_err("refused");
    let busy = envelope(
        true,
        serde_json::json!({
            "ok": false,
            "status": 409,
            "error": "busy",
            "data": {"ok": false, "error": {"code": delivery::TAB_BUSY_REFUSED, "bytes_written": false}},
        }),
    );
    let transient = answer(SEND, reqwest::StatusCode::OK, busy).expect_err("busy");
    assert_ne!(
        deterministic.retry_hint, transient.retry_hint,
        "a refusal that can never clear must not share the safe-retry hint"
    );
    assert_eq!(deterministic.to_json()["error"]["class"], "deterministic");
    assert_eq!(transient.to_json()["error"]["class"], "transient");
}

#[test]
fn mesh_tty_line_limit_is_refused_only_where_the_platform_truncates() {
    use crate::mesh_cli::tty_line_guard::{
        longest_line_bytes, platform_has_tty_line_limit, refusal, LINE_EXCEEDS_TTY_LIMIT,
        TTY_LINE_LIMIT_BYTES,
    };
    let at_limit = "x".repeat(TTY_LINE_LIMIT_BYTES);
    let over = format!("short\r\n{}\n", "y".repeat(TTY_LINE_LIMIT_BYTES + 1));
    assert_eq!(longest_line_bytes(&at_limit), TTY_LINE_LIMIT_BYTES);
    assert_eq!(longest_line_bytes(&over), TTY_LINE_LIMIT_BYTES + 1);
    assert!(platform_has_tty_line_limit("linux"));
    assert!(platform_has_tty_line_limit("macos"));
    assert!(!platform_has_tty_line_limit("windows"));
    let failure = refusal("ubunta", TTY_LINE_LIMIT_BYTES + 1);
    assert_eq!(failure.code.as_deref(), Some(LINE_EXCEEDS_TTY_LIMIT));
    assert_eq!(failure.bytes_written, Some(false));
    assert_eq!(failure.class, Some(RefusalClass::Deterministic));
}
