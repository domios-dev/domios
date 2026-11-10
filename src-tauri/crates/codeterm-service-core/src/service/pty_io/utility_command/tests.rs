use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

fn fixture(pty: &str) -> (ServiceFixture, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.link_pty(&tab_id, pty.into());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.role = None;
    tab.role_profile = None;
    tab.shell_integration_nonce = Some("nonce".into());
    tab.shell_command_state = ShellCommandState {
        activity: ShellActivity::Idle,
        last_exit_code: Some(7),
    };
    (ServiceFixture::new(mux), tab_id)
}

#[test]
fn authenticated_running_command_refuses_exec_before_preparation_or_any_write() {
    let pty = "utility-busy-admission";
    let (fixture, tab_id) = fixture(pty);
    fixture.service.process_pty_output(
        pty,
        b"\x1b]633;A\x07\x1b]633;E;previous;nonce\x07\x1b]633;C\x07",
    );
    let before = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .shell_command_state;
    let error = fixture
        .service
        .send_shell_spawn_observed(pty, "next")
        .err()
        .unwrap();
    assert!(error.starts_with("tab_busy_refused:"), "{error}");
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.shell_command_state, before);
    assert_eq!(tab.shell_command_state.activity, ShellActivity::Running);
    assert!(!tab.utility_command_pending);
}

#[test]
fn failed_prewrite_admission_restores_state_and_uncertain_write_keeps_input_owned() {
    let pty = "utility-admission-rollback";
    let (fixture, tab_id) = fixture(pty);
    let before = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .shell_command_state;
    let admission = UtilityCommandAdmission::begin(&fixture.service, pty).unwrap();
    assert!(UtilityCommandAdmission::begin(&fixture.service, pty).is_err());
    drop(admission);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .shell_command_state,
        before
    );
    let admission = UtilityCommandAdmission::begin(&fixture.service, pty).unwrap();
    admission.mark_written();
    drop(admission);
    assert!(UtilityCommandAdmission::begin(&fixture.service, pty).is_err());
    assert!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .utility_command_pending
    );
    fixture.service.process_pty_output(
        pty,
        b"\x1b]633;A\x07\x1b]633;E;next;nonce\x07\x1b]633;C\x07\x1b]633;D;0\x07",
    );
    assert!(UtilityCommandAdmission::begin(&fixture.service, pty).is_ok());
}

#[test]
fn competing_admissions_have_exactly_one_owner() {
    let pty = "utility-competing-admission";
    let (fixture, _) = fixture(pty);
    let barrier = std::sync::Barrier::new(2);
    let winners = std::thread::scope(|scope| {
        let attempt = || {
            barrier.wait();
            let admission = UtilityCommandAdmission::begin(&fixture.service, pty);
            let admitted = admission.is_ok();
            barrier.wait();
            drop(admission);
            admitted
        };
        let first = scope.spawn(attempt);
        let second = scope.spawn(attempt);
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(winners.into_iter().filter(|admitted| *admitted).count(), 1);
}

#[test]
fn command_reservation_refuses_competing_exec_without_touching_shell_state() {
    let pty = "utility-exclusive-command";
    let (fixture, tab_id) = fixture(pty);
    let reservation = crate::service::pty_io::submit_transaction::reserve_command_input(
        pty,
        std::time::Instant::now(),
    )
    .unwrap();
    let error = fixture
        .service
        .send_shell_spawn_observed(pty, "next")
        .err()
        .unwrap();
    assert!(error.starts_with("tab_busy_refused:"), "{error}");
    assert!(
        !fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .utility_command_pending
    );
    drop(reservation);
}

#[test]
fn failed_prewrite_cannot_restore_state_into_a_replaced_pty() {
    let pty = "utility-old-pty";
    let (fixture, tab_id) = fixture(pty);
    let admission = UtilityCommandAdmission::begin(&fixture.service, pty).unwrap();
    {
        let mut mux = fixture.service.mux.write();
        mux.link_pty(&tab_id, "replacement-pty".into());
        mux.get_tab_mut(&tab_id).unwrap().shell_command_state = Default::default();
    }
    drop(admission);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .shell_command_state,
        ShellCommandState::default()
    );
}
