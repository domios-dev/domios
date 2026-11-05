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
fn retained_launch_snapshot_releases_registry_guards_and_detects_same_authority_rearm() {
    let registry = WorkerAckRegistry::default();
    let expected = authority();
    let _receiver = registry.arm_startup(expected.clone());
    let captured = registry.current_task_launch_authority("child").unwrap();
    assert_eq!(captured.authority, expected);
    assert!(registry.launch_registry_available_for_test());
    registry.arm_startup(expected);
    let current = registry.current_task_launch_authority("child").unwrap();
    assert_ne!(current, captured);
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
    let result = registry.current_task_launch_authority("child");
    assert!(result.is_err());
    assert_eq!(
        registry.finalized_startups.lock().get("child"),
        Some(&expected)
    );
}
