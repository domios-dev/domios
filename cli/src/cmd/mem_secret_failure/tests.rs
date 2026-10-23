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
