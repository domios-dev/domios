use super::*;

#[test]
fn ga_exit_recovery_uses_verified_session_or_bounded_fresh_launch() {
    let verified = recovery_target(
        Ok(("fixture-provider".into(), Some("verified-session".into()))),
        Some("fixture-provider"),
    )
    .unwrap();
    assert_eq!(
        verified,
        ("fixture-provider".into(), Some("verified-session".into()))
    );
    let unverified = recovery_target(
        Err(AgentRestartError::BadRequest("unverified".into())),
        Some("fixture-provider"),
    )
    .unwrap();
    assert_eq!(unverified, ("fixture-provider".into(), None));
    assert!(recovery_target(
        Err(AgentRestartError::BadRequest("unverified".into())),
        None
    )
    .is_err());
}
