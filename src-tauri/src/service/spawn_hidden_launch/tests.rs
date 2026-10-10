use super::after_shell_ready;
use codeterm_pty::pty::screen_state::ScreenState;
use codeterm_util::lock_hold::TimedMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn hidden_system_instructions_require_one_starter_turn() {
    assert_eq!(
        super::first_turn(true, true, Some("opaque-task"), Some("opaque-starter")),
        Some("opaque-starter".into())
    );
    assert_eq!(
        super::first_turn(false, true, Some("opaque-task"), None),
        None
    );
    assert_eq!(
        super::first_turn(false, false, Some("opaque-task"), None),
        Some("opaque-task".into())
    );
    assert_eq!(super::first_turn(true, true, None, None), None);
}

fn screen_state() -> crate::ScreenStateHandle {
    Arc::new(TimedMutex::new("screen_state", ScreenState::new()))
}

#[test]
fn cron_spawn_waits_for_shell_ready_event() {
    let screen = screen_state();
    assert!(screen.lock().bind_pty_generation("cron-pty", 1));
    let ready = screen.lock().arm_shell_ready_signal("cron-pty");
    let signal_screen = screen.clone();
    let launch_issued = Arc::new(AtomicBool::new(false));
    let launch_result = launch_issued.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let waiter_screen = screen.clone();
    let waiter = std::thread::spawn(move || {
        let result = after_shell_ready(
            true,
            &waiter_screen,
            "cron-pty",
            Duration::from_secs(60),
            || launch_result.store(true, Ordering::Release),
        );
        done_tx.send(result.is_ok()).unwrap();
    });

    assert!(!ready.is_ready());
    assert!(signal_screen
        .lock()
        .process_if_generation("cron-pty", 1, b"\x1b]133;A"));
    assert!(done_rx.recv_timeout(Duration::from_secs(5)).unwrap());
    waiter.join().unwrap();
    assert!(launch_issued.load(Ordering::Acquire));
}

#[test]
fn cron_spawn_shell_ready_deadline_fails_cleanly() {
    let screen = screen_state();
    assert!(screen.lock().bind_pty_generation("cron-pty", 1));
    let launch_issued = AtomicBool::new(false);

    let result = after_shell_ready(true, &screen, "cron-pty", Duration::ZERO, || {
        launch_issued.store(true, Ordering::Release)
    });

    assert!(result.is_err());
    assert!(!launch_issued.load(Ordering::Acquire));
}
