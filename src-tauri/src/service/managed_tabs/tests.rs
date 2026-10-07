use super::*;

fn managed() -> ManagedTab {
    managed_tab_at_spawn(true, Some(("manager", "Scout", true))).unwrap()
}

#[test]
fn only_tool_less_agent_spawned_tabs_are_managed() {
    assert!(managed_tab_at_spawn(false, Some(("manager", "Scout", true))).is_none());
    assert!(managed_tab_at_spawn(true, Some(("shell", "Shell", false))).is_none());
    assert!(managed_tab_at_spawn(true, None).is_none());
    assert!(managed_tab_at_spawn(true, Some(("", "Unknown", true))).is_none());
    assert_eq!(managed().manager_tab_id, "manager");
}

#[test]
fn managed_tab_refuses_user_and_shell_input_but_allows_agents_and_stop() {
    let managed = managed();
    assert!(
        managed_send_refusal(Some(&managed), ManagedSendOrigin::User)
            .unwrap()
            .contains(&managed.manager_name)
    );
    for origin in [
        ManagedSendOrigin::Agent,
        ManagedSendOrigin::System,
        ManagedSendOrigin::Stop,
    ] {
        assert_eq!(managed_send_refusal(Some(&managed), origin), None);
    }
    assert_eq!(managed_send_refusal(None, ManagedSendOrigin::User), None);
}

#[test]
fn manager_closure_unlocks_input_and_keeps_notice_identity() {
    let mut managed = managed();
    reconcile_manager(&mut managed, true);
    assert!(managed.is_active());
    reconcile_manager(&mut managed, false);
    assert!(!managed.is_active());
    assert_eq!(
        managed_send_refusal(Some(&managed), ManagedSendOrigin::User),
        None
    );
    assert_eq!(managed.manager_name, "Scout");
    reconcile_manager(&mut managed, true);
    assert!(!managed.is_active());
}

#[test]
fn managed_ownership_survives_snapshot_serialization() {
    let original = managed();
    let restored: ManagedTab =
        serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
    assert_eq!(restored, original);
}
