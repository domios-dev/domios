use super::DeliveryError;
use crate::service::inject_verify::InjectionError;
use axum::http::StatusCode;

fn submit_never_confirmed() -> InjectionError {
    InjectionError::SubmitNeverConfirmed {
        waited: std::time::Duration::from_secs(3),
        screen_tail: "> parked body".into(),
    }
}

#[test]
fn submit_failure_after_paste_is_never_reported_as_tab_not_found() {
    let err = DeliveryError::from_injection("53854ac8", submit_never_confirmed());
    assert_eq!(err.code(), "submit_unconfirmed");
    assert_ne!(
        err.status(),
        StatusCode::NOT_FOUND,
        "bytes reached the PTY — a 404 would tell the caller nothing happened"
    );
    assert!(err.bytes_written());
}

#[test]
fn echo_mismatch_admits_the_write() {
    let err = DeliveryError::from_injection(
        "p1",
        InjectionError::EchoMismatch {
            expected_substr: "hi".into(),
            screen_tail: "> ".into(),
        },
    );
    assert_eq!(err.code(), "submit_unconfirmed");
    assert!(err.bytes_written());
    assert!(
        err.ledger_message().starts_with("submit_unconfirmed:"),
        "the ledger boundary retains the retryable classification"
    );
}

#[test]
fn direct_send_boundaries_retain_delivery_error_type() {
    let _: fn(&crate::service::ServiceContext, &str, &str) -> Result<(), DeliveryError> =
        crate::service::ServiceContext::send_text_with_guarantee;
    let _: fn(&crate::service::ServiceContext, &str, &str) -> Result<(), DeliveryError> =
        crate::service::ServiceContext::send_text_delivery;
}

#[test]
fn preflight_refusals_guarantee_zero_bytes() {
    let booting = DeliveryError::from_injection(
        "p1",
        InjectionError::PromptNeverReady {
            waited: std::time::Duration::from_secs(1),
        },
    );
    assert_eq!(booting.code(), "tab_busy_refused");
    assert!(!booting.bytes_written());
    assert_eq!(booting.status(), StatusCode::CONFLICT);

    let tui = DeliveryError::from_injection(
        "p1",
        InjectionError::AgentTuiPresent {
            pty_id: "pty1".into(),
        },
    );
    assert_eq!(tui.code(), "tab_busy_refused");
    assert!(!tui.bytes_written());

    let stale_permit = DeliveryError::from_injection(
        "p1",
        InjectionError::AgentInputUnavailable {
            pty_id: "pty1".into(),
        },
    );
    assert_eq!(stale_permit.code(), "tab_busy_refused");
    assert!(!stale_permit.bytes_written());
}

#[test]
fn unresolvable_tab_is_the_only_not_found() {
    let err = DeliveryError::TabNotReachableOnPeer {
        tab: "ghost".into(),
    };
    assert_eq!(err.code(), "tab_not_reachable_on_peer");
    assert_eq!(err.status(), StatusCode::NOT_FOUND);
    assert!(!err.bytes_written());
}

#[test]
fn pty_closed_is_unavailable_not_missing() {
    let err = DeliveryError::from_injection(
        "p1",
        InjectionError::PtyClosed {
            pty_id: "pty1".into(),
        },
    );
    assert_eq!(err.code(), "pty_unavailable");
    assert!(!err.bytes_written());
    assert_ne!(err.status(), StatusCode::NOT_FOUND);
}

/// An unrecognised failure must not be allowed to claim zero bytes.
#[test]
fn opaque_failure_defaults_to_admitting_the_write() {
    let err = DeliveryError::from_opaque("p1", "something exploded mid-paste");
    assert_eq!(err.code(), "submit_unconfirmed");
    assert!(err.bytes_written());
}

#[test]
fn opaque_boot_refusal_is_recognised_as_zero_byte() {
    let err = DeliveryError::from_opaque(
        "p1",
        "Agent in Tab p1 is still booting — input would be lost",
    );
    assert_eq!(err.code(), "tab_busy_refused");
    assert!(!err.bytes_written());
}

#[test]
fn json_surface_carries_code_and_write_state() {
    let err = DeliveryError::from_injection("53854ac8", submit_never_confirmed());
    let json = err.to_json();
    assert_eq!(json["ok"], serde_json::json!(false));
    assert_eq!(
        json["error"]["code"],
        serde_json::json!("submit_unconfirmed")
    );
    assert_eq!(json["error"]["bytes_written"], serde_json::json!(true));
    assert_eq!(
        json["error"]["delivery_state"],
        serde_json::json!("unconfirmed")
    );
    assert_eq!(json["error"]["tab"], serde_json::json!("53854ac8"));
}
