use super::{DeliveryError, SendOrigin};
use crate::service::inject_verify::InjectionError;
use axum::http::StatusCode;

fn tab_busy() -> DeliveryError {
    DeliveryError::TabBusyRefused {
        tab: "p1".into(),
        detail: "still booting".into(),
    }
}

fn pty_unavailable() -> DeliveryError {
    DeliveryError::PtyUnavailable {
        tab: "p1".into(),
        detail: "PTY did not link".into(),
    }
}

fn submit_unconfirmed() -> DeliveryError {
    DeliveryError::SubmitUnconfirmed {
        tab: "p1".into(),
        detail: "submit key not acknowledged".into(),
    }
}

#[test]
fn user_origin_never_surfaces_any_refusal() {
    for err in [
        tab_busy(),
        pty_unavailable(),
        submit_unconfirmed(),
        DeliveryError::TabNotReachableOnPeer { tab: "p1".into() },
    ] {
        assert!(
            !err.surfaces_as_failure(SendOrigin::User),
            "{} must degrade to pending-until-link for a user send",
            err.code()
        );
        assert_eq!(err.into_surfaced(SendOrigin::User), Ok(()));
    }
}

#[test]
fn programmatic_origin_keeps_the_honest_typed_split() {
    // Pre-write refusals surface (nothing was written); a post-write submit
    // stays unknown (bytes may have landed).
    assert!(tab_busy().surfaces_as_failure(SendOrigin::Programmatic));
    assert!(pty_unavailable().surfaces_as_failure(SendOrigin::Programmatic));
    assert!(DeliveryError::TabNotReachableOnPeer { tab: "p1".into() }
        .surfaces_as_failure(SendOrigin::Programmatic));
    assert!(!submit_unconfirmed().surfaces_as_failure(SendOrigin::Programmatic));
    assert!(tab_busy().into_surfaced(SendOrigin::Programmatic).is_err());
    assert!(submit_unconfirmed()
        .into_surfaced(SendOrigin::Programmatic)
        .is_ok());
}

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
    let _: fn(
        &crate::service::ServiceContext,
        &str,
        &str,
        Option<crate::service::pty_io::submit_transaction::SubmitCycleClaim>,
    ) -> Result<(), DeliveryError> = crate::service::ServiceContext::send_chat_text_claimed;
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

mod payload_preflight_bound_tests {
    use super::super::{preflight_payload_size, SendOrigin, PROGRAMMATIC_SEND_MAX_BYTES};

    #[test]
    fn payload_preflight_bound_is_exactly_sixteen_megabytes() {
        assert_eq!(PROGRAMMATIC_SEND_MAX_BYTES, 16 * 1024 * 1024);
    }

    #[test]
    fn payload_preflight_refuses_only_over_bound_programmatic() {
        let over = PROGRAMMATIC_SEND_MAX_BYTES + 1;
        let err = preflight_payload_size("p1", SendOrigin::Programmatic, over).unwrap_err();
        assert_eq!(err.code(), "payload_too_large");
        assert_eq!(err.tab, "p1");
        assert_eq!(err.payload_bytes, over);
        assert_eq!(err.max_bytes, PROGRAMMATIC_SEND_MAX_BYTES);
        assert_eq!(
            preflight_payload_size("p1", SendOrigin::Programmatic, PROGRAMMATIC_SEND_MAX_BYTES),
            Ok(())
        );
    }

    #[test]
    fn payload_preflight_exempts_user_origin_at_any_size() {
        assert_eq!(
            preflight_payload_size("p1", SendOrigin::User, PROGRAMMATIC_SEND_MAX_BYTES + 1),
            Ok(())
        );
        assert_eq!(preflight_payload_size("p1", SendOrigin::User, usize::MAX), Ok(()));
    }

    #[test]
    fn payload_preflight_refusal_json_claims_zero_bytes() {
        let err = preflight_payload_size("p1", SendOrigin::Programmatic, usize::MAX).unwrap_err();
        let json = err.to_json();
        assert_eq!(json["ok"], serde_json::json!(false));
        assert_eq!(json["error"]["code"], serde_json::json!("payload_too_large"));
        assert_eq!(json["error"]["bytes_written"], serde_json::json!(false));
        assert_eq!(json["error"]["tab"], serde_json::json!("p1"));
    }
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
