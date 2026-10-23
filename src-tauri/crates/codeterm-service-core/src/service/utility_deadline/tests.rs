use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::ShellPurpose;
use crate::service::spawn::SpawnTabSpec;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn shell_marks_arm_cancel_and_restart_preserves_the_tab() {
    let (mut mux, owner) = MultiplexerState::new();
    mux.get_tab_mut(&owner).unwrap().agent_type = Some("fixture-provider".into());
    let fixture = ServiceFixture::new(mux);
    drop(fixture.bind_recorded_agent_recipient(&owner, "timeout-session", "timeout-nonce"));
    let (notice_sent, notice_received) = mpsc::channel();
    let _delivery = super::super::agent_delivery_outbox::test_boundary::Boundary::install(
        &owner,
        Arc::new(move |payload| {
            notice_sent
                .send(payload.transport.body().to_string())
                .map_err(|error| error.to_string())
        }),
        Arc::new(|| {}),
    );
    let shell = fixture
        .service
        .spawn_visible_shell_impl(SpawnTabSpec {
            creator_tab_id: Some(owner.clone()),
            purpose: ShellPurpose::Build,
            timeout: Some("60m".into()),
            title: Some("Deadline".into()),
            ..Default::default()
        })
        .unwrap();
    let initial = fixture
        .service
        .utility_deadline_snapshot(&shell.tab_id)
        .unwrap();
    assert_eq!(initial["duration_ms"], 3_600_000);
    assert!(initial["deadline_ms"].is_null());
    fixture
        .service
        .prepare_utility_command(&shell.tab_id, "opaque-command");
    assert!(fixture
        .service
        .utility_deadline_snapshot(&shell.tab_id)
        .unwrap()["deadline_ms"]
        .is_null());
    let pty = "fixture-deadline-pty";
    {
        let mut mux = fixture.service.mux.write();
        mux.link_pty(&shell.tab_id, pty.into());
        let tab = mux.get_tab_mut(&shell.tab_id).unwrap();
        tab.shell_integration_nonce = None;
    }
    fixture.service.process_pty_output(
        pty,
        b"\x1b]133;A\x07\x1b]633;E;opaque-command;\x07\x1b]133;C\x07",
    );
    let running = fixture
        .service
        .utility_deadline_snapshot(&shell.tab_id)
        .unwrap();
    assert_eq!(
        running["deadline_ms"].as_u64().unwrap() - running["started_ms"].as_u64().unwrap(),
        3_600_000
    );
    assert_eq!(running["command"], "opaque-command");
    fixture
        .service
        .set_utility_timeout(&shell.tab_id, &owner, "120m")
        .unwrap();
    let extended = fixture
        .service
        .utility_deadline_snapshot(&shell.tab_id)
        .unwrap();
    assert_eq!(extended["started_ms"], running["started_ms"]);
    assert_eq!(
        extended["deadline_ms"].as_u64().unwrap() - running["started_ms"].as_u64().unwrap(),
        7_200_000
    );
    fixture.service.process_pty_output(pty, b"\x1b]133;D;0\x07");
    let idle = fixture
        .service
        .utility_deadline_snapshot(&shell.tab_id)
        .unwrap();
    assert!(idle["deadline_ms"].is_null());
    assert!(idle["started_ms"].is_null());
    assert!(!COMMANDS.lock().contains_key(&shell.tab_id));
    fixture.service.process_pty_output(
        pty,
        b"\x1b]133;A\x07\x1b]633;E;second-opaque-command;\x07\x1b]133;C\x07",
    );
    let current = ARMED.lock().get(&shell.tab_id).unwrap().deadline.clone();
    assert!(current.model.started_ms.unwrap() >= running["started_ms"].as_u64().unwrap());
    let before = fixture
        .service
        .mux
        .read()
        .get_tab(&shell.tab_id)
        .unwrap()
        .clone();
    fixture.service.expire_utility_timeout(current);
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&shell.tab_id).unwrap();
        assert_eq!(tab.id, before.id);
        assert_eq!(tab.title, before.title);
        assert_eq!(tab.purpose, before.purpose);
        assert_eq!(tab.cwd, before.cwd);
        assert_ne!(tab.pty_id, before.pty_id);
    }
    let notice = notice_received
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert!(notice.contains("second-opaque-command"));
    assert!(notice.contains("command elapsed:"));
    assert!(notice.contains("deadline_ms:"));
    assert!(notice_received.try_recv().is_err());
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path(&fixture.service, &shell.tab_id)).unwrap())
            .unwrap();
    assert!(persisted["deadline_ms"].is_null());
    assert_eq!(persisted["duration_ms"], 7_200_000);
    fixture
        .service
        .set_utility_timeout(&shell.tab_id, &owner, "none")
        .unwrap();
    assert!(fixture
        .service
        .utility_deadline_snapshot(&shell.tab_id)
        .is_none());
    assert!(!path(&fixture.service, &shell.tab_id).exists());
    fixture.service.close_tab(&shell.tab_id).unwrap();
}

#[test]
fn dev_servers_refuse_a_command_timeout() {
    let (mut mux, owner) = MultiplexerState::new();
    mux.get_tab_mut(&owner).unwrap().agent_type = Some("fixture-provider".into());
    let fixture = ServiceFixture::new(mux);
    let shell = fixture
        .service
        .spawn_visible_shell_impl(SpawnTabSpec {
            creator_tab_id: Some(owner.clone()),
            purpose: ShellPurpose::Dev,
            ..Default::default()
        })
        .unwrap();
    assert!(fixture
        .service
        .set_utility_timeout(&shell.tab_id, &owner, "60m")
        .is_err());
    assert!(fixture
        .service
        .utility_deadline_snapshot(&shell.tab_id)
        .is_none());
    fixture.service.close_tab(&shell.tab_id).unwrap();
}
