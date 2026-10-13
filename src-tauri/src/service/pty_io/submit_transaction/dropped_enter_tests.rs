//! WHAT: dropped-Enter recovery at the transport and ledger seams.
//! WHY: a provider that drops one CR while the draft stays parked in the
//! composer must be recovered key-only within the turn deadline; a genuine
//! loss keeps its honest submit_unconfirmed classification.
//! ORACLE: recorded writes, ledger state, and the exact error variant.
use super::tests::{opts, tui};
use super::*;
use std::cell::{Cell, RefCell};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// Models the captured production trace (prod log 2026-07-30, tab 166df18d):
/// paste ingested and rendered, Enter written and dropped by the TUI, draft
/// still parked in the composer at the turn deadline. `wait_change` honors the
/// production contract of `PtySubmitTransport`: with no new frame it returns
/// false only once the passed failure deadline has expired.
struct ParkedDraft {
    drops_remaining: Cell<usize>,
    starts_turn: bool,
    draft_visible: Cell<bool>,
    turn: Cell<bool>,
    generation: Cell<u64>,
    turn_generation: Cell<u64>,
    payload_writes: Cell<usize>,
    submit_key_writes: Cell<usize>,
    idle: Mutex<()>,
    never_signaled: Condvar,
}

impl ParkedDraft {
    fn dropping(drops: usize) -> Self {
        Self {
            drops_remaining: Cell::new(drops),
            starts_turn: true,
            draft_visible: Cell::new(false),
            turn: Cell::new(false),
            generation: Cell::new(0),
            turn_generation: Cell::new(0),
            payload_writes: Cell::new(0),
            submit_key_writes: Cell::new(0),
            idle: Mutex::new(()),
            never_signaled: Condvar::new(),
        }
    }

    fn lifecycle_only_drain() -> Self {
        Self {
            starts_turn: false,
            ..Self::dropping(0)
        }
    }
}

impl SubmitTransport for ParkedDraft {
    fn observe(&self) -> Option<Observation> {
        let tail = if self.turn.get() {
            "• Working (5s • esc to interrupt)".to_string()
        } else if self.draft_visible.get() {
            "› run /review on my current changes".to_string()
        } else {
            "› ".to_string()
        };
        Some(Observation { tail })
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        usize::from(self.draft_visible.get())
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn wait_change(&self, generation: u64, deadline: Instant) -> bool {
        if self.generation.get() != generation {
            return true;
        }
        let mut guard = self.idle.lock().unwrap();
        loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return self.generation.get() != generation;
            };
            let (next, timeout) = self.never_signaled.wait_timeout(guard, remaining).unwrap();
            guard = next;
            if self.generation.get() != generation {
                return true;
            }
            if timeout.timed_out() {
                return false;
            }
        }
    }

    fn turn_started(&self) -> bool {
        self.turn.get()
    }

    fn turn_generation(&self) -> u64 {
        self.turn_generation.get()
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.payload_writes.set(self.payload_writes.get() + 1);
        self.draft_visible.set(true);
        self.generation.set(self.generation.get() + 1);
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        self.submit_key_writes.set(self.submit_key_writes.get() + 1);
        if self.drops_remaining.get() > 0 {
            self.drops_remaining.set(self.drops_remaining.get() - 1);
            return Ok(());
        }
        self.draft_visible.set(false);
        if self.starts_turn {
            self.turn.set(true);
            self.turn_generation.set(self.turn_generation.get() + 1);
        } else {
            self.turn_generation.set(self.turn_generation.get() + 2);
        }
        self.generation.set(self.generation.get() + 1);
        Ok(())
    }
}

fn short_opts() -> VerifyOpts {
    VerifyOpts {
        echo_timeout: Duration::ZERO,
        submit_timeout: Duration::from_millis(250),
        ..opts()
    }
}

struct GuardTransport {
    writes: RefCell<Vec<String>>,
}

impl GuardTransport {
    fn new() -> Self {
        Self {
            writes: RefCell::new(Vec::new()),
        }
    }
}

impl SubmitTransport for GuardTransport {
    fn observe(&self) -> Option<Observation> {
        None
    }

    fn draft_evidence_count(&self, _text: &str) -> usize {
        0
    }

    fn generation(&self) -> u64 {
        0
    }

    fn wait_change(&self, _generation: u64, _deadline: Instant) -> bool {
        false
    }

    fn turn_started(&self) -> bool {
        false
    }

    fn write_paste(&self, text: &str, _deadline: Instant) -> Result<usize, InjectionError> {
        self.writes.borrow_mut().push(format!("paste:{text}"));
        Ok(text.len())
    }

    fn write_submit_key(&self, _key: &[u8], _deadline: Instant) -> Result<(), InjectionError> {
        self.writes.borrow_mut().push("enter".into());
        Ok(())
    }
}

#[test]
fn recovery_burst_stops_when_update_dialog_appears_after_initial_submit() {
    let io = GuardTransport::new();
    let mut guard_checks = 0;
    let result = run_with_submit_key(
        &io,
        "payload",
        true,
        b"\r",
        VerifySubmitStrategy::None,
        opts(),
        |shell_submit| {
            guard_checks += 1;
            if guard_checks == 1 {
                assert!(shell_submit);
                Ok(())
            } else {
                assert!(!shell_submit);
                Err(InjectionError::AgentTuiPresent {
                    pty_id: "same-pty".into(),
                })
            }
        },
    )
    .unwrap();

    assert_eq!(guard_checks, 2);
    assert_eq!(&*io.writes.borrow(), &["paste:payload", "enter"]);
    assert_eq!(result.bytes_written, 7);
    assert_eq!(result.submit_key_writes, 1);
    assert!(!result.submit_observed);
    assert!(result.submit_anomaly);
    let unconfirmed = result
        .clone()
        .require_submission(true, result.elapsed, "provider update dialog".into())
        .unwrap_err();
    let delivery =
        crate::service::delivery_error::DeliveryError::from_injection("same-pty", unconfirmed);
    assert_eq!(delivery.code(), "submit_unconfirmed");
    assert!(delivery.bytes_written());
}

#[test]
fn recovery_burst_stops_on_trust_dialog_after_shell_started_tui() {
    let io = GuardTransport::new();
    let mut guard_checks = 0;
    let result = run_with_submit_key(
        &io,
        "codex",
        true,
        b"\r",
        VerifySubmitStrategy::None,
        opts(),
        |shell_submit| {
            guard_checks += 1;
            if guard_checks == 1 {
                assert!(shell_submit);
                Ok(())
            } else {
                assert!(!shell_submit);
                Err(InjectionError::AgentTuiPresent {
                    pty_id: "same-pty".into(),
                })
            }
        },
    )
    .unwrap();

    assert_eq!(guard_checks, 2);
    assert_eq!(&*io.writes.borrow(), &["paste:codex", "enter"]);
    assert_eq!(result.submit_key_writes, 1);
    assert!(result.submit_anomaly);
}

#[test]
fn shell_submit_has_no_recovery_enter_schedule() {
    let io = GuardTransport::new();
    let mut guard_checks = 0;
    let result = run_with_submit_key(
        &io,
        "claude --model haiku",
        true,
        b"\r",
        VerifySubmitStrategy::None,
        opts(),
        |initial_key| {
            guard_checks += 1;
            if initial_key {
                Ok(())
            } else {
                Err(InjectionError::AgentInputUnavailable {
                    pty_id: "shell-pty".into(),
                })
            }
        },
    )
    .unwrap();

    assert_eq!(guard_checks, 2);
    assert_eq!(
        &*io.writes.borrow(),
        &["paste:claude --model haiku", "enter"]
    );
    assert_eq!(result.submit_key_writes, 1);
    assert!(result.submit_anomaly);
}

#[test]
fn readiness_deadline_fallback_rechecks_the_provider_guard() {
    let io = ParkedDraft::dropping(usize::MAX);
    let mut guard_checks = 0;
    let error = run_with_submit_key(
        &io,
        "payload",
        true,
        b"\r",
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay: Duration::ZERO,
            retry_offsets: crate::agent::provider::DEFAULT_SUBMIT_RETRY_OFFSETS,
        },
        short_opts(),
        |_initial_key| {
            guard_checks += 1;
            if guard_checks == 1 {
                Err(InjectionError::AgentInputUnavailable {
                    pty_id: "same-pty".into(),
                })
            } else {
                Err(InjectionError::AgentTuiPresent {
                    pty_id: "same-pty".into(),
                })
            }
        },
    )
    .unwrap_err();

    assert_eq!(guard_checks, 2);
    assert_eq!(io.payload_writes.get(), 1);
    assert_eq!(io.submit_key_writes.get(), 0);
    assert!(matches!(error, InjectionError::SubmitNeverConfirmed { .. }));
}

#[test]
fn plain_shell_send_succeeds_when_recovery_proof_is_absent() {
    let io = GuardTransport::new();
    let mut guard_checks = 0;
    let result = run_with_submit_key(
        &io,
        "ls",
        true,
        b"\r",
        VerifySubmitStrategy::None,
        opts(),
        |initial_key| {
            guard_checks += 1;
            if guard_checks == 1 {
                assert!(initial_key);
                Ok(())
            } else {
                assert!(!initial_key);
                Err(InjectionError::AgentInputUnavailable {
                    pty_id: "shell-pty".into(),
                })
            }
        },
    )
    .unwrap();

    assert_eq!(guard_checks, 2);
    assert_eq!(&*io.writes.borrow(), &["paste:ls", "enter"]);
    assert_eq!(result.submit_key_writes, 1);
}

#[test]
fn observed_initial_enter_stops_before_recovery_guard() {
    let io = ParkedDraft::dropping(0);
    let mut guard_checks = 0;
    let result = run_with_submit_key(&io, "codex", true, b"\r", tui(), opts(), |initial_key| {
        guard_checks += 1;
        assert!(initial_key);
        Ok(())
    })
    .unwrap();

    assert_eq!(guard_checks, 1);
    assert_eq!(io.payload_writes.get(), 1);
    assert_eq!(io.submit_key_writes.get(), 1);
    assert!(result.submit_observed);
    let elapsed = result.elapsed;
    assert!(result
        .require_submission(true, elapsed, String::new())
        .is_ok());
}

#[test]
fn empty_enter_guard_refusal_after_initial_key_is_not_a_new_error() {
    let io = GuardTransport::new();
    let mut guard_checks = 0;
    let result = run_with_submit_key(
        &io,
        "",
        true,
        b"\r",
        VerifySubmitStrategy::None,
        opts(),
        |initial_key| {
            guard_checks += 1;
            if guard_checks == 1 {
                assert!(initial_key);
                Ok(())
            } else {
                assert!(!initial_key);
                Err(InjectionError::AgentInputUnavailable {
                    pty_id: "shell-pty".into(),
                })
            }
        },
    )
    .unwrap();

    assert_eq!(guard_checks, 2);
    assert_eq!(&*io.writes.borrow(), &["paste:", "enter"]);
    assert_eq!(result.bytes_written, 0);
    assert_eq!(result.submit_key_writes, 1);
}

#[test]
fn recovery_keys_require_same_pty_proof() {
    let io = GuardTransport::new();
    let mut guard_checks = 0;
    let result = run_with_submit_key(
        &io,
        "payload",
        true,
        b"\r",
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay: Duration::from_millis(1),
            retry_offsets: crate::agent::provider::DEFAULT_SUBMIT_RETRY_OFFSETS,
        },
        opts(),
        |allow_initial_key| {
            guard_checks += 1;
            if guard_checks == 1 {
                assert!(allow_initial_key);
                Ok(())
            } else {
                assert!(!allow_initial_key);
                Err(InjectionError::AgentInputUnavailable {
                    pty_id: "same-pty".into(),
                })
            }
        },
    )
    .unwrap();

    assert_eq!(guard_checks, 2);
    assert_eq!(&*io.writes.borrow(), &["paste:payload", "enter"]);
    assert_eq!(result.submit_key_writes, 1);
    assert!(!result.submit_observed);
    let elapsed = result.elapsed;
    let error = result
        .require_submission(true, elapsed, "recovery proof absent".into())
        .unwrap_err();
    let delivery = crate::service::delivery_error::DeliveryError::from_injection("same-pty", error);
    assert_eq!(delivery.code(), "submit_unconfirmed");
    assert!(delivery.bytes_written());
}

fn assert_prompt_fixture_stops_recovery(
    label: &str,
    screen: &str,
    prompt: codeterm_types::pty::PendingPrompt,
) {
    use codeterm_plugin::tab_state::{TabState, TabStateClassification, TabStateSnapshot};

    let provider = crate::agent::test_provider::MockProvider::new("fixture")
        .with_tab_state(TabStateClassification {
            state: TabState::ClarifyingQuestion,
            confidence: 1.0,
            data: None,
            action: None,
        })
        .with_parsed_prompt(prompt);
    let snapshot = TabStateSnapshot {
        tab_id: "fixture-tab".into(),
        agent_type: "fixture".into(),
        session_id: Some("fixture-session".into()),
        cwd: Some("/fixture".into()),
        screen_text: screen.into(),
    };
    let io = GuardTransport::new();
    let result = run_with_submit_key(
        &io,
        "payload",
        true,
        b"\r",
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay: Duration::ZERO,
            retry_offsets: crate::agent::provider::DEFAULT_SUBMIT_RETRY_OFFSETS,
        },
        opts(),
        |initial_key| {
            if initial_key {
                Ok(())
            } else {
                recovery_dialog_guard(&provider, &snapshot, false, "same-pty")
            }
        },
    )
    .unwrap();

    assert_eq!(
        &*io.writes.borrow(),
        &["paste:payload", "enter"],
        "{label} prompt must cancel every remaining recovery key"
    );
    assert_eq!(result.bytes_written, 7);
    assert_eq!(result.submit_key_writes, 1);
    assert!(result.submit_anomaly);
}

#[test]
fn provider_prompt_fixtures_cancel_recovery_after_the_initial_submit() {
    use codeterm_plugin::provider::AgentProvider;

    let claude = codeterm_plugin::agent_claude::provider::ClaudeProvider::new();
    let trust = include_str!("../../../../../plugins/provider-claude/tests/fixtures/folder-trust/trust-continue-without-permissions.txt");
    let permission = include_str!(
        "../../../../../plugins/provider-claude/tests/fixtures/folder-trust/tool-permission.txt"
    );
    assert_prompt_fixture_stops_recovery(
        "Claude trust",
        trust,
        claude
            .parse_prompt(trust)
            .expect("Claude trust fixture parses"),
    );
    assert_prompt_fixture_stops_recovery(
        "Claude permission",
        permission,
        claude
            .parse_prompt(permission)
            .expect("Claude permission fixture parses"),
    );

    let model_picker =
        include_str!("../../../../../plugins/provider-codex/tests/fixtures/model-picker.txt");
    let (question, options) = codeterm_plugin::detector::parse_permission_prompt(model_picker)
        .expect("Codex model picker fixture parses");
    assert_prompt_fixture_stops_recovery(
        "Codex /model picker",
        model_picker,
        codeterm_plugin::prompt::build_prompt(question, options),
    );
}

#[test]
fn plain_unconfirmed_composer_keeps_its_bounded_key_schedule() {
    use codeterm_plugin::tab_state::{TabState, TabStateClassification, TabStateSnapshot};

    let provider = crate::agent::test_provider::MockProvider::new("fixture").with_tab_state(
        TabStateClassification {
            state: TabState::Working,
            confidence: 0.0,
            data: None,
            action: None,
        },
    );
    let snapshot = TabStateSnapshot {
        tab_id: "fixture-tab".into(),
        agent_type: "fixture".into(),
        session_id: Some("fixture-session".into()),
        cwd: Some("/fixture".into()),
        screen_text: "composer".into(),
    };
    let io = GuardTransport::new();
    let result = run_with_submit_key(
        &io,
        "payload",
        true,
        b"\r",
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay: Duration::ZERO,
            retry_offsets: crate::agent::provider::DEFAULT_SUBMIT_RETRY_OFFSETS,
        },
        opts(),
        |initial_key| {
            if initial_key {
                Ok(())
            } else {
                recovery_dialog_guard(&provider, &snapshot, false, "same-pty")
            }
        },
    )
    .unwrap();

    assert_eq!(result.submit_key_writes, MAX_SUBMIT_KEY_WRITES);
    assert_eq!(
        io.writes
            .borrow()
            .iter()
            .filter(|write| write.as_str() == "enter")
            .count(),
        MAX_SUBMIT_KEY_WRITES
    );
    assert_eq!(
        io.writes
            .borrow()
            .iter()
            .filter(|write| write.starts_with("paste:"))
            .count(),
        1
    );
    assert!(result.submit_anomaly);
}

#[test]
fn unbound_recovery_requires_a_provider_capability() {
    use codeterm_plugin::provider::AgentProvider;

    let provider = crate::agent::test_provider::MockProvider::new("fixture");
    assert!(!provider.supports_unbound_submit_recovery());
    assert!(!recovery_session_authorized(false, false));
    assert!(recovery_session_authorized(false, true));
    assert!(recovery_session_authorized(true, false));
}

#[test]
fn recovery_stops_on_provider_modal_without_using_screen_turn_state() {
    use codeterm_plugin::tab_state::{TabState, TabStateClassification, TabStateSnapshot};

    let snapshot = TabStateSnapshot {
        tab_id: "fixture-tab".into(),
        agent_type: "fixture".into(),
        session_id: Some("fixture-session".into()),
        cwd: Some("/fixture".into()),
        screen_text: "screen".into(),
    };
    let working = crate::agent::test_provider::MockProvider::new("fixture").with_tab_state(
        TabStateClassification {
            state: TabState::Working,
            confidence: 0.0,
            data: None,
            action: None,
        },
    );
    assert!(matches!(
        recovery_dialog_guard(&working, &snapshot, false, "same-pty"),
        Ok(())
    ));

    let prompt = crate::agent::test_provider::MockProvider::new("fixture").with_tab_state(
        TabStateClassification {
            state: TabState::ClarifyingQuestion,
            confidence: 1.0,
            data: None,
            action: None,
        },
    );
    assert!(matches!(
        recovery_dialog_guard(&prompt, &snapshot, false, "same-pty"),
        Err(InjectionError::AgentTuiPresent { .. })
    ));
    assert!(matches!(
        recovery_dialog_guard(&prompt, &snapshot, true, "same-pty"),
        Ok(())
    ));
}

#[test]
fn failed_agent_readiness_is_not_overridden_by_shell_prompt_text() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::delivery_error::DeliveryError;
    use crate::service::test_fixture::ServiceFixture;

    let (mut mux, tab_id) = MultiplexerState::new();
    let pty_id = "stale-agent-shell";
    mux.link_pty(&tab_id, pty_id.to_string());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("codex".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
    tab.boot_failure_reason = Some("agent exited".into());
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .screen_state
        .lock()
        .process(pty_id, b"PS C:\\Users\\Tester>");

    let error = fixture
        .service
        .send_text_input_delivery(&tab_id, "codex --no-daemon", true, None)
        .unwrap_err();

    assert!(matches!(error, DeliveryError::TabBusyRefused { .. }));
    assert!(!error.bytes_written());
}

#[test]
fn parked_draft_after_dropped_enter_recovers_before_turn_deadline() {
    let io = ParkedDraft::dropping(1);

    let result = run(
        &io,
        "run /review on my current changes",
        true,
        tui(),
        short_opts(),
    )
    .unwrap();

    assert_eq!(io.payload_writes.get(), 1, "the payload is written once");
    assert!(
        result.submit_observed,
        "a dropped Enter with the draft parked in the composer must be \
         recovered key-only within the turn deadline; writes={} anomaly={}",
        io.submit_key_writes.get(),
        result.submit_anomaly
    );
    assert!(io.turn.get(), "the recovered Enter starts a real turn");
    assert_eq!(io.submit_key_writes.get(), 2);
}

#[test]
fn restart_status_transitions_without_a_turn_do_not_acknowledge_submission() {
    let mut tracker = codeterm_types::pty::TabStatusTracker::new();
    tracker.update_output("pty-restart", [1; 32]);
    tracker.set_idle("pty-restart");
    let signal = tracker.arm_status_change_signal("pty-restart");
    let before_restart = signal.generation();
    tracker.set_dead("pty-restart");
    tracker.set_waiting("pty-restart");

    assert_eq!(
        signal.generation(),
        before_restart + 2,
        "restart lifecycle transitions advance the generic status signal"
    );
    let io = ParkedDraft::lifecycle_only_drain();

    let result = run(&io, "post-restart payload", true, tui(), short_opts()).unwrap();

    assert_eq!(io.payload_writes.get(), 1);
    assert!(io.turn_generation.get() >= 2);
    assert!(!result.submit_observed);
    assert!(result.submit_anomaly);
}

#[test]
fn recovered_submit_reaches_sent_only_after_exact_turn_evidence() {
    use crate::service::chat_send_transaction::{run_chat_send, ChatSendLedger, ChatSendState};
    use crate::service::delivery_error::DeliveryError;

    let ledger = ChatSendLedger::default();
    let pending = ledger
        .try_start("tab-1", "run /review on my current changes", None)
        .unwrap();
    let io = ParkedDraft::dropping(1);

    let status = run_chat_send(
        &ledger,
        pending,
        "run /review on my current changes",
        |text| {
            run(&io, text, true, tui(), short_opts())
                .and_then(|result| {
                    let elapsed = result.elapsed;
                    let tail = io.observe().map(|o| o.tail).unwrap_or_default();
                    result.require_submission(true, elapsed, tail)
                })
                .map(|_| ())
                .map_err(|error| DeliveryError::from_injection("tab-1", error))
        },
    );

    assert_eq!(io.payload_writes.get(), 1);
    assert_eq!(
        status.state,
        ChatSendState::Unknown,
        "transport success stays unknown without transcript evidence, got {:?}",
        status.error
    );
    let confirmed = ledger
        .confirm_delivered_user_turn("tab-1", "session-1", "run /review on my current changes")
        .unwrap();
    assert_eq!(confirmed.state, ChatSendState::Sent);
}

#[test]
fn unrecoverable_dropped_enter_stays_submit_unconfirmed_with_single_payload() {
    let io = ParkedDraft::dropping(usize::MAX);

    let result = run(&io, "held message", true, tui(), short_opts()).unwrap();

    assert_eq!(
        io.payload_writes.get(),
        1,
        "recovery is key-only; the payload is never re-pasted"
    );
    assert!(io.submit_key_writes.get() <= MAX_SUBMIT_KEY_WRITES);
    assert!(!result.submit_observed);
    assert!(result.submit_anomaly);

    let surfaced = result
        .require_submission(true, Duration::ZERO, "› held message".into())
        .unwrap_err();
    assert!(
        matches!(surfaced, InjectionError::SubmitNeverConfirmed { .. }),
        "a genuinely lost Enter keeps its honest classification, got {surfaced:?}"
    );
    let delivery = crate::service::delivery_error::DeliveryError::from_injection("tab-1", surfaced);
    assert_eq!(delivery.code(), "submit_unconfirmed");
    assert!(delivery.bytes_written());
}
