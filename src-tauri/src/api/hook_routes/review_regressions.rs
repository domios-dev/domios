use super::*;

#[test]
fn nonceless_duplicate_does_not_take_the_acknowledged_shortcut() {
    let claim = SessionStartClaim {
        prev: Some("session".into()),
        changed: false,
        nonce_verified: false,
        nonce_already_acknowledged: true,
        identity_evidence: crate::agent::provider_identity::ProviderIdentityEvidence {
            cwd: Some("/tmp/project".into()),
            session_id: Some("session".into()),
            configured_nonce: None,
            reported_nonce: None,
        },
    };

    assert!(!claim.is_unchanged_acknowledged_report());
    assert!(!can_short_circuit_session_start(&claim, true, true));
}
