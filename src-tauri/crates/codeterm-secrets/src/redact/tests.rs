use super::*;
use serde_json::json;

const SENTINEL: &str = "SENTINEL-VALUE-7f3a";

#[test]
fn a_backend_payload_never_survives_redaction() {
    let payload = json!([{ "id": "i1", "login": { "password": SENTINEL } }]).to_string();
    let message = SecretError::Backend { message: payload }
        .redacted()
        .to_string();
    assert!(!message.contains(SENTINEL));
    assert_eq!(message, format!("secret store backend: {WITHHELD}"));
}

#[test]
fn an_oversized_or_multiline_reason_is_withheld() {
    assert_eq!(bounded_reason(&SENTINEL.repeat(20)), WITHHELD);
    assert_eq!(bounded_reason(&format!("bw failed\n{SENTINEL}")), WITHHELD);
    assert_eq!(bounded_reason("bw is not on PATH"), "bw is not on PATH");
}

#[test]
fn a_metadata_projection_drops_every_value_bearing_field() {
    let records = json!([{
        "item_id": "i1",
        "name": "token",
        "folder": "f",
        "last_modified": 7,
        "value": SENTINEL,
        "notes": SENTINEL,
        "login": { "password": SENTINEL },
        "fields": [{ "value": SENTINEL }],
    }]);
    let items = metadata_list(records).expect("list");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "token");
    assert_eq!(items[0].last_modified, Some(7));
    assert!(!serde_json::to_string(&items).unwrap().contains(SENTINEL));
}

#[test]
fn a_non_list_shape_is_refused_without_echoing_it() {
    let err = metadata_list(json!({ "value": SENTINEL })).unwrap_err();
    assert!(!err.to_string().contains(SENTINEL));
}

pub(crate) fn nested_bw_list() -> serde_json::Value {
    json!({ "success": true, "data": { "object": "list", "data": [{
        "id": "i1",
        "name": "token",
        "revisionDate": "2026-01-01T00:00:00.000Z",
        "login": { "username": "u", "password": SENTINEL },
        "notes": SENTINEL,
        "fields": [{ "value": SENTINEL }],
    }]}})
}

#[test]
fn a_nested_vault_list_envelope_projects_onto_metadata() {
    let items = metadata_list(nested_bw_list()).expect("nested list decodes");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_id, "i1");
    assert_eq!(items[0].name, "token");
    assert!(!format!("{items:?}").contains(SENTINEL));
}
