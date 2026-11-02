use super::*;
use std::sync::mpsc;

const TEST_DEADLINE: Duration = Duration::from_secs(5);

#[test]
fn general_and_restored_launches_wait_for_provider_writes() {
    let (release, released) = mpsc::channel();
    let (events, observed) = mpsc::channel();
    let cli_events = events.clone();
    let cli = std::thread::spawn(move || {
        released.recv().unwrap();
        cli_events.send("cli-written").unwrap();
    });
    let (hooks_release, hooks_released) = mpsc::channel();
    let startup = std::thread::spawn(move || {
        let hook_events = events.clone();
        let rule_events = events.clone();
        let _complete = complete_until(
            Some(cli),
            [("test integration".into(), move |deadline: Instant| {
                if hooks_released
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .is_err()
                {
                    return false;
                }
                hook_events.send("hooks-written").unwrap();
                true
            })],
            move || {
                rule_events.send("rules-written").unwrap();
            },
            Instant::now() + TEST_DEADLINE,
            |writer, failure| panic!("unexpected failure: {writer}: {failure}"),
        );
        events.send("general-spawn").unwrap();
        events.send("restore-spawn").unwrap();
    });
    assert!(observed.try_recv().is_err());
    release.send(()).unwrap();
    assert_eq!(observed.recv_timeout(TEST_DEADLINE).unwrap(), "cli-written");
    assert!(observed.try_recv().is_err());
    hooks_release.send(()).unwrap();
    for expected in [
        "hooks-written",
        "rules-written",
        "general-spawn",
        "restore-spawn",
    ] {
        assert_eq!(observed.recv_timeout(TEST_DEADLINE).unwrap(), expected);
    }
    startup.join().unwrap();
}

#[test]
fn failed_provider_writer_is_recorded_and_startup_continues_through_remaining_writers() {
    let cli = std::thread::spawn(|| panic!("opaque-install-failure"));
    let (events, observed) = mpsc::channel();
    let rule_events = events.clone();
    let mut failures = Vec::new();
    let deadline = Instant::now() + TEST_DEADLINE;
    let _complete = complete_until(
        Some(cli),
        [("test integration".into(), move |received_deadline| {
            assert_eq!(received_deadline, deadline);
            events.send("hooks-written").unwrap();
            true
        })],
        move || {
            rule_events.send("rules-written").unwrap();
        },
        deadline,
        |writer, failure| failures.push((writer.to_string(), failure.to_string())),
    );
    assert_eq!(observed.try_recv().unwrap(), "hooks-written");
    assert_eq!(observed.try_recv().unwrap(), "rules-written");
    assert_eq!(
        failures,
        vec![(CLI_WRITER.into(), "writer panicked".into())]
    );
}

#[test]
fn stalled_writers_share_one_injected_deadline_and_startup_continues_with_named_failures() {
    let (cli_release, cli_released) = mpsc::channel();
    let (rules_release, rules_released) = mpsc::channel();
    let (finished, finished_events) = mpsc::channel();
    let cli_finished = finished.clone();
    let cli = std::thread::spawn(move || {
        cli_released.recv().unwrap();
        cli_finished.send(CLI_WRITER).unwrap();
    });
    let deadline = Instant::now();
    let mut failures = Vec::new();
    let _complete = complete_until(
        Some(cli),
        [("stalled integration".into(), move |received_deadline| {
            assert_eq!(received_deadline, deadline);
            false
        })],
        move || {
            rules_released.recv().unwrap();
            finished.send(RULES_WRITER).unwrap();
        },
        deadline,
        |writer, failure| failures.push((writer.to_string(), failure.to_string())),
    );
    assert_eq!(
        failures
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec![CLI_WRITER, "stalled integration", RULES_WRITER]
    );
    assert!(failures
        .iter()
        .all(|(_, failure)| failure.contains("deadline elapsed")));
    cli_release.send(()).unwrap();
    rules_release.send(()).unwrap();
    for _ in 0..2 {
        finished_events.recv_timeout(TEST_DEADLINE).unwrap();
    }
}

#[test]
fn failed_instruction_writer_is_recorded_without_aborting_startup() {
    let mut failures = Vec::new();
    let _complete = complete_until(
        None,
        std::iter::empty::<(String, fn(Instant) -> bool)>(),
        || panic!("opaque-instruction-failure"),
        Instant::now() + TEST_DEADLINE,
        |writer, failure| failures.push((writer.to_string(), failure.to_string())),
    );
    assert_eq!(
        failures,
        vec![(RULES_WRITER.into(), "writer panicked".into())]
    );
}
