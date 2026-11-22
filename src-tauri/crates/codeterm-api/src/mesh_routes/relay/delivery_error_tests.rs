use super::*;

#[test]
fn relay_refusal_preserves_text_and_json_reasons() {
    for body in [
        "refusal-token-403",
        r#""refusal-token-403""#,
        r#"{"message":"refusal-token-403"}"#,
        r#"{"error":"refusal-token-403"}"#,
        r#"{"code":"refused","message":"refusal-token-403"}"#,
        r#"{"error":{"code":"refused","message":"refusal-token-403"}}"#,
    ] {
        let data = relay_response_data(body.as_bytes());
        assert!(relay_error_summary(403, &data).contains("refusal-token-403"));
    }
    for body in [b"".as_slice(), b"  ", b"null", b"{}"] {
        assert_eq!(
            relay_error_summary(403, &relay_response_data(body)),
            "HTTP 403"
        );
    }
    let success = serde_json::json!({"status": "updating"});
    assert_eq!(relay_response_data(success.to_string().as_bytes()), success);
}

/// The receiving hop's classification must still lead with its delivery code,
/// independent of the sender hop's transport classification.
#[test]
fn receiving_classification_still_leads_with_the_delivery_code() {
    use codeterm_mesh::contract::delivery;

    let submit_unconfirmed =
        codeterm_service_core::service::delivery_error::DeliveryError::SubmitUnconfirmed {
            tab: "53854ac8".into(),
            detail: "x".into(),
        }
        .to_json();
    assert!(relay_error_summary(502, &submit_unconfirmed).starts_with(delivery::SUBMIT_UNCONFIRMED));

    let tab_busy = codeterm_service_core::service::delivery_error::DeliveryError::TabBusyRefused {
        tab: "53854ac8".into(),
        detail: "x".into(),
    }
    .to_json();
    assert!(relay_error_summary(409, &tab_busy).starts_with(delivery::TAB_BUSY_REFUSED));

    let tab_unreachable =
        codeterm_service_core::service::delivery_error::DeliveryError::TabNotReachableOnPeer {
            tab: "ghost".into(),
        }
        .to_json();
    assert!(
        relay_error_summary(404, &tab_unreachable).starts_with(delivery::TAB_NOT_REACHABLE_ON_PEER)
    );
}

/// Keep the shared constants and the delivery error taxonomy aligned.
#[test]
fn shared_delivery_constants_match_the_delivery_error_taxonomy() {
    use codeterm_mesh::contract::delivery;
    use codeterm_service_core::service::delivery_error::DeliveryError;

    let tab = || "e35319d7".to_string();
    let detail = || String::new();
    assert_eq!(
        DeliveryError::TabNotReachableOnPeer { tab: tab() }.code(),
        delivery::TAB_NOT_REACHABLE_ON_PEER
    );
    assert_eq!(
        DeliveryError::TabBusyRefused {
            tab: tab(),
            detail: detail()
        }
        .code(),
        delivery::TAB_BUSY_REFUSED
    );
    assert_eq!(
        DeliveryError::PtyUnavailable {
            tab: tab(),
            detail: detail()
        }
        .code(),
        delivery::PTY_UNAVAILABLE
    );
    assert_eq!(
        DeliveryError::SubmitUnconfirmed {
            tab: tab(),
            detail: detail()
        }
        .code(),
        delivery::SUBMIT_UNCONFIRMED
    );
}

#[test]
fn exec_relay_keeps_delivery_budget_dedupe_and_classified_restart_outcomes() {
    use codeterm_mesh::contract::delivery;

    assert_eq!(
        local_forward_deadline("POST", "/api/tabs/shell-1/exec"),
        SEND_FORWARD_DEADLINE
    );
    assert!(is_tab_delivery("POST", "/api/tabs/shell-1/exec"));
    assert!(relay_send_delivery_id("POST", "/api/tabs/shell-1/exec").is_some());

    let unconfirmed = relay_failure(
        409,
        delivery::SUBMIT_UNCONFIRMED,
        Some(true),
        "command queued; execution not confirmed".into(),
    );
    assert!(outcome_holds_dedupe(&unconfirmed));
    assert_eq!(
        unconfirmed.data.as_ref().unwrap()["outcome"],
        delivery::OUTCOME_SUBMIT_UNCONFIRMED
    );
    assert!(unconfirmed
        .error
        .as_deref()
        .unwrap()
        .starts_with(delivery::SUBMIT_UNCONFIRMED));

    let restarting = relay_failure(
        503,
        delivery::TARGET_RESTARTING,
        Some(true),
        "target restarted mid-exec".into(),
    );
    assert!(outcome_holds_dedupe(&restarting));
    assert_eq!(
        restarting.data.as_ref().unwrap()["outcome"],
        delivery::OUTCOME_UNKNOWN
    );

    let dropped = forward_failure_response(false, false, "connection reset".into());
    assert!(outcome_holds_dedupe(&dropped));
    assert_eq!(
        dropped.data.as_ref().unwrap()["outcome"],
        delivery::OUTCOME_UNKNOWN
    );
    assert_eq!(
        dropped.data.as_ref().unwrap()["error"]["code"],
        delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED
    );
    assert!(dropped.data.as_ref().unwrap()["error"]
        .get("bytes_written")
        .is_none());

    for code in [
        delivery::TAB_BUSY_REFUSED,
        delivery::SHELL_EXITED_REFUSED,
        delivery::EXEC_AGENT_TAB_REFUSED,
    ] {
        let refused = relay_failure(409, code, Some(false), "pre-write refusal".into());
        assert!(!outcome_holds_dedupe(&refused));
        assert_eq!(
            refused.data.as_ref().unwrap()["outcome"],
            delivery::OUTCOME_REFUSED
        );
        assert!(refused.error.as_deref().unwrap().starts_with(code));
    }
}
