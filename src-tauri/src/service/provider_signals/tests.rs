use super::*;
use codeterm_types::agent::provider_signals::{AuthStatus, SessionObservations};

#[test]
fn claude_signals_auth_overrides_scrape_only_when_available() {
    let screen = TabStateClassification {
        state: TabState::WaitingOnAuth,
        confidence: 0.9,
        data: None,
        action: None,
    };
    let mut signals = ProviderSignals::default();
    assert_eq!(
        classify(&signals, Some(screen.clone())),
        Some(screen.clone())
    );
    signals.auth = Some(AuthStatus {
        logged_in: true,
        auth_method: Some("oauth".into()),
        subscription_type: Some("max".into()),
        config_directory: Some("fixture".into()),
    });
    assert!(classify(&signals, Some(screen)).is_none());
    signals.auth.as_mut().unwrap().logged_in = false;
    assert_eq!(
        classify(&signals, None).unwrap().state,
        TabState::WaitingOnAuth
    );
}

#[test]
fn claude_signals_transcript_errors_and_plan_drift_are_typed() {
    let mut signals = ProviderSignals::default();
    signals.observe(&SessionObservations {
        api_error: Some(ApiErrorKind::RateLimit),
        permission_mode: Some(PermissionMode::Plan),
        clears_api_error: false,
    });
    assert!(plan_drift(&signals));
    assert_eq!(
        classify(&signals, None).unwrap().state,
        TabState::RateLimited
    );
    signals.observe(&SessionObservations {
        api_error: None,
        permission_mode: None,
        clears_api_error: true,
    });
    assert_eq!(signals.api_error, None);
    assert!(plan_drift(&signals));
    signals.observe(&SessionObservations {
        permission_mode: Some(PermissionMode::Default),
        ..Default::default()
    });
    assert!(!plan_drift(&signals));
}
