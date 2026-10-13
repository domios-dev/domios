use super::*;
use crate::multiplexer::types::RestorableLaunchParameters;

#[test]
fn saved_opaque_arguments_refuse_runtime_use_with_a_visible_reason_without_leaking_arguments() {
    let launch = RestorableLaunchParameters {
        args: Some(vec!["opaque-private-value".into()]),
        ..Default::default()
    };
    let failure = validate_restored_arguments("fixture-provider", Some(&launch)).unwrap_err();
    assert!(matches!(failure, RuntimeFailure::Refused(_)));
    let mut state = RuntimeState::unloaded(Some("saved-thread".into()));
    let epoch = state.begin();
    assert!(state.degrade(epoch, &failure));
    let snapshot = state.snapshot();
    assert_eq!(snapshot.state, "degraded");
    let reason = snapshot.reason.unwrap();
    assert!(reason.contains("runtime_launch_arguments"));
    assert!(!reason.contains("opaque-private-value"));
}

#[test]
fn saved_typed_settings_and_empty_arguments_remain_eligible_for_runtime_use() {
    assert!(validate_restored_arguments("fixture-provider", None).is_ok());
    let launch = RestorableLaunchParameters {
        requested_model: Some("typed-model".into()),
        requested_reasoning_effort: Some("typed-effort".into()),
        args: Some(Vec::new()),
        ..Default::default()
    };
    assert!(validate_restored_arguments("fixture-provider", Some(&launch)).is_ok());
}
