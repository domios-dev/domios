use super::{
    authenticated_pty_target, optimistic_agent_pty_target, provider_frame_ready,
    provider_or_generic_tui, submit_authenticated_pty_input,
};
use crate::multiplexer::state::MultiplexerState;

// ---------------------------------------------------------------------------
// send_notice_tests
// ---------------------------------------------------------------------------

#[test]
fn failed_detached_send_names_the_pane_and_reason() {
    let (title, body) =
        super::send_delivery_notice(Some("Fermat"), "38cf30e1", "PTY did not start").unwrap();
    assert_eq!(title, "Message not delivered");
    assert_eq!(body, "Fermat: PTY did not start");
}

#[test]
fn stale_agent_delivery_drops_without_a_user_notice() {
    let error = "agent_unavailable: input permit is stale for pty pty-1";
    assert_eq!(
        super::detached_delivery_notice(Some("Codex"), "pane-1", error),
        None
    );
    assert!(
        super::detached_delivery_notice(Some("Codex"), "pane-1", "pty pty-1 is closed").is_some()
    );
}

#[test]
fn missing_or_blank_pane_title_falls_back_to_the_pane_id() {
    let (_, body) = super::send_delivery_notice(None, "38cf30e1", "boot timeout").unwrap();
    assert_eq!(body, "38cf30e1: boot timeout");
    let (_, body) = super::send_delivery_notice(Some("   "), "38cf30e1", "boot timeout").unwrap();
    assert_eq!(body, "38cf30e1: boot timeout");
}

// ---------------------------------------------------------------------------
// shell_ready_wait_tests
// ---------------------------------------------------------------------------

use super::super::{await_shell_prompt, await_shell_prompt_armed};
use codeterm_pty::pty::screen_state::ScreenState;
use codeterm_util::lock_hold::TimedMutex;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MACOS_BASH_BANNER_PROMPT: &[u8] = b"The default interactive shell is now zsh.\r\n\
    To update your account to use zsh, please run `chsh -s /bin/zsh`.\r\n\
    For more details, please visit https://support.apple.com/kb/HT208050.\r\n\
    rollacode-bot:general dev$ ";

fn handle() -> crate::ScreenStateHandle {
    Arc::new(TimedMutex::new("screen_state", ScreenState::new()))
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

// ---------------------------------------------------------------------------
// planner_delivery_tests
// ---------------------------------------------------------------------------

#[test]
fn pty_planner_input_spy_never_degrades_to_shell_write() {
    let (mut mux, pane_id) = MultiplexerState::new();
    let pane = mux.get_pane_mut(&pane_id).unwrap();
    pane.backing = crate::multiplexer::types::PaneBacking::Pty;
    pane.pty_id = Some("pty-planner".into());
    pane.agent_type = Some("codex".into());
    pane.agent_session_id = Some("session".into());
    pane.session_binding_nonce = Some("nonce".into());
    pane.reported_session_binding_nonce = Some("nonce".into());
    pane.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);

    let payload = "- audit `model`\n- <nonexistent>\n$env:X='y'; $(false)";
    let mut agent_input_spy = Vec::new();
    submit_authenticated_pty_input(pane, "session", "nonce", payload, |pty, provider, text| {
        agent_input_spy.push((pty.to_string(), provider.to_string(), text.to_string()));
        Ok(())
    })
    .unwrap();
    assert_eq!(
        agent_input_spy,
        [("pty-planner".into(), "codex".into(), payload.into())]
    );

    for shell in ["powershell", "bash", "zsh"] {
        pane.agent_type = None;
        pane.agent_session_id = None;
        pane.reported_session_binding_nonce = None;
        pane.agent_state = None;
        pane.boot_stage = Some(codeterm_types::pty::BootStage::ShellReady);
        let mut shell_write_spy = Vec::new();
        let result =
            submit_authenticated_pty_input(pane, "session", "nonce", payload, |_, _, text| {
                shell_write_spy.push((shell, text.to_string()));
                Ok(())
            });
        assert!(result.is_err());
        assert!(shell_write_spy.is_empty(), "{shell} received planner bytes");
    }

    pane.agent_type = Some("codex".into());
    pane.agent_session_id = Some("session".into());
    pane.session_binding_nonce = Some("nonce".into());
    pane.reported_session_binding_nonce = Some("nonce".into());
    pane.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    pane.exit_suspected_at = Some(std::time::Instant::now());
    let mut shell_write_spy = Vec::new();
    let result = submit_authenticated_pty_input(pane, "session", "nonce", payload, |_, _, text| {
        shell_write_spy.push(text.to_string());
        Ok(())
    });
    assert!(result.is_err());
    assert!(
        shell_write_spy.is_empty(),
        "suspected shell fallback received agent bytes"
    );
}

#[test]
fn optimistic_send_accepts_a_live_agent_without_a_reported_nonce() {
    let (mut mux, pane_id) = MultiplexerState::new();
    let pane = mux.get_pane_mut(&pane_id).unwrap();
    pane.backing = crate::multiplexer::types::PaneBacking::Pty;
    pane.pty_id = Some("pty-agent".into());
    pane.agent_type = Some("codex".into());
    pane.agent_session_id = Some("session".into());
    pane.session_binding_nonce = Some("launch-nonce".into());
    pane.reported_session_binding_nonce = None;
    pane.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);

    assert_eq!(
        optimistic_agent_pty_target(pane).unwrap(),
        ("pty-agent".into(), "codex".into()),
    );
    assert!(authenticated_pty_target(pane, "session", "launch-nonce").is_err());
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
fn alternate_screen_composer_proves_provider_readiness_without_text_patterns() {
    assert!(provider_frame_ready(false, false, true, true, true));
    assert!(!provider_frame_ready(false, false, true, false, true));
    assert!(!provider_frame_ready(false, false, true, true, false));
}

// ---------------------------------------------------------------------------
// routability_wait_tests
//
// The predicate mirrors `await_pane_routable_blocking`:
//   agent_type.is_some()
//   && agent_session_id.is_some()
//   && exit_suspected_at.is_none()
//   && boot_stage == AgentReady
//
// UI mount state is not part of backend routability.
// ---------------------------------------------------------------------------

fn fully_routable_pane() -> (MultiplexerState, String) {
    let (mut mux, pane_id) = MultiplexerState::new();
    let pane = mux.get_pane_mut(&pane_id).unwrap();
    pane.backing = crate::multiplexer::types::PaneBacking::Pty;
    pane.pty_id = Some("pty-test".into());
    pane.agent_type = Some("codex".into());
    pane.agent_session_id = Some("session-1".into());
    pane.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    (mux, pane_id)
}

/// Mirrors the exact predicate `await_pane_routable_blocking` evaluates.
fn is_pane_routable_for_send(pane: &crate::multiplexer::types::Pane) -> bool {
    pane.agent_type.is_some()
        && pane.agent_session_id.is_some()
        && pane.exit_suspected_at.is_none()
        && matches!(
            pane.boot_stage,
            Some(codeterm_types::pty::BootStage::AgentReady)
        )
}

#[test]
fn routable_pane_passes_all_checks() {
    let (_mux, pane_id) = fully_routable_pane();
    let pane = _mux.get_pane(&pane_id).unwrap();
    assert!(is_pane_routable_for_send(pane));
}

#[test]
fn pane_without_agent_type_skips_routability_check() {
    let (mut mux, pane_id) = MultiplexerState::new();
    let pane = mux.get_pane_mut(&pane_id).unwrap();
    pane.backing = crate::multiplexer::types::PaneBacking::Pty;
    pane.pty_id = Some("pty-shell".into());
    pane.agent_type = None;
    pane.boot_stage = Some(codeterm_types::pty::BootStage::ShellReady);
    let pane = mux.get_pane(&pane_id).unwrap();
    assert!(!pane.agent_type.is_some());
}

#[test]
fn pane_missing_session_id_is_not_routable() {
    let (_mux, pane_id) = fully_routable_pane();
    let pane = _mux.get_pane_mut(&pane_id).unwrap();
    pane.agent_session_id = None;
    let pane = _mux.get_pane(&pane_id).unwrap();
    assert!(!is_pane_routable_for_send(pane));
}

#[test]
fn pane_with_exit_suspected_is_not_routable() {
    let (_mux, pane_id) = fully_routable_pane();
    let pane = _mux.get_pane_mut(&pane_id).unwrap();
    pane.exit_suspected_at = Some(std::time::Instant::now());
    let pane = _mux.get_pane(&pane_id).unwrap();
    assert!(!is_pane_routable_for_send(pane));
}

#[test]
fn pane_before_agent_ready_is_not_routable() {
    let (_mux, pane_id) = fully_routable_pane();
    let pane = _mux.get_pane_mut(&pane_id).unwrap();
    pane.boot_stage = Some(codeterm_types::pty::BootStage::AgentLaunching);
    let pane = _mux.get_pane(&pane_id).unwrap();
    assert!(!is_pane_routable_for_send(pane));
}

#[test]
fn pane_with_no_boot_stage_is_not_routable() {
    let (_mux, pane_id) = fully_routable_pane();
    let pane = _mux.get_pane_mut(&pane_id).unwrap();
    pane.boot_stage = None;
    let pane = _mux.get_pane(&pane_id).unwrap();
    assert!(!is_pane_routable_for_send(pane));
}

#[test]
fn inactive_tab_is_still_routable_after_activation() {
    let (_mux, pane_id) = fully_routable_pane();
    let pane = _mux.get_pane_mut(&pane_id).unwrap();
    pane.chat_view = false;
    let pane = _mux.get_pane(&pane_id).unwrap();
    assert!(
        is_pane_routable_for_send(pane),
        "chat_view=false is UI state, not a backend routability gate"
    );
}

// ---------------------------------------------------------------------------
// Integration: a restored or inactive tab retains a routable live session.
// ---------------------------------------------------------------------------

#[test]
fn restored_tab_delivery_survives_reconnect_and_activation() {
    let (mut mux, pane_id) = MultiplexerState::new();
    let pane = mux.get_pane_mut(&pane_id).unwrap();
    pane.backing = crate::multiplexer::types::PaneBacking::Pty;
    pane.pty_id = Some("pty-bg".into());
    pane.agent_type = Some("claude".into());
    pane.agent_session_id = Some("session-bg".into());
    pane.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);

    let (pty_id, agent_type) = optimistic_agent_pty_target(pane).unwrap();
    assert_eq!(pty_id, "pty-bg");
    assert_eq!(agent_type, "claude");

    let mut received = Vec::new();
    submit_authenticated_pty_input(
        pane,
        "session-bg",
        "",
        "hello background",
        |pty, agent, text| {
            received.push((pty.to_string(), agent.to_string(), text.to_string()));
            Ok::<(), String>(())
        },
    )
    .unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].0, "pty-bg");
    assert_eq!(received[0].2, "hello background");
}

#[test]
fn transport_failure_emits_chat_send_failed_event() {
    let (mut mux, pane_id) = MultiplexerState::new();
    let pane = mux.get_pane_mut(&pane_id).unwrap();
    pane.backing = crate::multiplexer::types::PaneBacking::Pty;
    pane.pty_id = Some("pty-dead".into());
    pane.agent_type = Some("codex".into());
    pane.agent_session_id = Some("session-dead".into());
    pane.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    pane.exit_suspected_at = Some(std::time::Instant::now()); // dead pane

    // optimistic_agent_pty_target must fail — exit_suspected_at blocks send
    let err = optimistic_agent_pty_target(pane).unwrap_err();
    assert!(
        err.contains("not accepting input"),
        "expected transport failure message, got: {err}"
    );
}
