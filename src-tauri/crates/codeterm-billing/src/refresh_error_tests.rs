#[test]
fn refresh_error_kinds_redact_provider_payloads() {
    use crate::cache::refresh_error_kind;

    assert_eq!(
        refresh_error_kind("Installation proof unavailable"),
        "installation_proof_unavailable"
    );
    assert_eq!(
        refresh_error_kind("Billing server unreachable"),
        "transport"
    );
    assert_eq!(
        refresh_error_kind("Invalid billing response"),
        "response_decode"
    );
    assert_eq!(
        refresh_error_kind("Invalid callable response"),
        "response_decode"
    );
    assert_eq!(
        refresh_error_kind("Billing request refused"),
        "backend_refusal"
    );
    assert_eq!(
        refresh_error_kind(codeterm_account::session_revocation::SESSION_REVOKED),
        "session_revoked"
    );
    assert_eq!(
        refresh_error_kind("token=secret proof=secret"),
        "provider_failure"
    );
}
