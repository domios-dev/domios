use super::*;
use serde_json::json;

#[test]
fn codex_rpc_framing_without_jsonrpc_and_unknown_fields() {
    assert_eq!(
        decode(r#"{"id":0,"result":null,"future":true}"#).unwrap(),
        Frame::Response {
            id: RequestId::Number(0),
            result: Ok(Value::Null)
        }
    );
    assert!(matches!(
        decode(r#"{"jsonrpc":"2.0","method":"event"}"#).unwrap(),
        Frame::Notification { .. }
    ));
}

#[test]
fn codex_rpc_server_request_id_does_not_become_response() {
    assert!(matches!(
        decode(r#"{"method":"approval","id":0,"params":{}}"#).unwrap(),
        Frame::ServerRequest {
            id: RequestId::Number(0),
            ..
        }
    ));
    assert!(matches!(
        decode(r#"{"id":"0","result":{}}"#).unwrap(),
        Frame::Response {
            id: RequestId::String(_),
            ..
        }
    ));
}

#[test]
fn codex_rpc_rejects_ambiguous_or_malformed_responses() {
    for frame in [
        r#"[]"#,
        r#"{}"#,
        r#"{"id":true,"result":null}"#,
        r#"{"id":0}"#,
        r#"{"id":0,"result":null,"error":{}}"#,
        r#"{"method":0}"#,
    ] {
        assert!(decode(frame).is_err(), "{frame}");
    }
}

#[test]
fn codex_rpc_missing_method_is_unavailable() {
    let Frame::Response {
        result: Err(error), ..
    } = decode(r#"{"id":1,"error":{"code":-32601,"message":"missing","future":true}}"#).unwrap()
    else {
        panic!()
    };
    assert!(error.unavailable());
}

#[test]
fn codex_rpc_initialize_declares_experimental_and_opt_out() {
    let params = initialize_params();
    assert_eq!(
        params["capabilities"],
        json!({"experimentalApi":true,"optOutNotificationMethods":["turn/diff/updated"]})
    );
}
