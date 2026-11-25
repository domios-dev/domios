use super::{classify_auth_probe_result, ProviderAuthPreflightOutcome, ProviderSignInRequired};
use crate::agent::provider::AuthStatus;
use crate::service::provider_signals_probe::ProbeFailure;

#[test]
fn auth_probe_classification_refuses_explicit_signed_out_and_keeps_unknown_open() {
    let signed_out = classify_auth_probe_result(
        "opaque-cli",
        "Opaque CLI",
        Ok(Some(AuthStatus {
            logged_in: false,
            auth_method: None,
            subscription_type: None,
            config_directory: None,
        })),
    );
    assert_eq!(
        signed_out,
        Err(ProviderSignInRequired {
            provider_id: "opaque-cli".into(),
            display_name: "Opaque CLI".into(),
        })
    );

    let signed_in = classify_auth_probe_result(
        "opaque-cli",
        "Opaque CLI",
        Ok(Some(AuthStatus {
            logged_in: true,
            auth_method: Some("oauth".into()),
            subscription_type: None,
            config_directory: None,
        })),
    );
    assert_eq!(signed_in, Ok(ProviderAuthPreflightOutcome::Verified));

    for inconclusive in [
        classify_auth_probe_result("opaque-cli", "Opaque CLI", Ok(None)),
        classify_auth_probe_result("opaque-cli", "Opaque CLI", Err(ProbeFailure::TimedOut)),
        classify_auth_probe_result("opaque-cli", "Opaque CLI", Err(ProbeFailure::Unavailable)),
    ] {
        assert!(matches!(
            inconclusive,
            Ok(ProviderAuthPreflightOutcome::Inconclusive(_))
        ));
    }
}
