use super::*;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::integration_silence::facts_from_status;
use std::cell::Cell;

#[test]
fn only_a_declared_signal_requires_its_terminal_mode() {
    for mode in [None, Some(false), Some(true)] {
        for started in [
            SessionStart::Awaited,
            SessionStart::Reported,
            SessionStart::HooksUnavailable,
        ] {
            assert!(input_ready(InputAttachSignal::None, mode, started));
            assert_eq!(
                input_ready(InputAttachSignal::BracketedPaste, mode, started),
                mode == Some(true),
                "a bracketed-paste provider never waits for a session start"
            );
        }
    }
}

#[test]
fn session_start_declaring_provider_needs_its_hook_and_bracketed_paste_in_either_order() {
    let signal = InputAttachSignal::SessionStartAndBracketedPaste;
    assert!(!input_ready(signal, None, SessionStart::Awaited));
    assert!(!input_ready(signal, Some(true), SessionStart::Awaited));
    assert!(input_ready(signal, Some(true), SessionStart::Reported));
    assert!(!input_ready(signal, None, SessionStart::Reported));
    assert!(!input_ready(signal, Some(false), SessionStart::Reported));
    assert!(input_ready(signal, Some(true), SessionStart::Reported));
}

#[test]
fn hooks_unavailable_at_launch_leave_readiness_to_the_input_attach() {
    let signal = InputAttachSignal::SessionStartAndBracketedPaste;
    assert!(!input_ready(signal, None, SessionStart::HooksUnavailable));
    assert!(input_ready(
        signal,
        Some(true),
        SessionStart::HooksUnavailable
    ));
}

#[test]
fn session_start_evidence_belongs_to_the_launch_nonce_it_carried() {
    assert_eq!(session_start(None, "launch-2"), SessionStart::Awaited);
    let previous_launch = TabSessionStart {
        reported_nonce: Some("launch-1".into()),
        hookless_nonce: Some("launch-1".into()),
    };
    assert_eq!(
        session_start(Some(&previous_launch), "launch-2"),
        SessionStart::Awaited,
        "a hook or hookless verdict from the previous launch proves nothing now"
    );
    let reported = TabSessionStart {
        reported_nonce: Some("launch-2".into()),
        hookless_nonce: None,
    };
    assert_eq!(
        session_start(Some(&reported), "launch-2"),
        SessionStart::Reported
    );
    let hookless = TabSessionStart {
        reported_nonce: Some("launch-1".into()),
        hookless_nonce: Some("launch-2".into()),
    };
    assert_eq!(
        session_start(Some(&hookless), "launch-2"),
        SessionStart::HooksUnavailable
    );
}

#[test]
fn only_installed_session_hooks_can_report_a_session_start() {
    let facts =
        |status: serde_json::Value, registers: bool| facts_from_status(&status, registers, true);
    assert!(session_start_hooks_available(&facts(
        serde_json::json!({"session_start_hook": true}),
        true
    )));
    for (status, registers) in [
        (serde_json::json!({"session_start_hook": true}), false),
        (serde_json::json!({"session_start_hook": false}), true),
        (serde_json::json!({"status": "ok"}), true),
        (
            serde_json::json!({"hooks_installed": false, "hooks_owned": true, "hook_trust_state": "disabled"}),
            true,
        ),
    ] {
        assert!(
            !session_start_hooks_available(&facts(status.clone(), registers)),
            "{status} registers={registers}"
        );
    }
}

#[test]
fn attach_event_ends_the_wait_and_the_deadline_only_records_once() {
    let attached = Cell::new(false);
    let waits = Cell::new(Vec::new());
    let deadlines = Cell::new(0);
    let outcome = await_attach(
        || attached.get(),
        || true,
        |remaining| {
            let mut seen = waits.take();
            seen.push(remaining.is_some());
            waits.set(seen);
            attached.set(true);
            true
        },
        || deadlines.set(deadlines.get() + 1),
        Instant::now(),
    );
    assert_eq!(outcome, AttachOutcome::Attached);
    assert_eq!(
        deadlines.get(),
        1,
        "the elapsed deadline is recorded, never readiness"
    );
    assert_eq!(
        waits.take(),
        vec![false],
        "after the deadline only events wake the wait"
    );
}

#[test]
fn ended_launch_or_closed_output_supersedes_without_a_deadline_record() {
    let deadlines = Cell::new(0);
    let far = Instant::now() + Duration::from_secs(3600);
    assert_eq!(
        await_attach(|| false, || false, |_| true, || deadlines.set(1), far),
        AttachOutcome::Superseded
    );
    assert_eq!(
        await_attach(|| false, || true, |_| false, || deadlines.set(1), far),
        AttachOutcome::Superseded
    );
    assert_eq!(deadlines.get(), 0);
}

fn attach_fixture(
    provider: MockProvider,
) -> (crate::service::test_fixture::ServiceFixture, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("attach-fixture".into());
    tab.agent_session_id = Some("own-session".into());
    tab.pty_id = Some("attach-pty".into());
    tab.session_binding_nonce = Some("attach-nonce".into());
    tab.reported_session_binding_nonce = Some("attach-nonce".into());
    tab.boot_stage = Some(BootStage::ProviderReady);
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    fixture
        .service
        .screen_state
        .process("attach-pty", b"resumed history");
    (fixture, tab_id)
}

fn declaring(signal: InputAttachSignal) -> MockProvider {
    MockProvider::new("attach-fixture").with_input_attach_signal(signal)
}

fn boot_stage(
    fixture: &crate::service::test_fixture::ServiceFixture,
    tab_id: &str,
) -> Option<BootStage> {
    fixture
        .service
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.boot_stage)
}

fn becomes_ready(fixture: &crate::service::test_fixture::ServiceFixture, tab_id: &str) -> bool {
    crate::service::boot_wait::wait_until(Instant::now() + Duration::from_secs(10), || {
        boot_stage(fixture, tab_id) == Some(BootStage::AgentReady)
    })
}

#[test]
fn declared_attach_holds_agent_ready_until_the_provider_enables_bracketed_paste() {
    let (fixture, tab_id) = attach_fixture(declaring(InputAttachSignal::BracketedPaste));

    fixture
        .service
        .emit_boot_stage(&tab_id, BootStage::AgentReady, None);
    assert_eq!(
        boot_stage(&fixture, &tab_id),
        Some(BootStage::ProviderReady)
    );

    fixture
        .service
        .screen_state
        .process("attach-pty", b"\x1b[?2004h");
    assert!(
        becomes_ready(&fixture, &tab_id),
        "the provider's own attach event latches AgentReady"
    );
}

#[test]
fn launch_without_session_hooks_becomes_ready_on_the_input_attach_alone() {
    let (fixture, tab_id) =
        attach_fixture(declaring(InputAttachSignal::SessionStartAndBracketedPaste));
    fixture
        .service
        .screen_state
        .process("attach-pty", b"\x1b[?2004h");

    fixture
        .service
        .emit_boot_stage(&tab_id, BootStage::AgentReady, None);
    assert!(
        becomes_ready(&fixture, &tab_id),
        "no hook can arrive, so the launch behaves as a bracketed-paste launch"
    );
}

#[test]
fn launch_with_session_hooks_waits_for_its_own_session_start_report() {
    let (fixture, tab_id) = attach_fixture(
        declaring(InputAttachSignal::SessionStartAndBracketedPaste)
            .with_hooks()
            .with_integration_status(serde_json::json!({"session_start_hook": true})),
    );
    fixture
        .service
        .screen_state
        .process("attach-pty", b"\x1b[?2004h");

    fixture
        .service
        .emit_boot_stage(&tab_id, BootStage::AgentReady, None);
    assert_eq!(
        boot_stage(&fixture, &tab_id),
        Some(BootStage::ProviderReady)
    );
    fixture
        .service
        .observe_session_start_hook(&tab_id, "previous-launch-nonce");
    assert_eq!(
        boot_stage(&fixture, &tab_id),
        Some(BootStage::ProviderReady),
        "a report from another launch proves nothing"
    );

    fixture
        .service
        .observe_session_start_hook(&tab_id, "attach-nonce");
    assert!(becomes_ready(&fixture, &tab_id));
}

#[test]
fn undeclared_provider_reaches_agent_ready_on_binding_alone() {
    let (fixture, tab_id) = attach_fixture(declaring(InputAttachSignal::None));
    fixture
        .service
        .emit_boot_stage(&tab_id, BootStage::AgentReady, None);
    assert_eq!(boot_stage(&fixture, &tab_id), Some(BootStage::AgentReady));
}
