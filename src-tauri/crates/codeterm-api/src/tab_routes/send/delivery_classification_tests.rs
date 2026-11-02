use super::{
    chat_send_target_refusal, classify_delivery_failure, public_send_readiness_refusal,
    SendRejection,
};
use axum::http::StatusCode;
use codeterm_service_core::service::delivery_error::DeliveryError;

/// #240: a submit that failed AFTER the paste reached the PTY must never be
/// reported as a missing tab — the caller would retry onto parked text.
#[test]
fn submit_failure_is_not_reported_as_missing_tab() {
    let err = classify_delivery_failure(
        "53854ac8",
        "submit not confirmed after 3s; screen_tail: \"> parked body\"",
    );
    assert_eq!(err.code(), "submit_unconfirmed");
    assert!(err.bytes_written());
}

#[test]
fn booting_tab_refusal_is_a_zero_byte_busy_error() {
    let typed = DeliveryError::TabBusyRefused {
        tab: "53854ac8".into(),
        detail: "Agent in Tab 53854ac8 is still booting — input would be lost".into(),
    };
    let err = classify_delivery_failure("53854ac8", &typed.ledger_message());
    assert_eq!(err.code(), "tab_busy_refused");
    assert!(!err.bytes_written());
}

#[test]
fn pre_write_refusal_is_never_submit_unconfirmed() {
    let typed = DeliveryError::TabBusyRefused {
        tab: "53854ac8".into(),
        detail: "provider-classified update dialog is visible".into(),
    };
    let err = classify_delivery_failure("53854ac8", &typed.ledger_message());

    assert_eq!(err.code(), "tab_busy_refused");
    assert_eq!(err.status(), StatusCode::CONFLICT);
    assert!(!err.bytes_written());
    let rejection = SendRejection::from(err);
    assert_eq!(rejection.status, StatusCode::CONFLICT);
    assert_eq!(rejection.body["error"]["code"], "tab_busy_refused");
    assert_eq!(rejection.body["error"]["bytes_written"], false);
}

#[test]
fn public_send_readiness_deadline_is_a_zero_byte_route_refusal() {
    let rejection = public_send_readiness_refusal(
        "new-agent-tab",
        "public input readiness deadline elapsed; no input was written".into(),
    );

    assert_eq!(rejection.status, StatusCode::CONFLICT);
    assert_eq!(rejection.body["ok"], false);
    assert_eq!(rejection.body["error"]["code"], "tab_busy_refused");
    assert_eq!(rejection.body["error"]["bytes_written"], false);
}

#[test]
fn written_payload_failure_reports_submit_unconfirmed_with_bytes_written() {
    let rejection = SendRejection::from(DeliveryError::SubmitUnconfirmed {
        tab: "53854ac8".into(),
        detail: "provider modal appeared before recovery Enter".into(),
    });

    assert_eq!(rejection.status, StatusCode::BAD_GATEWAY);
    assert_eq!(rejection.body["error"]["code"], "submit_unconfirmed");
    assert_eq!(rejection.body["error"]["bytes_written"], true);
}

#[test]
fn chat_send_to_shell_without_agent_identity_refuses_without_writing() {
    let error = chat_send_target_refusal("shell-tab", false, false).unwrap();
    let rejection = SendRejection::from(error);

    assert_eq!(rejection.status, StatusCode::CONFLICT);
    assert_eq!(rejection.body["ok"], false);
    assert_eq!(rejection.body["error"]["code"], "tab_busy_refused");
    assert_eq!(rejection.body["error"]["bytes_written"], false);
}

#[test]
fn chat_send_target_guard_allows_booting_agent_identity_or_chat_backend() {
    assert!(chat_send_target_refusal("booting-agent-tab", true, false).is_none());
    assert!(chat_send_target_refusal("backend-tab", false, true).is_none());
}
