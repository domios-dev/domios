use super::*;
use crate::agent::command_detection::CommandLifecycle;

fn start(command: Option<&str>) -> CommandLifecycle {
    CommandLifecycle::Started {
        command_line: command.map(str::to_string),
        cwd: None,
    }
}

fn finish(code: Option<i32>) -> CommandLifecycle {
    CommandLifecycle::Finished {
        command_line: None,
        exit_code: code,
        cwd: None,
    }
}

fn model() -> CommandCorrelation {
    CommandCorrelation {
        expected: Some("target".into()),
        started: false,
    }
}

#[test]
fn startup_and_previous_command_marks_cannot_finish_target() {
    let mut model = model();
    assert_eq!(model.observe(&finish(Some(0))), None);
    assert_eq!(model.observe(&start(Some("previous"))), None);
    assert_eq!(model.observe(&finish(Some(0))), None);
    assert_eq!(model.observe(&start(Some("target"))), None);
    assert_eq!(
        model.observe(&finish(Some(2))),
        Some(ExecCompletion::CommandFinished { exit_code: Some(2) })
    );
    assert_eq!(model.observe(&finish(Some(0))), None);
}

#[test]
fn missing_command_or_start_remains_unknown() {
    let mut model = model();
    assert_eq!(model.observe(&start(None)), None);
    assert_eq!(model.observe(&finish(Some(0))), None);
    assert_eq!(model.observe(&start(Some(""))), None);
    assert_eq!(model.observe(&finish(Some(0))), None);
    assert_eq!(model.observe(&start(Some("target"))), None);
    assert_eq!(
        model.observe(&finish(None)),
        Some(ExecCompletion::CommandFinished { exit_code: None })
    );
}

#[test]
fn intervening_command_invalidates_target_start() {
    let mut model = model();
    model.observe(&start(Some("target")));
    model.observe(&start(Some("other")));
    assert_eq!(model.observe(&finish(Some(0))), None);
}

#[tokio::test]
async fn shell_markers_wire_only_matching_start_then_finish_to_waiter() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    let (mut mux, tab) = MultiplexerState::new();
    let pty = "exec-marker-wiring";
    mux.link_pty(&tab, pty.into());
    mux.get_tab_mut(&tab).unwrap().shell_integration_nonce = Some("nonce".into());
    let fixture = ServiceFixture::new(mux);
    let mut waiter = ExecWaiter::for_command(pty, "target");
    fixture.service.process_pty_output(pty, b"\x1b]633;E;;nonce\x07\x1b]633;D;0\x07\x1b]633;A\x07\x1b]633;E;previous;nonce\x07\x1b]633;C\x07\x1b]633;D;0\x07");
    assert_eq!(
        waiter.rx.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    );
    fixture.service.process_pty_output(
        pty,
        b"\x1b]633;A\x07\x1b]633;E;target;nonce\x07\x1b]633;C\x07",
    );
    assert_eq!(
        waiter.rx.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    );
    fixture.service.process_pty_output(pty, b"\x1b]633;D;2\x07");
    assert_eq!(
        waiter.wait(Duration::ZERO).await,
        Some(ExecCompletion::CommandFinished { exit_code: Some(2) })
    );
}

#[tokio::test]
async fn missing_start_times_out_and_shell_exit_retains_its_status() {
    let silent = ExecWaiter::for_command("exec-missing-start", "target");
    observe_command("exec-missing-start", &finish(Some(0)));
    assert_eq!(silent.wait(Duration::ZERO).await, None);
    let exited = ExecWaiter::register("exec-shell-exit");
    publish(
        "exec-shell-exit",
        ExecCompletion::ShellExited { exit_code: Some(7) },
    );
    assert_eq!(
        exited.wait(Duration::ZERO).await,
        Some(ExecCompletion::ShellExited { exit_code: Some(7) })
    );
}

#[tokio::test]
async fn shell_exit_without_own_start_never_reports_success() {
    let waiter = ExecWaiter::for_command("exec-unstarted-exit", "target");
    publish(
        "exec-unstarted-exit",
        ExecCompletion::ShellExited { exit_code: Some(0) },
    );
    assert_eq!(
        waiter.wait(Duration::ZERO).await,
        Some(ExecCompletion::ShellExited { exit_code: None })
    );
}

#[test]
fn sacrificial_paste_space_does_not_change_command_identity() {
    let mut model = model();
    assert_eq!(model.observe(&start(Some(" target"))), None);
    assert_eq!(
        model.observe(&finish(Some(0))),
        Some(ExecCompletion::CommandFinished { exit_code: Some(0) })
    );
}
