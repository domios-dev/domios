use super::*;
use std::sync::mpsc::channel;

#[test]
fn second_command_waits_for_post_model_idle_event() {
    let signal = Arc::new(ScreenChangeSignal::default());
    let idle = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (tx, rx) = channel();
    let worker_signal = signal.clone();
    let worker_idle = idle.clone();
    let worker = std::thread::spawn(move || {
        let writes = std::cell::RefCell::new(Vec::new());
        execute_command(
            || true,
            || {
                writes.borrow_mut().push("model");
                Ok(())
            },
            || true,
        )
        .unwrap();
        let mut first = true;
        wait_for_idle(
            &worker_signal,
            Instant::now() + Duration::from_secs(2),
            || {
                let ready = worker_idle.load(std::sync::atomic::Ordering::Acquire);
                if first {
                    first = false;
                    tx.send(writes.borrow().clone()).unwrap();
                }
                post_command_idle(false, ready)
            },
        )
        .unwrap();
        execute_command(
            || worker_idle.load(std::sync::atomic::Ordering::Acquire),
            || {
                writes.borrow_mut().push("effort");
                Ok(())
            },
            || true,
        )
        .unwrap();
        writes.into_inner()
    });
    assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), ["model"]);
    idle.store(true, std::sync::atomic::Ordering::Release);
    signal.mark_changed();
    assert_eq!(worker.join().unwrap(), ["model", "effort"]);
}

#[test]
fn idle_before_wait_and_idle_between_observation_and_wait_are_retained() {
    let signal = ScreenChangeSignal::default();
    wait_for_idle(&signal, Instant::now() + Duration::from_secs(2), || {
        Ok(true)
    })
    .unwrap();
    let mut observations = 0;
    wait_for_idle(&signal, Instant::now() + Duration::from_secs(2), || {
        observations += 1;
        if observations == 1 {
            signal.mark_changed();
            post_command_idle(false, false)
        } else {
            Ok(true)
        }
    })
    .unwrap();
    assert_eq!(observations, 2);
}

#[test]
fn foreign_turn_refuses_second_command_even_if_idle_arrives() {
    let signal = ScreenChangeSignal::default();
    let mut observations = 0;
    let result = wait_for_idle(&signal, Instant::now() + Duration::from_secs(2), || {
        observations += 1;
        if observations == 1 {
            signal.mark_changed();
            Ok(false)
        } else {
            post_command_idle(true, true)
        }
    });
    assert!(result.unwrap_err().contains("foreign user turn"));
    assert!(post_command_idle(true, false).is_err());
}

#[test]
fn deadline_and_closed_event_stream_do_not_claim_idle() {
    let signal = ScreenChangeSignal::default();
    assert!(wait_for_idle(&signal, Instant::now(), || Ok(true))
        .unwrap_err()
        .contains("deadline"));
    let mut screens = crate::pty::screen_state::ScreenState::new();
    let signal = screens.subscribe_screen_changes("closed-command-pty");
    screens.remove("closed-command-pty");
    assert!(
        wait_for_idle(&signal, Instant::now() + Duration::from_secs(2), || Ok(
            false
        ))
        .unwrap_err()
        .contains("closed")
    );
}
