use super::*;

#[test]
fn notify_spawn_ok_does_not_deliver_a_user_turn_to_a_restored_recipient() {
    assert!(spawn_ok_notice_allowed(Some(false)));
    assert!(!spawn_ok_notice_allowed(Some(true)));
    assert!(!spawn_ok_notice_allowed(None));
}

#[test]
fn notify_user_spawned_tab_suppressed_for_trust_permission_login() {
    let facts = LaunchFacts {
        origin: LaunchOrigin::Ui,
        ..Default::default()
    };
    for label in ["trust_or_permission_prompt", "login_or_auth_required"] {
        assert!(suppress_owner_notification(label, &facts));
    }
    assert!(!suppress_owner_notification(
        "provider_update_prompt",
        &facts
    ));
}

#[test]
fn notify_agent_spawned_tab_still_notified() {
    let cases = [
        LaunchFacts {
            parent_or_spawner: true,
            ..Default::default()
        },
        LaunchFacts {
            team_owned: true,
            ..Default::default()
        },
        LaunchFacts {
            cron: true,
            ..Default::default()
        },
        LaunchFacts {
            hidden: true,
            ..Default::default()
        },
        LaunchFacts {
            general_agent: true,
            ..Default::default()
        },
    ];
    for mut facts in cases {
        facts.origin = LaunchOrigin::Ui;
        assert!(!suppress_owner_notification(
            "login_or_auth_required",
            &facts
        ));
    }
    assert!(!suppress_owner_notification(
        "login_or_auth_required",
        &LaunchFacts::default()
    ));
}

fn notice(tab: &str, label: &str, launch: &str) -> Notice {
    Notice {
        tab: tab.into(),
        title: tab.into(),
        label: label.into(),
        launch: launch.into(),
        body: "fixture".into(),
    }
}

#[test]
fn notify_restored_tabs_coalesced_per_state_kind_at_restore_complete() {
    let mut queue = RestoreQueue::default();
    for tab in ["a", "b", "c"] {
        assert!(queue
            .accept(notice(tab, "provider_update_prompt", "launch"), true)
            .is_empty());
    }
    assert!(queue
        .accept(notice("d", "login_or_auth_required", "launch"), true)
        .is_empty());
    let batches = queue.complete();
    assert_eq!(batches["provider_update_prompt"].len(), 3);
    assert_eq!(batches["login_or_auth_required"].len(), 1);
    assert!(queue.complete().is_empty());
    assert_eq!(
        queue
            .accept(notice("e", "provider_update_prompt", "new"), true)
            .len(),
        1
    );
}

#[test]
fn notify_dedupe_key_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let first = super::super::supporter::SupporterRuntime::new(dir.path().into());
    assert!(first.claim_notification("tab-state-launch").unwrap());
    let restored = super::super::supporter::SupporterRuntime::new(dir.path().into());
    assert!(!restored.claim_notification("tab-state-launch").unwrap());
}

#[test]
fn notify_new_launch_notifies_again() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = super::super::supporter::SupporterRuntime::new(dir.path().into());
    assert!(runtime
        .claim_notification("tab-state-first-launch")
        .unwrap());
    assert!(runtime
        .claim_notification("tab-state-second-launch")
        .unwrap());
    assert!(!runtime
        .claim_notification("tab-state-second-launch")
        .unwrap());
}

#[test]
fn notify_restore_settlement_waits_for_inflight_observations_without_timer() {
    let mut queue = RestoreQueue::default();
    assert!(queue.begin_observation());
    assert!(queue.begin_observation());
    assert!(queue.complete().is_empty());
    assert!(queue
        .accept(notice("a", "provider_update_prompt", "launch"), true)
        .is_empty());
    assert!(queue.finish_observation().is_empty());
    assert!(queue
        .accept(notice("b", "provider_update_prompt", "launch"), true)
        .is_empty());
    let grouped = queue.finish_observation();
    assert_eq!(grouped["provider_update_prompt"].len(), 2);
    assert!(!queue.begin_observation());
    assert_eq!(
        queue
            .accept(notice("c", "provider_update_prompt", "launch"), true)
            .len(),
        1
    );
}
