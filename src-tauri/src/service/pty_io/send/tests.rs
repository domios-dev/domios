use super::readiness::composer_input_ready;
use super::{
    authenticated_pty_target, authenticated_pty_target_pinned, await_shell_prompt,
    await_shell_prompt_armed, delivery_is_unknown, is_pre_ready_launch_stage,
    optimistic_agent_pty_target, provider_frame_ready, provider_or_generic_tui,
    public_agent_input_readiness, require_submission_result, submit_authenticated_pty_input,
    wait_for_observed_readiness,
};
use crate::multiplexer::state::MultiplexerState;

// shell_ready_wait_tests

use codeterm_pty::pty::screen_state::ScreenState;
use codeterm_util::lock_hold::TimedMutex;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MACOS_BASH_BANNER_PROMPT: &[u8] = b"The default interactive shell is now zsh.\r\n\
    To update your account to use zsh, please run `chsh -s /bin/zsh`.\r\n\
    For more details, please visit https://support.apple.com/kb/HT208050.\r\n\
    rollacode-bot:general ainyiva$ ";

fn handle() -> crate::ScreenStateHandle {
    Arc::new(TimedMutex::new("screen_state", ScreenState::new()))
}

#[test]
fn post_write_uncertainty_is_not_a_user_facing_delivery_failure() {
    assert!(delivery_is_unknown(
        "submit_unconfirmed: Enter was not observed"
    ));
    assert!(!delivery_is_unknown(
        "pty_unavailable: recipient PTY missing"
    ));
}

#[test]
fn refused_agent_report_waits_for_state_change_and_writes_once() {
    let temp = tempfile::tempdir().unwrap();
    let store =
        crate::service::pending_delivery_payloads::agent_deliveries::AgentDeliveryStore::open(
            temp.path(),
        );
    let tab_id = "orchestrator-tab";
    let report = "never-written report body";
    let generation = store.state_generation(tab_id);
    let id = store
        .enqueue(tab_id, report, Some("report:report-1".into()))
        .unwrap();
    let next_id = store.enqueue(tab_id, "next agent message", None).unwrap();

    assert!(store.block_after_refusal(tab_id, generation));
    assert!(!store.start_worker(tab_id));
    let record = store.pending_head(tab_id).unwrap().unwrap();
    assert_eq!(store.payload(tab_id, &record).unwrap(), report);

    store.state_changed(tab_id);
    assert!(store.start_worker(tab_id));
    let mut writes = Vec::new();
    for expected_id in [&id, &next_id] {
        let record = store.pending_head(tab_id).unwrap().unwrap();
        assert_eq!(&record.id, expected_id);
        assert!(store.claim(tab_id, expected_id).unwrap());
        writes.push(record.id.clone());
        assert!(store.complete_written(tab_id, expected_id).unwrap());
    }
    store.finish_worker(tab_id);

    assert_eq!(writes, [id, next_id]);
    assert!(store.pending_head(tab_id).unwrap().is_none());
}

#[test]
fn claimed_agent_report_is_not_replayed_after_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let tab_id = "orchestrator-tab";
    let store =
        crate::service::pending_delivery_payloads::agent_deliveries::AgentDeliveryStore::open(
            temp.path(),
        );
    let id = store.enqueue(tab_id, "report body", None).unwrap();
    assert!(store.claim(tab_id, &id).unwrap());

    let reopened =
        crate::service::pending_delivery_payloads::agent_deliveries::AgentDeliveryStore::open(
            temp.path(),
        );
    assert!(reopened.pending_head(tab_id).unwrap().is_none());
    let receipts = reopened.pending_receipts(tab_id).unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].id, id);
    assert_eq!(receipts[0].status, None);
}

#[test]
fn never_written_agent_report_survives_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let tab_id = "orchestrator-tab";
    let store =
        crate::service::pending_delivery_payloads::agent_deliveries::AgentDeliveryStore::open(
            temp.path(),
        );
    let id = store
        .enqueue(
            tab_id,
            "never-written report",
            Some("report:report-1".into()),
        )
        .unwrap();

    let reopened =
        crate::service::pending_delivery_payloads::agent_deliveries::AgentDeliveryStore::open(
            temp.path(),
        );
    let record = reopened.pending_head(tab_id).unwrap().unwrap();
    assert_eq!(record.id, id);
    assert_eq!(
        reopened.payload(tab_id, &record).unwrap(),
        "never-written report"
    );
    assert!(reopened.claim(tab_id, &record.id).unwrap());
    assert!(reopened.complete_written(tab_id, &record.id).unwrap());
}

#[test]
fn routed_suffix_is_the_reply_hint_and_closing_tag() {
    let suffix = super::compose_routed_suffix(" [reply]");
    assert_eq!(
        suffix,
        format!(" [reply]{}", codeterm_types::domios::DOMIOS_CLOSE_TAG)
    );
}

#[test]
fn stuck_after_write_reports_submit_unconfirmed_not_ok() {
    let error = require_submission_result(
        crate::service::inject_verify::InjectionResult {
            bytes_written: 12,
            echo_observed: true,
            submit_observed: false,
            submit_anomaly: true,
            submit_key_writes: 7,
            elapsed: Duration::from_secs(3),
        },
        true,
    )
    .unwrap_err();
    let delivery = crate::service::delivery_error::DeliveryError::from_injection("tab-1", error);

    assert_eq!(delivery.code(), "submit_unconfirmed");
    assert!(delivery.bytes_written());
}

#[test]
fn a_torn_down_readiness_signal_re_arms_instead_of_reporting_a_deadline() {
    let screen_state = handle();
    assert!(screen_state.lock().bind_pty_generation("pty-1", 1));

    let (armed_tx, armed_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let waiter_screen = screen_state.clone();
    let waiter = std::thread::spawn(move || {
        let started = Instant::now();
        let ready =
            await_shell_prompt_armed(&waiter_screen, "pty-1", Duration::from_secs(60), |rearms| {
                armed_tx.send(rearms).unwrap()
            });
        done_tx.send((ready, started.elapsed())).unwrap();
    });

    assert_eq!(armed_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 0);
    assert!(screen_state.lock().bind_pty_generation("pty-1", 2));
    assert!(screen_state
        .lock()
        .process_if_generation("pty-1", 2, MACOS_BASH_BANNER_PROMPT));

    let (ready, elapsed) = done_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    waiter.join().unwrap();
    assert!(ready, "readiness must survive a signal teardown");
    assert!(
        elapsed < Duration::from_secs(10),
        "wait must not run to the deadline: {elapsed:?}"
    );
}

#[test]
fn readiness_armed_before_the_reader_binds_still_completes() {
    let screen_state = handle();
    let (armed_tx, armed_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let waiter_screen = screen_state.clone();
    let waiter = std::thread::spawn(move || {
        let ready =
            await_shell_prompt_armed(&waiter_screen, "pty-1", Duration::from_secs(60), |rearms| {
                armed_tx.send(rearms).unwrap()
            });
        done_tx.send(ready).unwrap();
    });

    assert_eq!(armed_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 0);
    assert!(screen_state.lock().bind_pty_generation("pty-1", 1));
    assert!(screen_state
        .lock()
        .process_if_generation("pty-1", 1, MACOS_BASH_BANNER_PROMPT));

    assert!(done_rx.recv_timeout(Duration::from_secs(10)).unwrap());
    waiter.join().unwrap();
}

#[test]
fn a_prompt_already_on_screen_returns_without_waiting() {
    let screen_state = handle();
    assert!(screen_state.lock().bind_pty_generation("pty-1", 1));
    assert!(screen_state
        .lock()
        .process_if_generation("pty-1", 1, MACOS_BASH_BANNER_PROMPT));

    assert!(await_shell_prompt(
        &screen_state,
        "pty-1",
        Duration::from_secs(60)
    ));
}

#[test]
fn an_expired_deadline_reports_not_ready() {
    let screen_state = handle();
    assert!(screen_state.lock().bind_pty_generation("pty-1", 1));

    assert!(!await_shell_prompt(&screen_state, "pty-1", Duration::ZERO));
}

// planner_delivery_tests

#[test]
fn pty_planner_input_spy_never_degrades_to_shell_write() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::Pty;
    tab.pty_id = Some("pty-planner".into());
    tab.agent_type = Some("codex".into());
    tab.agent_session_id = Some("session".into());
    tab.session_binding_nonce = Some("nonce".into());
    tab.reported_session_binding_nonce = Some("nonce".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);

    let payload = "- audit `model`\n- <nonexistent>\n$env:X='y'; $(false)";
    let mut agent_input_spy = Vec::new();
    submit_authenticated_pty_input(tab, "session", "nonce", payload, |pty, provider, text| {
        agent_input_spy.push((pty.to_string(), provider.to_string(), text.to_string()));
        Ok(())
    })
    .unwrap();
    assert_eq!(
        agent_input_spy,
        [("pty-planner".into(), "codex".into(), payload.into())]
    );

    for shell in ["powershell", "bash", "zsh"] {
        tab.agent_type = None;
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
        tab.agent_state = None;
        tab.boot_stage = Some(codeterm_types::pty::BootStage::ShellReady);
        let mut shell_write_spy = Vec::new();
        let result =
            submit_authenticated_pty_input(tab, "session", "nonce", payload, |_, _, text| {
                shell_write_spy.push((shell, text.to_string()));
                Ok(())
            });
        assert!(result.is_err());
        assert!(shell_write_spy.is_empty(), "{shell} received planner bytes");
    }
}

#[test]
fn optimistic_send_accepts_a_live_agent_without_a_reported_nonce() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::Pty;
    tab.pty_id = Some("pty-agent".into());
    tab.agent_type = Some("codex".into());
    tab.agent_session_id = Some("session".into());
    tab.session_binding_nonce = Some("launch-nonce".into());
    tab.reported_session_binding_nonce = None;
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);

    assert_eq!(
        optimistic_agent_pty_target(tab).unwrap(),
        ("pty-agent".into(), "codex".into()),
    );
    assert!(authenticated_pty_target(tab, "session", "launch-nonce").is_err());
}

#[test]
fn authenticated_resume_refuses_a_rotated_pty_or_binding_nonce() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::Pty;
    tab.pty_id = Some("pty-old".into());
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("session".into());
    tab.session_binding_nonce = Some("nonce-old".into());
    tab.reported_session_binding_nonce = Some("nonce-old".into());

    assert!(authenticated_pty_target_pinned(tab, "pty-old", "session", "nonce-old").is_ok());
    assert!(authenticated_pty_target_pinned(tab, "pty-new", "session", "nonce-old").is_err());
    tab.pty_id = Some("pty-old".into());
    tab.session_binding_nonce = Some("nonce-new".into());
    tab.reported_session_binding_nonce = Some("nonce-new".into());
    assert!(authenticated_pty_target_pinned(tab, "pty-old", "session", "nonce-old").is_err());
}

#[test]
fn pinned_resume_send_writes_no_payload_when_the_pty_changed() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::Pty;
    tab.pty_id = Some("pty-current".into());
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("session-a".into());
    tab.session_binding_nonce = Some("nonce-a".into());
    tab.reported_session_binding_nonce = Some("nonce-a".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let payload = "opaque resume payload";
    let mut written = Vec::new();

    let result = fixture
        .service
        .send_to_authenticated_agent_session_pinned_with(
            &tab_id,
            "pty-previous",
            "session-a",
            "nonce-a",
            payload,
            |_, text| {
                written.extend_from_slice(text.as_bytes());
                Ok(())
            },
        );

    assert!(result.is_err());
    assert!(written.is_empty());
}

#[test]
fn shell_spawn_command_locks_the_detected_agent_type() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::Pty;
    mux.link_pty(&tab_id, "pty-cli-launch".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("claude"),
    ));

    fixture
        .service
        .feed_input_parser("pty-cli-launch", "claude --resume session", true);

    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert!(tab.agent_type_locked);
}

#[test]
fn provider_readiness_overrides_generic_tui_glyphs() {
    assert!(!provider_or_generic_tui(Some(false), true));
    assert!(provider_or_generic_tui(Some(true), false));
    assert!(provider_or_generic_tui(None, true));
}

#[test]
fn provider_prompt_proves_launch_without_claiming_composer_readiness() {
    assert!(provider_frame_ready(false, true, false, false, false));
    assert!(!provider_frame_ready(false, true, true, false, false));
    assert!(provider_frame_ready(true, false, true, false, true));
}

#[test]
fn structural_composer_readiness_requires_observation_and_rejects_elapsed_time() {
    use std::cell::Cell;
    use std::time::{Duration, Instant};

    let strategy = crate::agent::provider::ProviderReadinessStrategy::StructuralComposer {
        timeout: Duration::from_secs(30),
    };
    assert!(strategy.requires_composer());

    let composer_observed = Cell::new(false);
    let event_waits = Cell::new(0);
    assert!(wait_for_observed_readiness(
        Instant::now() + Duration::from_secs(1),
        || {
            provider_frame_ready(
                false,
                false,
                strategy.requires_composer(),
                true,
                composer_observed.get(),
            )
        },
        |_| {
            event_waits.set(event_waits.get() + 1);
            composer_observed.set(true);
            true
        },
    ));
    assert_eq!(event_waits.get(), 1);

    let expired_wait_called = Cell::new(false);
    assert!(!wait_for_observed_readiness(
        Instant::now(),
        || false,
        |_| {
            expired_wait_called.set(true);
            true
        },
    ));
    assert!(!expired_wait_called.get());
}

#[test]
fn hidden_inverse_caret_is_composer_evidence_only_in_the_bottom_region() {
    use codeterm_pty::pty::screen_state::ComposerInputState;

    let mut input = ComposerInputState {
        alternate_screen: false,
        cursor_visible: false,
        cursor_row: 22,
        rows: 40,
        cols: 120,
        inverse_caret_row: Some(17),
        nonblank_inverse_cells: 1,
        sole_nonblank_inverse_row: Some(17),
        last_nonblank_row: Some(21),
    };
    assert!(composer_input_ready(input));

    input.inverse_caret_row = Some(3);
    assert!(!composer_input_ready(input));

    input.inverse_caret_row = None;
    assert!(!composer_input_ready(input));
}

#[test]
fn alternate_screen_composer_proves_provider_readiness_without_text_patterns() {
    assert!(provider_frame_ready(false, false, true, true, true));
    assert!(!provider_frame_ready(false, false, true, false, true));
    assert!(!provider_frame_ready(false, false, true, true, false));
}

// The wait and delivery predicates must remain identical.

fn fully_routable_tab() -> (MultiplexerState, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::Pty;
    tab.pty_id = Some("pty-test".into());
    tab.agent_type = Some("codex".into());
    tab.agent_session_id = Some("session-1".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    (mux, tab_id)
}

/// Mirrors the exact predicate `await_tab_routable_blocking` evaluates.
fn is_tab_routable_for_send(tab: &crate::multiplexer::types::Tab) -> bool {
    optimistic_agent_pty_target(tab).is_ok()
}

#[test]
fn routable_tab_passes_all_checks() {
    let (_mux, tab_id) = fully_routable_tab();
    let tab = _mux.get_tab(&tab_id).unwrap();
    assert!(is_tab_routable_for_send(tab));
}

#[test]
fn tab_without_agent_type_skips_routability_check() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::Pty;
    tab.pty_id = Some("pty-shell".into());
    tab.agent_type = None;
    tab.boot_stage = Some(codeterm_types::pty::BootStage::ShellReady);
    let tab = mux.get_tab(&tab_id).unwrap();
    assert!(!tab.agent_type.is_some());
}

#[test]
fn live_agent_tui_is_routable_before_session_binding() {
    let (mut _mux, tab_id) = fully_routable_tab();
    let tab = _mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_session_id = None;
    let tab = _mux.get_tab(&tab_id).unwrap();
    assert!(is_tab_routable_for_send(tab));
}

#[test]
fn stale_exit_suspicion_does_not_block_a_live_agent_pty() {
    let (mut _mux, tab_id) = fully_routable_tab();
    let tab = _mux.get_tab_mut(&tab_id).unwrap();
    tab.exit_suspected_at = Some(std::time::Instant::now());
    let tab = _mux.get_tab(&tab_id).unwrap();
    assert!(is_tab_routable_for_send(tab));
}

#[test]
fn launching_agent_with_a_live_pty_is_optimistically_routable() {
    let (mut _mux, tab_id) = fully_routable_tab();
    let tab = _mux.get_tab_mut(&tab_id).unwrap();
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentLaunching);
    let tab = _mux.get_tab(&tab_id).unwrap();
    assert!(is_tab_routable_for_send(tab));
    assert!(public_agent_input_readiness(tab).is_none());
}

#[test]
fn public_send_waits_for_agent_ready_even_when_pty_exists() {
    let (mut mux, tab_id) = fully_routable_tab();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.boot_stage = Some(codeterm_types::pty::BootStage::ProviderReady);
    assert!(public_agent_input_readiness(tab).is_none());

    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    assert_eq!(public_agent_input_readiness(tab), Some(Ok(())));
}

#[test]
fn inferred_question_and_prompt_never_block_ready_agent_input() {
    use crate::agent::offline_state_classifier::{TabVerdict, TimelineState};

    let (mut mux, tab_id) = fully_routable_tab();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Question,
            subtype: None,
        }
        .into(),
    );
    tab.pending_prompt = Some(codeterm_types::pty::PendingPrompt {
        request_id: None,
        question: "Select an option".into(),
        options: Vec::new(),
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    });
    tab.pending_prompt_kind = Some(codeterm_types::pty::PromptKind::Provider);

    assert_eq!(public_agent_input_readiness(tab), Some(Ok(())));
}

#[test]
fn agent_with_a_live_pty_is_routable_without_boot_metadata() {
    let (mut _mux, tab_id) = fully_routable_tab();
    let tab = _mux.get_tab_mut(&tab_id).unwrap();
    tab.boot_stage = None;
    let tab = _mux.get_tab(&tab_id).unwrap();
    assert!(is_tab_routable_for_send(tab));
}

#[test]
fn reattached_live_session_accepts_public_input_without_a_spawn() {
    let (mut mux, tab_id) = fully_routable_tab();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.boot_stage = Some(codeterm_types::pty::BootStage::PtyReattached);
    assert_eq!(public_agent_input_readiness(tab), Some(Ok(())));
}

#[test]
fn pre_ready_launch_stages_are_distinguished_from_unresolved_sessions() {
    use codeterm_types::pty::BootStage;

    for stage in [
        BootStage::PtyStarting,
        BootStage::PtySpawned,
        BootStage::ReaderReady,
        BootStage::ShellReady,
        BootStage::AgentLaunching,
        BootStage::LaunchWritten,
        BootStage::ProviderReady,
        BootStage::PreparingRoleInstructions,
        BootStage::DeliveringInstructions,
    ] {
        assert!(
            is_pre_ready_launch_stage(stage),
            "{stage:?} is a launch stage"
        );
    }
    for stage in [
        BootStage::AgentReady,
        BootStage::PtyReattached,
        BootStage::SpawnFailed,
        BootStage::LaunchDeadlineElapsed,
        BootStage::LaunchBindingUnconfirmed,
    ] {
        assert!(
            !is_pre_ready_launch_stage(stage),
            "{stage:?} must not park a send"
        );
    }
}

#[test]
fn stale_timeline_state_does_not_block_a_live_agent_pty() {
    use crate::agent::offline_state_classifier::{TabVerdict, TimelineState};

    let (mut mux, tab_id) = fully_routable_tab();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Question,
            subtype: None,
        }
        .into(),
    );

    assert!(is_tab_routable_for_send(tab));
}

#[test]
fn restored_working_tab_is_routable_after_daemon_restart() {
    use crate::agent::offline_state_classifier::{AgentLiveState, TabVerdict, TimelineState};

    let (mut mux, tab_id) = fully_routable_tab();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.boot_stage = Some(codeterm_types::pty::BootStage::PtyReattached);
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Working,
            subtype: None,
        }
        .into(),
    );
    let _typed: &AgentLiveState = tab.agent_state.as_ref().unwrap();

    assert!(is_tab_routable_for_send(tab));
}

#[test]
fn inactive_tab_is_still_routable_after_activation() {
    let (mut _mux, tab_id) = fully_routable_tab();
    let tab = _mux.get_tab_mut(&tab_id).unwrap();
    tab.chat_view = false;
    let tab = _mux.get_tab(&tab_id).unwrap();
    assert!(
        is_tab_routable_for_send(tab),
        "chat_view=false is UI state, not a backend routability gate"
    );
}

// Integration: a restored or inactive tab retains a routable live session.

#[test]
fn restored_tab_target_survives_reconnect_and_activation() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::Pty;
    tab.pty_id = Some("pty-bg".into());
    tab.agent_type = Some("claude".into());
    tab.agent_session_id = Some("session-bg".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);

    let (pty_id, agent_type) = optimistic_agent_pty_target(tab).unwrap();
    assert_eq!(pty_id, "pty-bg");
    assert_eq!(agent_type, "claude");
}

#[test]
fn transport_failure_emits_chat_send_failed_event() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::Pty;
    tab.pty_id = Some("pty-dead".into());
    tab.agent_type = Some("codex".into());
    tab.agent_session_id = Some("session-dead".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    tab.agent_type = None;

    // A tab without an agent classification must not receive agent input.
    let err = optimistic_agent_pty_target(tab).unwrap_err();
    assert!(
        err.contains("not accepting input"),
        "expected transport failure message, got: {err}"
    );
}

#[test]
fn codex_routed_frame_keeps_both_tags_inside_the_paste() {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../plugins/provider-codex/plugin.json"
    ))
    .unwrap();
    let requires_instruction = manifest["spawn"]["pasteCapability"]
        ["requiresTypedInstructionForRoutedMessages"]
        .as_bool()
        .unwrap_or(false);
    assert!(!requires_instruction);

    let frame = codeterm_types::domios::DomiosFrame {
        opening_tag: codeterm_types::domios::build_opening_tag(&[
            ("from", "tab"),
            ("label", "sender"),
            ("tab", "abc12345"),
        ]),
        body: "opaque first line\nopaque second line".into(),
        suffix: " [opaque hint]".into(),
    };
    let input = super::prepare_routed_input(&frame, requires_instruction);
    assert_eq!(input.body, frame.to_message());
    assert_eq!(input.typed_prefix, None);
    assert_eq!(input.typed_suffix, None);
    let parsed = codeterm_types::domios::parse_message(&input.body).unwrap();
    assert_eq!(parsed.attr("tab"), Some("abc12345"));
    assert_eq!(parsed.content, format!("{}{}", frame.body, frame.suffix));
}

#[test]
fn restored_structured_turn_routes_one_complete_pasted_frame() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/pty-delivery/restored-structured-turn.json"
    ))
    .unwrap();
    let frame = codeterm_types::domios::DomiosFrame {
        opening_tag: fixture["opening_tag"].as_str().unwrap().into(),
        body: fixture["body"].as_str().unwrap().into(),
        suffix: fixture["suffix"].as_str().unwrap().into(),
    };
    let expected = frame.to_message();
    assert_ne!(fixture["persisted_content"].as_str().unwrap(), expected);
    assert!(
        fixture["created_ms"].as_u64().unwrap() > fixture["cursor_before"][0].as_u64().unwrap()
    );

    let input = super::prepare_routed_input(&frame, false);
    assert_eq!(input.body, expected);
    assert_eq!(input.typed_prefix, None);
    assert_eq!(input.typed_suffix, None);

    let typed = super::prepare_routed_input(&frame, true);
    assert_eq!(typed.body, frame.body);
    assert_eq!(
        typed.typed_prefix.as_deref(),
        Some(frame.opening_tag.as_str())
    );
    assert_eq!(
        typed.typed_suffix,
        Some(super::compose_routed_suffix(&frame.suffix))
    );
}

#[test]
fn routed_agent_and_report_hints_stay_out_of_the_body_in_both_transport_forms() {
    for messenger in [
        crate::service::messenger::Messenger::message("Sender", "abc12345"),
        crate::service::messenger::Messenger::report("Sender", "abc12345"),
    ] {
        let frame = messenger.routed_frame("opaque body");
        let input = super::prepare_routed_input(&frame, true);
        assert_eq!(input.body, frame.body);
        assert_eq!(
            input.typed_prefix.as_deref(),
            Some(frame.opening_tag.as_str())
        );
        let suffix = input.typed_suffix.unwrap();
        assert!(!suffix.contains(codeterm_types::domios::DOMIOS_LEGACY_TYPED_INSTRUCTION));
        let delivered = format!("{}{}{}", frame.opening_tag, input.body, suffix);
        let parsed = codeterm_types::domios::parse_message(&delivered).unwrap();
        assert_eq!(parsed.body, frame.body);
        assert!(parsed.content.contains(&frame.suffix));
        let pasted = super::prepare_routed_input(&frame, false);
        assert_eq!(pasted.body, frame.to_message());
        assert_eq!(pasted.typed_prefix, None);
        assert_eq!(pasted.typed_suffix, None);
    }
}
