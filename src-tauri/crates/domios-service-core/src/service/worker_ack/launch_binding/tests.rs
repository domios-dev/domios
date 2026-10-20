use super::*;

fn authority() -> StartupAuthority {
    StartupAuthority::new("creator", "child", "launch")
        .in_team(Some("team".into()))
        .for_task(Some(StartupTaskIdentity {
            plan_id: "plan".into(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
        }))
        .for_task_owner_epoch(Some(1))
        .for_binding(None, "nonce")
}

#[test]
fn retained_launch_guards_all_authority_transitions_through_mutation_callback() {
    let registry = WorkerAckRegistry::default();
    let expected = authority();
    let _receiver = registry.arm_startup(expected.clone());
    registry
        .with_current_task_launch_authority("child", |current| {
            assert_eq!(current, &expected);
            assert!(registry.startup_waiters.try_lock().is_none());
            assert!(registry.startup_acknowledged.try_lock().is_none());
            assert!(registry.finalized_startups.try_lock().is_none());
            Ok(())
        })
        .unwrap();
    assert!(registry.startup_waiters.try_lock().is_some());
    assert!(registry.startup_acknowledged.try_lock().is_some());
    assert!(registry.finalized_startups.try_lock().is_some());
}

#[test]
fn retained_launch_finalized_authority_never_enters_mutation_callback() {
    let registry = WorkerAckRegistry::default();
    let expected = authority();
    let _receiver = registry.arm_startup(expected.clone());
    registry
        .hold_startup_authorized(
            "creator",
            "child",
            "launch",
            Some(&ChildStartupState {
                attempt_id: Some("launch".into()),
                team_id: Some("team".into()),
                session_id: None,
                binding_nonce: Some("nonce".into()),
            }),
        )
        .unwrap();
    let result: Result<(), String> = registry.with_current_task_launch_authority("child", |_| {
        panic!("finalized authority must not grant a mutation")
    });
    assert!(result.is_err());
    assert_eq!(
        registry.finalized_startups.lock().get("child"),
        Some(&expected)
    );
}
