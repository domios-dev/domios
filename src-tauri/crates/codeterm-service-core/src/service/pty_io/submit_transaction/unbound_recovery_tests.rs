//! WHAT: recovery keys for a launch whose provider binds only on its first turn.
//! WHY: a restored Codex launch dropped its first Enter, the retry schedule
//! waited for a binding only that Enter could create, and the draft sat unsent.
//! ORACLE: the authority verdict, recorded key writes, and the refusal audit line.
use super::recovery_guard::{
    nudge_unbound_launch_draft, recovery_key_authority, recovery_refusal_line, RecoveryKeyInputs,
};
use super::tests::{opts, tui};
use super::*;
use std::cell::RefCell;

const UNBOUND_FIRST_TURN_DRAFT: RecoveryKeyInputs = RecoveryKeyInputs {
    session_bound: false,
    provider_capability: false,
    session_starts_on_first_user_turn: true,
    launch_owns_pty: true,
    draft_present: true,
};

#[test]
fn unbound_recovery_key_needs_a_first_turn_launch_with_its_draft_parked() {
    assert_eq!(recovery_key_authority(UNBOUND_FIRST_TURN_DRAFT), Ok(()));
    assert_eq!(
        recovery_key_authority(RecoveryKeyInputs {
            session_starts_on_first_user_turn: false,
            ..UNBOUND_FIRST_TURN_DRAFT
        }),
        Err("session_unbound")
    );
    assert_eq!(
        recovery_key_authority(RecoveryKeyInputs {
            launch_owns_pty: false,
            ..UNBOUND_FIRST_TURN_DRAFT
        }),
        Err("unbound_without_launch_owner")
    );
    assert_eq!(
        recovery_key_authority(RecoveryKeyInputs {
            draft_present: false,
            ..UNBOUND_FIRST_TURN_DRAFT
        }),
        Err("unbound_draft_absent")
    );
}

#[test]
fn a_bound_session_or_provider_capability_keeps_its_existing_authority() {
    let refused_shape = RecoveryKeyInputs {
        session_starts_on_first_user_turn: false,
        launch_owns_pty: false,
        draft_present: false,
        ..UNBOUND_FIRST_TURN_DRAFT
    };
    assert_eq!(
        recovery_key_authority(RecoveryKeyInputs {
            session_bound: true,
            ..refused_shape
        }),
        Ok(())
    );
    assert_eq!(
        recovery_key_authority(RecoveryKeyInputs {
            provider_capability: true,
            ..refused_shape
        }),
        Ok(())
    );
}

#[test]
fn refused_recovery_key_is_audited_once_per_reason() {
    let last = std::cell::Cell::new(None);

    let first = recovery_refusal_line(&last, "pty-1", "unbound_draft_absent");
    let repeat = recovery_refusal_line(&last, "pty-1", "unbound_draft_absent");
    let changed = recovery_refusal_line(&last, "pty-1", "session_unbound");

    assert_eq!(
        first.as_deref(),
        Some("[submit-audit] recovery-key refused pty=pty-1 reason=unbound_draft_absent")
    );
    assert_eq!(repeat, None);
    assert_eq!(
        changed.as_deref(),
        Some("[submit-audit] recovery-key refused pty=pty-1 reason=session_unbound")
    );
}

struct KeyLog {
    writes: RefCell<Vec<String>>,
}

impl SubmitTransport for KeyLog {
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

fn enter_count(inputs: RecoveryKeyInputs) -> usize {
    let io = KeyLog {
        writes: RefCell::new(Vec::new()),
    };
    let result = run_with_submit_key(
        &io,
        "restored draft",
        true,
        b"\r",
        tui(),
        opts(),
        |initial| {
            if initial {
                return Ok(());
            }
            recovery_key_authority(inputs).map_err(|_| InjectionError::AgentInputUnavailable {
                pty_id: "same-pty".into(),
            })?;
            Ok(())
        },
    )
    .unwrap();
    assert!(result.submit_anomaly);
    let enters = io
        .writes
        .borrow()
        .iter()
        .filter(|write| write.as_str() == "enter")
        .count();
    assert_eq!(enters, result.submit_key_writes);
    enters
}

#[test]
fn unbound_first_turn_draft_runs_the_bounded_retry_schedule() {
    assert_eq!(enter_count(UNBOUND_FIRST_TURN_DRAFT), MAX_SUBMIT_KEY_WRITES);
    assert_eq!(
        enter_count(RecoveryKeyInputs {
            draft_present: false,
            ..UNBOUND_FIRST_TURN_DRAFT
        }),
        1,
        "an empty composer never receives a recovery Enter"
    );
    assert_eq!(
        enter_count(RecoveryKeyInputs {
            session_starts_on_first_user_turn: false,
            ..UNBOUND_FIRST_TURN_DRAFT
        }),
        1,
        "a provider that binds at launch still waits for its binding"
    );
}

#[test]
fn launch_draft_nudge_presses_enter_only_on_a_parked_first_turn_draft() {
    use crate::agent::test_provider::MockProvider;
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;

    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let registry = &fixture.service.agent_registry;
    registry.register_provider(Box::new(
        MockProvider::new("first-turn").with_session_starts_on_first_user_turn(),
    ));
    registry.register_provider(Box::new(MockProvider::new("launch-bound")));
    let _pty = fixture.bind_test_pty_session(&tab_id, "restored-session", "launch-nonce");
    let pty_id = {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("first-turn".into());
        tab.reported_session_binding_nonce = None;
        tab.pty_id.clone().unwrap()
    };
    let draft = "resume the interrupted work";
    let set_agent = |agent: &str| {
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .agent_type = Some(agent.into());
    };

    fixture
        .service
        .screen_state
        .lock()
        .process(&pty_id, "› ".as_bytes());
    assert_eq!(
        nudge_unbound_launch_draft(&fixture.service, &pty_id, draft, b"\r"),
        Err("unbound_draft_absent")
    );

    fixture
        .service
        .screen_state
        .lock()
        .process(&pty_id, draft.as_bytes());
    set_agent("launch-bound");
    assert_eq!(
        nudge_unbound_launch_draft(&fixture.service, &pty_id, draft, b"\r"),
        Err("session_unbound")
    );
    set_agent("first-turn");
    assert_eq!(
        nudge_unbound_launch_draft(&fixture.service, &pty_id, draft, b"\r"),
        Ok(())
    );

    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .reported_session_binding_nonce = Some("launch-nonce".into());
    assert_eq!(
        nudge_unbound_launch_draft(&fixture.service, &pty_id, draft, b"\r"),
        Err("session_already_bound")
    );
}
