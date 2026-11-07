use super::*;

#[test]
fn only_correlated_overlapping_rpc_refusal_before_any_acceptance_settles_uncertain() {
    let error = |code, message| {
        serde_json::from_value::<RpcReply>(serde_json::json!({
            "kind":"error", "error":{"code":code,"message":message}
        }))
        .unwrap()
    };
    let reply = error(-32602, "opaque");
    let code = RuntimeFailureCode::ProviderRefused;
    assert!(eligible(&reply, code, true, true, false, false));
    assert!(eligible(
        &error(-32602, "unrelated wording"),
        code,
        true,
        true,
        false,
        false
    ));
    assert!(!eligible(
        &error(-32000, "opaque"),
        code,
        true,
        true,
        false,
        false
    ));
    assert!(!eligible(
        &RpcReply::Success {
            value: serde_json::json!({})
        },
        code,
        true,
        true,
        false,
        false
    ));
    assert!(!eligible(&reply, code, false, true, false, false));
    assert!(!eligible(&reply, code, true, false, false, false));
    assert!(!eligible(&reply, code, true, true, true, false));
    assert!(!eligible(&reply, code, true, true, false, true));
    assert!(!eligible(
        &reply,
        RuntimeFailureCode::AuthenticationRequired,
        true,
        true,
        false,
        false
    ));
}
