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

#[test]
fn manager_team_utility_is_allowed_but_foreign_shell_and_ui_are_refused() {
    let managed = managed();
    for source in [Some((false, Some("team"))), Some((true, Some("foreign")))] {
        assert!(
            managed_send_refusal(Some(&managed), managed_send_origin(source, Some("team")))
                .is_none()
        );
    }
    for source in [
        None,
        Some((false, Some("foreign"))),
        Some((false, None)),
        Some((false, Some(""))),
    ] {
        assert!(
            managed_send_refusal(Some(&managed), managed_send_origin(source, Some("team")))
                .is_some()
        );
    }
    assert_eq!(
        managed_send_origin(Some((false, None)), None),
        ManagedSendOrigin::User
    );
    let mut released = managed;
    reconcile_manager(&mut released, false);
    assert!(managed_send_refusal(Some(&released), managed_send_origin(None, None)).is_none());
}
