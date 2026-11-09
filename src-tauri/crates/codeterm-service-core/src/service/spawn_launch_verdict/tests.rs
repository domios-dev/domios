use super::*;

fn classify_spawn_launch(tab: Option<&Tab>) -> Option<SpawnLaunchVerdict> {
    super::classify_spawn_launch(tab, tab.is_some_and(|tab| tab.is_chat_backend()))
}

fn register_runtime(fixture: &ServiceFixture, tab_id: &str) {
    let capability = serde_json::from_value(serde_json::json!({
        "runtime":"stdio-jsonrpc", "command":{"binary":"opaque","args":[]},
        "requestMethods":{"initialize":["initialize"]},"notificationMethods":[]
    }))
    .unwrap();
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("opaque-runtime")
            .with_agent_runtime(capability),
    ));
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(tab_id)
        .unwrap()
        .agent_type = Some("opaque-runtime".into());
}

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
    // A PTY translates newlines to CRLF; a bare LF would stair-step the emulated screen.
    let mut screen: String = (0..40)
        .map(|line| format!("\x1b[31mline-{line}\x1b[0m\r\n"))
        .collect();
    screen.push_str("ANTHROPIC_API_KEY=sk-fixture-secret\r\n\r\n\r\n   \r\n");

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
    assert!(data.get("launch_state").is_none());
}

#[test]
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

#[test]
fn runtime_spawn_requires_own_bound_session_and_reports_pending_without_waiting() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
        plugin_id: "opaque-runtime".into(),
        session_id: String::new(),
    };
    tab.agent_booting = false;
    for state in [None, Some("loading"), Some("loaded"), Some("loginPending")] {
        tab.agent_runtime_state =
            state.map(|state| serde_json::json!({"state": state, "epoch": 1}));
        assert_eq!(
            classify_spawn_launch(Some(tab)),
            Some(SpawnLaunchVerdict::Pending)
        );
    }
    tab.agent_runtime_state = Some(serde_json::json!({"state":"loaded","epoch":1}));
    tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
        plugin_id: "opaque-runtime".into(),
        session_id: "own-session".into(),
    };
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("stale-nonce".into());
    assert_eq!(
        classify_spawn_launch(Some(tab)),
        Some(SpawnLaunchVerdict::Pending)
    );
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    assert_eq!(
        classify_spawn_launch(Some(tab)),
        Some(SpawnLaunchVerdict::Bound)
    );
    assert_eq!(SpawnLaunchVerdict::Bound.launch_state(), "bound");
    tab.agent_runtime_state = Some(
        serde_json::json!({"state":"degraded","reason":"Provider refused: protocol_error","epoch":1}),
    );
    let fixture = ServiceFixture::new(mux);
    register_runtime(&fixture, &tab_id);
    let failure = fixture
        .service
        .await_spawn_launch_verdict(&tab_id, Duration::from_secs(60))
        .unwrap_err();
    assert_eq!(failure.reason, "Provider refused: protocol_error");
    assert_eq!(failure.data()["reason"], "Provider refused: protocol_error");
    assert_eq!(failure.data()["launch_state"], "failed");
    assert!(!failure.data()["retry_hint"]
        .as_str()
        .unwrap()
        .contains("exited"));
}

#[test]
fn runtime_spawn_pending_is_a_snapshot_even_with_a_long_verdict_deadline() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
        plugin_id: "opaque-runtime".into(),
        session_id: String::new(),
    };
    tab.agent_runtime_state = Some(serde_json::json!({"state":"loading","epoch":1}));
    let fixture = ServiceFixture::new(mux);
    register_runtime(&fixture, &tab_id);
    let verdict = fixture
        .service
        .await_spawn_launch_verdict(&tab_id, Duration::from_secs(60))
        .unwrap();
    assert_eq!(verdict.launch_state(), "pending");
    assert!(verdict.warning(&tab_id).is_some());
}

#[test]
fn pty_spawn_verdict_ignores_runtime_state_and_keeps_existing_ready_label() {
    let mut tab = tab_at(None, false, None);
    tab.agent_runtime_state =
        Some(serde_json::json!({"state":"degraded","reason":"opaque failure"}));
    let verdict = classify_spawn_launch(Some(&tab)).unwrap();
    assert_eq!(verdict, SpawnLaunchVerdict::Ready);
    assert_eq!(verdict.launch_state(), "ready");
}

#[test]
fn non_runtime_chat_backend_retains_the_existing_boot_stage_verdict() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
        plugin_id: "plain-chat".into(),
        session_id: "".into(),
    };
    tab.agent_type = Some("plain-chat".into());
    tab.agent_booting = false;
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("plain-chat"),
    ));
    assert_eq!(
        fixture
            .service
            .await_spawn_launch_verdict(&id, Duration::ZERO)
            .unwrap(),
        SpawnLaunchVerdict::Ready
    );
}

#[test]
fn runtime_deadline_is_pending_for_degraded_and_unloaded_snapshots() {
    for unload in [false, true] {
        let (mut mux, id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: "opaque-runtime".into(),
            session_id: "".into(),
        };
        let mut state = crate::service::agent_runtime::model::RuntimeState::unloaded(None);
        let epoch = state.begin();
        if unload {
            state.unload(
                epoch,
                &crate::service::agent_runtime::model::RuntimeFailure::Deadline,
            );
        } else {
            state.degrade(
                epoch,
                &crate::service::agent_runtime::model::RuntimeFailure::Deadline,
            );
        }
        tab.agent_runtime_state = Some(serde_json::to_value(state.snapshot()).unwrap());
        let fixture = ServiceFixture::new(mux);
        register_runtime(&fixture, &id);
        assert_eq!(
            fixture
                .service
                .await_spawn_launch_verdict(&id, Duration::ZERO)
                .unwrap(),
            SpawnLaunchVerdict::Pending
        );
    }
}
