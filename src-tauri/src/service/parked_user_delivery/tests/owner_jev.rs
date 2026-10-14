use super::*;

static SUPPORTER_OVERRIDE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Copy)]
enum DeliveryModelAnswer {
    Ready,
    LowConfidence,
    Modal,
    Unavailable,
}

struct DeliveryDecisionModel {
    answers: Vec<DeliveryModelAnswer>,
    calls: AtomicUsize,
}

impl codeterm_decision::DecisionModel for DeliveryDecisionModel {
    fn supports_question_batch(&self) -> bool {
        true
    }
    fn id(&self) -> &str {
        "owner-delivery-model"
    }
    fn token_mode(&self) -> codeterm_decision::TokenMode {
        codeterm_decision::TokenMode::None
    }
    fn primitives(&self) -> codeterm_decision::DecisionPrimitives {
        codeterm_decision::DecisionPrimitives {
            choice: true,
            noul: true,
            score: false,
        }
    }
    fn decide(
        &self,
        _: &codeterm_decision::DecisionRequest,
        _: std::time::Instant,
    ) -> Result<codeterm_decision::DecisionAnswer, codeterm_decision::DecisionError> {
        Err(codeterm_decision::DecisionError::Unsupported)
    }
    fn decide_questions(
        &self,
        request: &codeterm_decision::DecisionQuestionsRequest,
        _: std::time::Instant,
    ) -> Result<codeterm_decision::DecisionResponse, codeterm_decision::DecisionError> {
        use codeterm_decision::{
            DecisionAnswer, DecisionError, DecisionQuestion, DecisionResponse,
        };
        use std::collections::BTreeMap;

        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        let answer = self
            .answers
            .get(index)
            .copied()
            .unwrap_or(DeliveryModelAnswer::Unavailable);
        if matches!(answer, DeliveryModelAnswer::Unavailable) {
            return Err(DecisionError::Unavailable(
                "fixture has no second verdict".into(),
            ));
        }
        let (confidence, input_area) = match answer {
            DeliveryModelAnswer::Ready => (0.99, "composer"),
            DeliveryModelAnswer::LowConfidence => (0.89, "composer"),
            DeliveryModelAnswer::Modal => (0.99, "active_modal"),
            DeliveryModelAnswer::Unavailable => unreachable!(),
        };
        let answers = request
            .questions
            .iter()
            .map(|(name, spec)| {
                let answer = match name.as_str() {
                    "label" => {
                        choice_answer(crate::service::supporter_tab_state::READY_IDLE, confidence)
                    }
                    "prompt_visible" => DecisionAnswer::Noul {
                        p: if matches!(answer, DeliveryModelAnswer::Modal) {
                            0.99
                        } else {
                            0.0
                        },
                    },
                    "input_area" => choice_answer(input_area, confidence),
                    _ => match &spec.question {
                        DecisionQuestion::Noul { .. } => DecisionAnswer::Noul { p: 0.0 },
                        DecisionQuestion::Choice { options } => choice_answer(
                            if options.contains_key(crate::service::supporter_tab_state::OTHER) {
                                crate::service::supporter_tab_state::OTHER
                            } else {
                                "__abstain__"
                            },
                            1.0,
                        ),
                        DecisionQuestion::Score { levels } => DecisionAnswer::Score {
                            score: 0.0,
                            probabilities: BTreeMap::from([(levels[0].clone(), 1.0)]),
                            confidence: Some(1.0),
                        },
                    },
                };
                Ok((name.clone(), answer))
            })
            .collect::<Result<BTreeMap<_, _>, DecisionError>>()?;
        Ok(DecisionResponse {
            answers,
            usage: None,
        })
    }
}

fn choice_answer(choice: &str, confidence: f64) -> codeterm_decision::DecisionAnswer {
    codeterm_decision::DecisionAnswer::Choice {
        choice: choice.into(),
        probabilities: std::collections::BTreeMap::from([(choice.into(), confidence)]),
        confidence: Some(confidence),
    }
}

fn restored_unbound_user_head(
    answers: Vec<DeliveryModelAnswer>,
) -> (
    ServiceFixture,
    String,
    Arc<DeliveryDecisionModel>,
    Arc<crate::service::supporter::SupporterRuntime>,
    crate::service::supporter_tab_state::TestSupporterOverrideGuard,
) {
    use codeterm_types::config::{SupporterConfig, SupporterConnection};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    let (fixture, tab_id) = queue_fixture(true, None);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.boot_stage = Some(codeterm_types::pty::BootStage::LaunchWritten);
        tab.session_binding_nonce = Some("nonce-restored-launch".into());
        tab.reported_session_binding_nonce = Some("nonce-prior-launch".into());
        tab.agent_session_id = Some("opaque-session".into());
    }
    let generation = attach_fixture_pty(&fixture, &tab_id);
    let accepted = admit_current_ledger(
        &fixture.service,
        &tab_id,
        FIRST_PAYLOAD,
        "restored-owner-client",
    );
    let error = DeliveryError::TabBusyRefused {
        tab: tab_id.clone(),
        detail: REFUSAL.into(),
    };
    fixture
        .service
        .park_refused_user_send(&tab_id, &accepted.send_id, FIRST_PAYLOAD, &error);
    let authority = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .head(&tab_id)
        .unwrap()
        .unwrap()
        .authority;
    assert_eq!(authority.pty_session_generation, Some(generation));

    let adapter = "owner-delivery-fixture";
    let model = Arc::new(DeliveryDecisionModel {
        answers,
        calls: AtomicUsize::new(0),
    });
    fixture
        .service
        .supporter_models
        .register(adapter, model.clone());
    let runtime = Arc::new(crate::service::supporter::SupporterRuntime::new(
        fixture.temp.path().join("supporter"),
    ));
    let config = SupporterConfig {
        adapter: Some(adapter.into()),
        connections: BTreeMap::from([(
            adapter.into(),
            SupporterConnection {
                model: None,
                endpoint: None,
            },
        )]),
        ..Default::default()
    };
    let config_guard = fixture
        .service
        .override_supporter_decision_for_test(config, runtime.clone());
    (fixture, tab_id, model, runtime, config_guard)
}

#[test]
fn restored_owner_needs_two_adapter_verdicts_for_one_pty_generation_before_one_write() {
    let _serial = SUPPORTER_OVERRIDE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (fixture, tab, model, runtime, _config_guard) =
        restored_unbound_user_head(vec![DeliveryModelAnswer::Ready, DeliveryModelAnswer::Ready]);
    let send_id = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .head(&tab)
        .unwrap()
        .unwrap()
        .send_id;
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );

    fixture.service.drain_user_owner(&tab).unwrap();

    assert_eq!(
        model.calls.load(Ordering::SeqCst),
        2,
        "head={:?} status={:?}",
        fixture
            .service
            .pending_delivery_payloads
            .user_sends
            .head(&tab),
        fixture.service.chat_send_ledger.status(&send_id)
    );
    assert_eq!(
        writes.load(Ordering::SeqCst),
        1,
        "status={:?}",
        fixture.service.chat_send_ledger.status(&send_id)
    );
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .is_empty());
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&send_id)
            .unwrap()
            .state,
        crate::service::chat_send_transaction::ChatSendState::Unknown
    );
    let evidence = runtime.feed(100, None).unwrap();
    let readiness = evidence
        .iter()
        .filter(|entry| {
            entry["kind"] == "tab_state"
                && entry["correlation"]["decision_point"] == "user_delivery_readiness"
        })
        .collect::<Vec<_>>();
    assert_eq!(readiness.len(), 2);
    assert!(readiness
        .iter()
        .all(|entry| entry["decided_by"] == "adapter"));
}

#[test]
fn restored_owner_with_only_one_adapter_verdict_stays_pending_without_a_write() {
    let _serial = SUPPORTER_OVERRIDE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (fixture, tab, model, _, _config_guard) = restored_unbound_user_head(vec![
        DeliveryModelAnswer::Ready,
        DeliveryModelAnswer::Unavailable,
    ]);
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );

    fixture.service.drain_user_owner(&tab).unwrap();

    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    let head = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .head(&tab)
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&head.send_id)
            .unwrap()
            .pending_reason
            .as_deref(),
        Some("supporter_observation_unavailable")
    );
}

#[test]
fn restored_owner_low_confidence_verdict_stays_pending_without_a_write() {
    let _serial = SUPPORTER_OVERRIDE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (fixture, tab, model, _, _config_guard) =
        restored_unbound_user_head(vec![DeliveryModelAnswer::LowConfidence]);
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );

    fixture.service.drain_user_owner(&tab).unwrap();

    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    let head = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .head(&tab)
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&head.send_id)
            .unwrap()
            .pending_reason
            .as_deref(),
        Some("supporter_confidence_below_threshold")
    );
}

#[test]
fn restored_owner_visible_modal_verdict_stays_pending_without_a_write() {
    let _serial = SUPPORTER_OVERRIDE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (fixture, tab, model, _, _config_guard) =
        restored_unbound_user_head(vec![DeliveryModelAnswer::Modal]);
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );

    fixture.service.drain_user_owner(&tab).unwrap();

    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    let head = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .head(&tab)
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&head.send_id)
            .unwrap()
            .pending_reason
            .as_deref(),
        Some("supporter_prompt_visible")
    );
}

#[test]
fn restored_owner_rechecks_pty_generation_immediately_before_the_single_write() {
    let _serial = SUPPORTER_OVERRIDE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (fixture, tab, model, _, _config_guard) =
        restored_unbound_user_head(vec![DeliveryModelAnswer::Ready, DeliveryModelAnswer::Ready]);
    let pty_id = fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .pty_id
        .clone()
        .unwrap();
    let service = fixture.service.clone();
    let rotate_id = pty_id.clone();
    let _rotation = BeforeUserSubmitBoundary::install(
        &tab,
        Arc::new(move || {
            service
                .pty
                .lock()
                .begin_kill(&rotate_id)
                .expect("remove the original PTY generation")
                .reap_in_background();
            let (_, session) = service
                .pty
                .lock()
                .spawn_with_id(
                    Some(rotate_id.clone()),
                    None,
                    None,
                    120,
                    40,
                    Default::default(),
                )
                .expect("replace the PTY with a new concrete generation");
            assert!(service
                .screen_state
                .lock()
                .bind_pty_generation(&rotate_id, session.generation));
        }),
    );
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _submit = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );

    fixture.service.drain_user_owner(&tab).unwrap();

    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    let head = fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .head(&tab)
        .unwrap()
        .unwrap();
    assert_eq!(head.state, UserSendState::Queued);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&head.send_id)
            .unwrap()
            .pending_reason
            .as_deref(),
        Some("readiness_changed_before_payload_write")
    );
}
