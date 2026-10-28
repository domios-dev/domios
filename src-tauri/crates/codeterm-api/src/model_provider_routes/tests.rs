// WHAT: local model-provider management requires the host credential.
// WHY: a scoped tab credential must not gain owner-only provider authority.
// ORACLE: loopback host passes; scoped, missing, unknown and remote callers refuse.
use super::*;
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};

#[test]
fn model_provider_management_accepts_only_the_loopback_host_credential() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("model-owner-fixture".into());
    let scoped = state
        .svc
        .frontend_pty_env(Some(&tab))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let local = "127.0.0.1:7000".parse().unwrap();
    for token in ["model-owner-fixture", scoped.as_str(), "unknown"] {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
        assert_eq!(
            host_caller(&state, local, &headers),
            token == "model-owner-fixture"
        );
        assert!(!host_caller(
            &state,
            "192.0.2.10:7000".parse().unwrap(),
            &headers
        ));
    }
    assert!(!host_caller(&state, local, &HeaderMap::new()));
}
