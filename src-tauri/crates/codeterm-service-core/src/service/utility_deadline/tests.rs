use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::ShellPurpose;
use crate::service::spawn::SpawnTabSpec;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn launched_shell_renews_requested_deadline_on_restart() {
    let (mut mux, owner) = MultiplexerState::new();
    mux.get_tab_mut(&owner).unwrap().agent_type = Some("fixture-provider".into());
    let fixture = ServiceFixture::new(mux);
    let shell = fixture
        .service
        .spawn_visible_shell_impl(SpawnTabSpec {
            creator_tab_id: Some(owner),
            purpose: ShellPurpose::Build,
            timeout: Some("30m".into()),
            eager: true,
            ..Default::default()
        })
        .unwrap();
    let original = fixture
        .service
        .utility_deadline_snapshot(&shell.tab_id)
        .unwrap();
    assert_eq!(
        original["deadline_ms"].as_u64().unwrap() - original["started_ms"].as_u64().unwrap(),
        1_800_000,
    );
    fixture
        .service
        .prepare_utility_command(&shell.tab_id, "opaque-command");
    let during_command = fixture
        .service
        .utility_deadline_snapshot(&shell.tab_id)
        .unwrap();
    assert_eq!(during_command["deadline_ms"], original["deadline_ms"]);
    fixture
        .service
        .restart_shell_tab(&shell.tab_id, None)
        .unwrap();
    fixture
        .service
        .utility_command_started(&shell.tab_id, Some("opaque-restarted-command"));
    let inspected = crate::dispatch::dispatch(
        &crate::dispatch::Command::TabInspect {
            tab_ids: vec![shell.tab_id.clone()],
            lifecycle: false,
        },
        &fixture.service,
    )
    .unwrap()
    .into_json();
    let renewed = &inspected[0]["utility_deadline"];
    assert!(renewed["started_ms"].as_u64().unwrap() >= original["started_ms"].as_u64().unwrap());
    assert_eq!(
        renewed["deadline_ms"].as_u64().unwrap() - renewed["started_ms"].as_u64().unwrap(),
        1_800_000
    );
    assert_eq!(
        inspected[0]["utility_deadline"]["command"],
        "opaque-restarted-command"
    );
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path(&fixture.service, &shell.tab_id)).unwrap())
            .unwrap();
    assert_eq!(persisted["deadline_ms"], renewed["deadline_ms"]);
    fixture.service.close_tab(&shell.tab_id).unwrap();
    assert!(fixture
        .service
        .utility_deadline_snapshot(&shell.tab_id)
        .is_none());
    assert!(!path(&fixture.service, &shell.tab_id).exists());
}

#[test]
fn expiry_journals_utility_timeout_instead_of_user_close() {
    let (mut mux, owner) = MultiplexerState::new();
    mux.get_tab_mut(&owner).unwrap().agent_type = Some("fixture-provider".into());
    let fixture = ServiceFixture::new(mux);
    let shell = fixture
        .service
        .spawn_visible_shell_impl(SpawnTabSpec {
            creator_tab_id: Some(owner),
            timeout: Some("30m".into()),
            ..Default::default()
        })
        .unwrap();
    fixture.service.open_lifecycle_epoch(&shell.tab_id).unwrap();
    let mut deadline = ARMED.lock().get(&shell.tab_id).unwrap().deadline.clone();
    deadline.deadline_ms = now_ms();
    fixture.service.expire_utility_timeout(deadline);
    assert!(fixture.service.mux.read().get_tab(&shell.tab_id).is_none());
    assert!(fixture
        .service
        .lifecycle_journal
        .load()
        .records
        .iter()
        .any(|record| matches!(record,
            crate::session::lifecycle::JournalRecord::Transition { entry }
                if matches!(&entry.transition,
                    crate::session::lifecycle::LifecycleTransition::Closed { reason }
                        if reason == "utility_timeout"
                )
        )));
}

#[test]
fn restart_renews_lifetime_and_clears_previous_command_without_runtime() {
    let previous = Deadline {
        tab_id: "shell".into(),
        owner: "owner".into(),
        started_ms: 1_000,
        deadline_ms: 601_000,
        command: Some("old-command".into()),
        command_started_ms: Some(580_000),
    };
    let renewed = previous.renewed(600_000).unwrap();
    assert!(!renewed.same_run(&previous));
    let same_end = Deadline {
        started_ms: 500_000,
        ..previous.clone()
    };
    assert!(!same_end.same_run(&previous));
    assert!(renewed.same_run(&renewed.clone()));
    assert_eq!(renewed.started_ms, 600_000);
    assert_eq!(renewed.deadline_ms, 1_200_000);
    assert_eq!(renewed.owner, previous.owner);
    assert_eq!(renewed.command, None);
    assert_eq!(renewed.command_started_ms, None);
    let mut model = DeadlineModel::default();
    model.reset(Some(renewed.deadline_ms));
    assert_eq!(
        model.expire(
            previous.deadline_ms,
            crate::multiplexer::types::ShellActivity::Running
        ),
        None
    );
    assert_eq!(
        model.expire(
            renewed.deadline_ms,
            crate::multiplexer::types::ShellActivity::Running
        ),
        Some(DeadlineAction::TerminateAndNotice)
    );
    assert!(previous.renewed(u64::MAX).is_err());
}

#[test]
fn later_commands_keep_the_creation_lifetime_without_a_command_default() {
    let mut deadline = Deadline {
        tab_id: "shell".into(),
        owner: "owner".into(),
        started_ms: 1_000,
        deadline_ms: 1_801_000,
        command: None,
        command_started_ms: None,
    };
    deadline.command_started(Some("first"), 20_000);
    deadline.command_started(Some("second"), 27_000);
    assert_eq!(deadline.started_ms, 1_000);
    assert_eq!(deadline.deadline_ms, 1_801_000);
    assert_eq!(deadline.command_started_ms, Some(27_000));
    let mut model = DeadlineModel::default();
    model.reset(Some(deadline.deadline_ms));
    for elapsed in [574_000, 600_000, 1_799_999] {
        assert_eq!(
            model.expire(
                deadline.started_ms + elapsed,
                crate::multiplexer::types::ShellActivity::Running
            ),
            None
        );
    }
    assert_eq!(
        model.expire(
            deadline.deadline_ms,
            crate::multiplexer::types::ShellActivity::Running
        ),
        Some(DeadlineAction::TerminateAndNotice)
    );
}
