use super::readiness::{enter_decision, EnterDecision};
use super::*;
use crate::agent::test_provider::MockProvider;
use codeterm_plugin::tab_state::{TabState, TabStateClassification, TabStateSnapshot};

struct Composer {
    status: Option<TabStatus>,
    claim: SubmitCycleClaim,
    payloads: RefCell<Vec<String>>,
    keys: Cell<usize>,
}

impl SubmitTransport for Composer {
    fn observe(&self) -> Option<Observation> {
        None
    }
    fn draft_evidence_count(&self, _: &str) -> usize {
        self.payloads.borrow().len()
    }
    fn generation(&self) -> u64 {
        0
    }
    fn wait_change(&self, _: u64, _: Instant) -> bool {
        false
    }
    fn turn_started(&self) -> bool {
        self.status.is_some_and(TabStatus::has_active_turn)
    }
    fn exact_user_turn_present(&self, text: &str) -> bool {
        self.keys.get() > 0 && self.payloads.borrow().iter().any(|payload| payload == text)
    }
    fn retry_active(&self) -> bool {
        self.claim.is_current()
    }
    fn write_paste(&self, text: &str, _: Instant) -> Result<usize, InjectionError> {
        self.payloads.borrow_mut().push(text.into());
        let (sender, _) = mpsc::sync_channel(1);
        self.claim.payload_enqueued(sender);
        Ok(text.len())
    }
    fn write_submit_key(&self, key: &[u8], _: Instant) -> Result<(), InjectionError> {
        assert_eq!(key, b"\r");
        self.keys.set(self.keys.get() + 1);
        Ok(())
    }
}

fn composer(status: Option<TabStatus>, pty: &str) -> Composer {
    Composer {
        status,
        claim: claim_submit_cycle(pty),
        payloads: RefCell::default(),
        keys: Cell::new(0),
    }
}

fn snapshot(screen: &str) -> TabStateSnapshot {
    TabStateSnapshot {
        tab_id: "tab".into(),
        agent_type: "fixture".into(),
        session_id: Some("session".into()),
        cwd: None,
        screen_text: screen.into(),
    }
}

fn classified(state: TabState, confidence: f32) -> MockProvider {
    MockProvider::new("fixture").with_tab_state(TabStateClassification {
        state,
        confidence,
        data: None,
        action: None,
    })
}

fn send_guarded(
    input: &Composer,
    provider: &MockProvider,
    screen: &str,
) -> Result<InjectionResult, InjectionError> {
    let snapshot = snapshot(screen);
    run_with_submit_key(
        input,
        "opaque-owner-message",
        true,
        b"\r",
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay: Duration::ZERO,
            retry_offsets: [Duration::ZERO; 6],
        },
        VerifyOpts {
            require_prompt_first: false,
            verify_echo: false,
            verify_submit: true,
            ..Default::default()
        },
        |_| recovery_dialog_guard(provider, &snapshot, input.claim.pty_id()),
    )
}

fn assert_sent(input: &Composer, result: InjectionResult) {
    assert_eq!(*input.payloads.borrow(), ["opaque-owner-message"]);
    assert_eq!(result.submit_key_writes, 1);
    assert!(result.submit_observed);
}

#[test]
fn only_a_confident_prompt_withholds_enter() {
    assert_eq!(enter_decision(false), EnterDecision::Submit);
    assert_eq!(enter_decision(true), EnterDecision::WithholdForPrompt);
}

#[test]
fn busy_tab_sends_payload_and_enter() {
    for (status, pty) in [
        (Some(TabStatus::Working), "modal-free-busy"),
        (None, "modal-free-unknown"),
        (Some(TabStatus::Idle), "modal-free-idle-stuck"),
    ] {
        let input = composer(status, pty);
        let result = send_guarded(&input, &classified(TabState::Working, 0.0), "busy").unwrap();
        assert_sent(&input, result);
    }
}

#[test]
fn uncertain_classifier_sends() {
    for state in [
        TabState::ClarifyingQuestion,
        TabState::WaitingOnAuth,
        TabState::UpdatePrompt,
        TabState::RateLimited,
    ] {
        let input = composer(Some(TabStatus::Idle), "modal-uncertain");
        let result = send_guarded(&input, &classified(state, 0.79), "menu").unwrap();
        assert_sent(&input, result);
    }
}

#[test]
fn false_menu_cursor_sends() {
    let history = "> 1. first\n  2. second\n\n> filled composer text\n";
    let prompt_like =
        MockProvider::new("fixture").with_parsed_prompt(codeterm_types::pty::PendingPrompt {
            request_id: None,
            question: "Choose".into(),
            options: Vec::new(),
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        });
    let input = composer(Some(TabStatus::Idle), "modal-false-cursor");
    let result = send_guarded(&input, &prompt_like, history).unwrap();
    assert_sent(&input, result);
}

#[test]
fn real_numbered_prompt_writes_payload_once_and_withholds_enter() {
    for state in [TabState::ClarifyingQuestion, TabState::WaitingOnAuth] {
        let input = composer(Some(TabStatus::Working), "modal-real-prompt");
        let error =
            send_guarded(&input, &classified(state, 0.8), "Do you want to proceed?").unwrap_err();
        assert!(matches!(
            error,
            InjectionError::SubmitWithheldForPrompt { .. }
        ));
        assert_eq!(*input.payloads.borrow(), ["opaque-owner-message"]);
        assert_eq!(input.keys.get(), 0);
        let delivery = crate::service::delivery_error::DeliveryError::from_injection("tab", error);
        assert_eq!(delivery.code(), "submit_unconfirmed");
        assert!(delivery.bytes_written());
    }
}
