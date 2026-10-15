use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::ShellPurpose;
use crate::service::spawn::SpawnTabSpec;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn launched_shell_retains_requested_deadline_across_command_and_restart() {
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
    assert_eq!(
        inspected[0]["utility_deadline"]["started_ms"],
        original["started_ms"]
    );
    assert_eq!(
        inspected[0]["utility_deadline"]["deadline_ms"],
        original["deadline_ms"]
    );
    assert_eq!(
        inspected[0]["utility_deadline"]["command"],
        "opaque-restarted-command"
    );
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path(&fixture.service, &shell.tab_id)).unwrap())
            .unwrap();
    assert_eq!(persisted["deadline_ms"], original["deadline_ms"]);
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
