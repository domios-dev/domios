use super::*;

#[test]
fn registered_policy_resolves_launch_and_authority_together() {
    let profile = RoleProfile::Curator;
    let policy = resolve(Some(profile.as_str())).unwrap();
    assert_eq!(policy.profile(), profile);
    assert_eq!(policy.spawn_role(), "Manager");
    assert!(policy.retain_oneshot());
    assert!(!policy.allow_manual_fire());
    assert!(policy.raw_task());
    assert!(policy.retarget_only());
    assert!(policy.conservative_save());
    assert_eq!(policy.run_completion(), RunCompletion::ExplicitFinish);
    assert!(resolve(None).is_none());
    assert!(resolve(Some("fixture-profile")).is_none());
}
