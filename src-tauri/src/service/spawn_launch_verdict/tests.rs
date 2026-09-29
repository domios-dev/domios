use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::launch_readiness::EXITED_BEFORE_READINESS;
use crate::service::test_fixture::ServiceFixture;

fn tab_at(stage: Option<BootStage>, booting: bool, reason: Option<&str>) -> Tab {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.boot_stage = stage;
    tab.agent_booting = booting;
    tab.boot_failure_reason = reason.map(str::to_string);
    tab.clone()
}

fn booting_fixture() -> (ServiceFixture, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.boot_stage = Some(BootStage::LaunchWritten);
    tab.agent_booting = true;
    (ServiceFixture::new(mux), tab_id)
}

#[test]
fn a_launch_verdict_is_read_from_the_tabs_own_boot_stage() {
    let failed = tab_at(Some(BootStage::SpawnFailed), false, Some("reason-9d1"));
    assert_eq!(
        classify_spawn_launch(Some(&failed)),
        Some(SpawnLaunchVerdict::Failed {
            reason: "reason-9d1".into()
        })
    );
    assert!(matches!(
        classify_spawn_launch(None),
        Some(SpawnLaunchVerdict::Failed { .. })
    ));
    assert_eq!(
        classify_spawn_launch(Some(&tab_at(Some(BootStage::AgentReady), false, None))),
        Some(SpawnLaunchVerdict::Ready)
    );
    for stage in [
        BootStage::AwaitingOwnerPrompt,
        BootStage::LaunchDeadlineElapsed,
        BootStage::LaunchBindingUnconfirmed,
    ] {
        assert_eq!(
            classify_spawn_launch(Some(&tab_at(Some(stage), true, None))),
            Some(SpawnLaunchVerdict::Starting { stage: Some(stage) })
        );
    }
    for stage in [
        BootStage::PtyStarting,
        BootStage::LaunchWritten,
        BootStage::ProviderReady,
    ] {
        assert_eq!(
            classify_spawn_launch(Some(&tab_at(Some(stage), true, None))),
            None,
            "{stage:?} is still on its way to a verdict"
        );
    }
    assert_eq!(
        classify_spawn_launch(Some(&tab_at(None, false, None))),
        Some(SpawnLaunchVerdict::Ready),
        "a tab with no launch in flight has nothing to wait for"
    );
}

#[test]
fn output_tail_keeps_the_last_bounded_lines_without_escapes_or_secrets() {
    let mut screen: String = (0..40)
        .map(|line| format!("\x1b[31mline-{line}\x1b[0m\n"))
        .collect();
    screen.push_str("ANTHROPIC_API_KEY=sk-fixture-secret\n\n\n   \n");

    let tail = output_tail(&screen).expect("printed output has a tail");

    assert!(tail.lines().count() <= OUTPUT_TAIL_LINES);
    assert!(tail.lines().any(|line| line == "line-39"));
    assert!(!tail.contains("line-0\n"));
    assert!(!tail.contains('\x1b'));
    assert!(!tail.contains("sk-fixture-secret"));
    assert!(tail.len() <= OUTPUT_TAIL_BYTES);
    assert_eq!(output_tail("\n  \n"), None);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn spawn_reports_pre_ready_exit_as_error() {
    let (fixture, tab_id) = booting_fixture();
    let exit_reason = format!("{EXITED_BEFORE_READINESS}: tab={tab_id} exit_code=1");
    let waiter = {
        let service = fixture.service.clone();
        let tab_id = tab_id.clone();
        std::thread::spawn(move || {
            service.await_spawn_launch_verdict(&tab_id, Duration::from_secs(60))
        })
    };

    fixture
        .service
        .emit_boot_stage(&tab_id, BootStage::SpawnFailed, Some(exit_reason.as_str()));

    let failure = waiter
        .join()
        .unwrap()
        .expect_err("a provider that exits before readiness fails the spawn");
    assert_eq!(failure.tab_id, tab_id);
    assert_eq!(failure.reason, exit_reason);
    assert!(failure.message().contains(&exit_reason));
    let data = failure.data();
    assert_eq!(data["type"], SpawnLaunchFailure::CODE);
    assert_eq!(data["tab_id"], tab_id);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn spawn_verdict_waits_for_readiness_and_never_calls_a_deadline_ready() {
    let (fixture, tab_id) = booting_fixture();

    let pending = fixture
        .service
        .await_spawn_launch_verdict(&tab_id, Duration::ZERO)
        .expect("an unfinished launch is not a failure");
    assert_eq!(
        pending,
        SpawnLaunchVerdict::Starting {
            stage: Some(BootStage::LaunchWritten)
        }
    );
    assert!(pending.warning(&tab_id).is_some());

    let waiter = {
        let service = fixture.service.clone();
        let tab_id = tab_id.clone();
        std::thread::spawn(move || {
            service.await_spawn_launch_verdict(&tab_id, Duration::from_secs(60))
        })
    };
    fixture
        .service
        .emit_boot_stage(&tab_id, BootStage::AgentReady, None);
    assert_eq!(waiter.join().unwrap(), Ok(SpawnLaunchVerdict::Ready));
}
