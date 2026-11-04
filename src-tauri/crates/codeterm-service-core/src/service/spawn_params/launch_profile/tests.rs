use super::*;
use serde_json::json;

fn capability() -> AgentRuntimeCapability {
    serde_json::from_value(json!({
        "runtime": "stdio-jsonrpc", "command": {"binary": "fallback", "args": []},
        "launchProfiles": {"selected": {"binary": "fixed-engine", "args": []}},
        "requestMethods": {"initialize": ["initialize"]}, "notificationMethods": []
    }))
    .unwrap()
}

#[test]
fn profile_refusals_preserve_codes_and_declared_ids() {
    for (requested, installed, code) in [
        (Some("unknown"), true, "unknown_launch_profile"),
        (None, true, "launch_profile_required"),
        (Some("selected"), false, "launch_profile_not_installed"),
    ] {
        let refusal =
            validate("fixture", Some(&capability()), requested, |_, _| installed).unwrap_err();
        assert_eq!(refusal.code, code);
        assert_eq!(refusal.requested_profile.as_deref(), requested);
        assert_eq!(refusal.declared_profile_ids, ["selected"]);
        assert_eq!(refusal.to_string(), refusal.message);
        let native_message: String = SpawnTargetRefusal::from(refusal.clone()).into();
        assert_eq!(native_message, refusal.message);
        assert!(!refusal.to_string().contains('{'));
    }
    assert!(
        validate("fixture", Some(&capability()), Some("selected"), |_, _| {
            true
        })
        .is_ok()
    );
    let refusal = validate("pty", None, Some("selected"), |_, _| {
        panic!("no install probe")
    })
    .unwrap_err();
    assert_eq!(refusal.code, "unknown_launch_profile");
    assert!(refusal.declared_profile_ids.is_empty());
    assert!(validate("pty", None, None, |_, _| panic!("no install probe")).is_ok());
}

#[test]
fn non_runtime_profile_refuses_before_tab_creation() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let before = fixture.service.mux.read().snapshot().tabs.len();
    let params = crate::service::SpawnAgentParams {
        agent: Some("plain-provider".into()),
        launch_profile: Some("selected".into()),
        ..Default::default()
    };
    let error = fixture
        .service
        .classify_spawn_target_for_params("plain-provider", &params)
        .unwrap_err();
    let SpawnTargetRefusal::LaunchProfile(refusal) = error else {
        panic!("expected typed profile refusal")
    };
    assert_eq!(refusal.code, "unknown_launch_profile");
    assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), before);
}

#[test]
fn sandbox_refusal_stays_typed_through_spawn_target_validation() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let params = crate::service::SpawnAgentParams {
        sandbox: Some("read-only".into()),
        ..Default::default()
    };
    let error = fixture
        .service
        .validate_spawn_options("plain-provider", &params)
        .unwrap_err();
    assert!(matches!(error, SpawnTargetRefusal::ExecutionSandbox { .. }));
    assert_eq!(
        error.data().unwrap()["code"],
        "execution_sandbox_unsupported"
    );
    assert!(!error
        .to_string()
        .starts_with("execution_sandbox_unsupported:"));
}
