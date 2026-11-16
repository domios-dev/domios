use super::*;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use std::cell::Cell;

#[test]
fn only_a_declared_signal_requires_its_terminal_mode() {
    for mode in [None, Some(false), Some(true)] {
        assert!(input_attached(InputAttachSignal::None, mode));
    }
    assert!(!input_attached(InputAttachSignal::BracketedPaste, None));
    assert!(!input_attached(
        InputAttachSignal::BracketedPaste,
        Some(false)
    ));
    assert!(input_attached(
        InputAttachSignal::BracketedPaste,
        Some(true)
    ));
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
    signal: InputAttachSignal,
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
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new("attach-fixture").with_input_attach_signal(signal),
    ));
    fixture
        .service
        .screen_state
        .process("attach-pty", b"resumed history");
    (fixture, tab_id)
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

#[test]
fn declared_attach_holds_agent_ready_until_the_provider_enables_bracketed_paste() {
    let (fixture, tab_id) = attach_fixture(InputAttachSignal::BracketedPaste);

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
    let ready =
        crate::service::boot_wait::wait_until(Instant::now() + Duration::from_secs(10), || {
            boot_stage(&fixture, &tab_id) == Some(BootStage::AgentReady)
        });
    assert!(ready, "the provider's own attach event latches AgentReady");
}

#[test]
fn undeclared_provider_reaches_agent_ready_on_binding_alone() {
    let (fixture, tab_id) = attach_fixture(InputAttachSignal::None);
    fixture
        .service
        .emit_boot_stage(&tab_id, BootStage::AgentReady, None);
    assert_eq!(boot_stage(&fixture, &tab_id), Some(BootStage::AgentReady));
}
