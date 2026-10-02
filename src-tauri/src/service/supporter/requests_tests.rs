use super::*;
use codeterm_types::decision::{DecisionQuestion, DecisionUsage};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

struct Model {
    calls: Arc<AtomicUsize>,
    exhausted: bool,
}

impl DecisionModel for Model {
    fn id(&self) -> &str {
        "fixture"
    }
    fn token_mode(&self) -> TokenMode {
        TokenMode::None
    }
    fn model_id(&self) -> Option<String> {
        Some("reported-model".into())
    }
    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            noul: true,
            ..Default::default()
        }
    }
    fn pricing(&self) -> Option<DecisionPricing> {
        Some(DecisionPricing {
            display_name: "priced fixture".into(),
            input_usd_per_million: 0.042,
            output_usd_per_million: 0.0,
        })
    }
    fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
        panic!("one shared adapter call required")
    }
    fn decide_questions(
        &self,
        request: &DecisionQuestionsRequest,
        _: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        codeterm_types::decision::physical::before_request()?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.exhausted {
            return Err(DecisionError::Http(402));
        }
        Ok(DecisionResponse {
            answers: request
                .questions
                .keys()
                .map(|name| (name.clone(), DecisionAnswer::Noul { p: 0.8 }))
                .collect(),
            usage: Some(DecisionUsage {
                input_tokens: 1_000_000,
                output_tokens: 500,
            }),
        })
    }
}

fn request(n: usize) -> DecisionQuestionsRequest {
    DecisionQuestionsRequest {
        state: serde_json::json!({ "screen": "private screen", "token": "private token" }),
        questions: (0..n)
            .map(|index| {
                (
                    format!("candidate_{index}"),
                    NamedDecisionQuestion {
                        instructions: "opaque".into(),
                        question: DecisionQuestion::Noul { criteria: None },
                    },
                )
            })
            .collect(),
    }
}

#[test]
fn supporter_shared_request_counts_once_and_journals_all_distributions_and_usage() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let calls = Arc::new(AtomicUsize::new(0));
    let model = budgeted_model(
        Arc::new(Model {
            calls: calls.clone(),
            exhausted: false,
        }),
        runtime.clone(),
        1,
        VerdictKind::Rerank,
        Some("tab".into()),
        None,
    );
    let response = model
        .decide_questions(&request(36), Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(response.answers.len(), 36);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.status(1)["calls_last_hour"], 1);
    assert_eq!(
        model.decide_questions(&request(36), Instant::now() + Duration::from_secs(2)),
        Err(DecisionError::Budget)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let text = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    let verdict: Value = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|line| line["action"] == "verdict")
        .unwrap();
    assert_eq!(verdict["question_count"], 36);
    assert_eq!(verdict["distribution"].as_object().unwrap().len(), 36);
    assert_eq!(verdict["usage"]["input_tokens"], 1_000_000);
    assert_eq!(verdict["model_id"], "reported-model");
    assert_eq!(
        verdict["criteria_version"],
        criteria_hash(&serde_json::to_vec(&request(36).questions).unwrap())
    );
    assert_eq!(verdict["question_set_hash"].as_str().unwrap().len(), 64);
    assert!(!text.contains("private screen"));
    assert!(!text.contains("private token"));
    let restarted = SupporterRuntime::new(dir.path().into());
    assert_eq!(
        restarted.spend_status("fixture")["today"]["input_tokens"],
        1_000_000
    );
    assert_eq!(
        restarted.spend_status("fixture")["today"]["estimated_usd"],
        0.042
    );
}

#[test]
fn supporter_402_exhaustion_alerts_once_blocks_calls_and_new_balance_rearms() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let alerts = Arc::new(Mutex::new(Vec::new()));
    let captured = alerts.clone();
    runtime.attach_alert_sink(Arc::new(move |alert| captured.lock().push(alert)));
    let calls = Arc::new(AtomicUsize::new(0));
    let model = budgeted_model(
        Arc::new(Model {
            calls: calls.clone(),
            exhausted: true,
        }),
        runtime.clone(),
        600,
        VerdictKind::TabState,
        None,
        None,
    );
    for _ in 0..3 {
        assert_eq!(
            model.decide_questions(&request(2), Instant::now() + Duration::from_secs(2)),
            Err(DecisionError::CreditExhausted)
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.status(600)["calls_last_hour"], 1);
    assert_eq!(runtime.spend_status("fixture")["exhausted"], true);
    assert_eq!(alerts.lock().len(), 1);
    assert_eq!(
        alerts.lock()[0].level,
        codeterm_types::notification::NotifyLevel::Failure
    );
    runtime
        .set_balance("fixture", model.pricing().as_ref(), Some(5.0))
        .unwrap();
    assert_eq!(runtime.spend_status("fixture")["exhausted"], false);
    assert_eq!(
        model.decide_questions(&request(2), Instant::now() + Duration::from_secs(2)),
        Err(DecisionError::CreditExhausted)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(alerts.lock().len(), 2);
    let text = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    assert_eq!(
        text.lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|line| line["kind"] == "spend_alert")
            .count(),
        2
    );
}

#[test]
fn supporter_low_balance_owner_notification_is_journaled_once() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let alerts = Arc::new(Mutex::new(Vec::new()));
    let captured = alerts.clone();
    runtime.attach_alert_sink(Arc::new(move |alert| captured.lock().push(alert)));
    let model = budgeted_model(
        Arc::new(Model {
            calls: Arc::default(),
            exhausted: false,
        }),
        runtime.clone(),
        600,
        VerdictKind::Rerank,
        None,
        None,
    );
    runtime
        .set_balance("fixture", model.pricing().as_ref(), Some(1.01))
        .unwrap();
    for _ in 0..2 {
        model
            .decide_questions(&request(36), Instant::now() + Duration::from_secs(2))
            .unwrap();
    }
    assert_eq!(alerts.lock().len(), 1);
    assert_eq!(
        alerts.lock()[0].level,
        codeterm_types::notification::NotifyLevel::ActionNeeded
    );
    let text = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    assert_eq!(
        text.lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|line| line["reason"] == "action_needed")
            .count(),
        1
    );
}

#[test]
fn supporter_first_opinion_physical_requests_counted_per_real_request() {
    struct Local;
    impl DecisionModel for Local {
        fn id(&self) -> &str {
            "local"
        }
        fn token_mode(&self) -> TokenMode {
            TokenMode::None
        }
        fn primitives(&self) -> DecisionPrimitives {
            DecisionPrimitives {
                noul: true,
                ..Default::default()
            }
        }
        fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
            Ok(DecisionAnswer::Noul { p: 0.8 })
        }
    }
    struct Sequential;
    impl DecisionModel for Sequential {
        fn id(&self) -> &str {
            "sequential"
        }
        fn token_mode(&self) -> TokenMode {
            TokenMode::None
        }
        fn primitives(&self) -> DecisionPrimitives {
            DecisionPrimitives {
                noul: true,
                ..Default::default()
            }
        }
        fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
            codeterm_types::decision::physical::before_request()?;
            Ok(DecisionAnswer::Noul { p: 0.8 })
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let local = budgeted_model(
        Arc::new(Local),
        runtime.clone(),
        2,
        VerdictKind::Rerank,
        None,
        None,
    );
    local
        .decide_questions(&request(3), Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(runtime.status(2)["calls_last_hour"], 0);
    let wire = budgeted_model(
        Arc::new(Sequential),
        runtime.clone(),
        2,
        VerdictKind::Rerank,
        None,
        None,
    );
    assert_eq!(
        wire.decide_questions(&request(3), Instant::now() + Duration::from_secs(2)),
        Err(DecisionError::Budget)
    );
    assert_eq!(runtime.status(2)["calls_last_hour"], 2);
}

#[test]
fn supporter_first_opinion_adapter_batch_cannot_hide_a_second_physical_request() {
    struct DishonestBatch;
    impl DecisionModel for DishonestBatch {
        fn id(&self) -> &str {
            "fixture"
        }
        fn token_mode(&self) -> TokenMode {
            TokenMode::None
        }
        fn supports_question_batch(&self) -> bool {
            true
        }
        fn primitives(&self) -> DecisionPrimitives {
            DecisionPrimitives {
                noul: true,
                ..Default::default()
            }
        }
        fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
            Err(DecisionError::Unsupported)
        }
        fn decide_questions(
            &self,
            request: &DecisionQuestionsRequest,
            _: Instant,
        ) -> Result<DecisionResponse, DecisionError> {
            codeterm_types::decision::physical::before_request()?;
            let _ = codeterm_types::decision::physical::before_request();
            Ok(DecisionResponse {
                answers: request
                    .questions
                    .keys()
                    .map(|key| (key.clone(), DecisionAnswer::Noul { p: 0.8 }))
                    .collect(),
                usage: None,
            })
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let model = observation_model(
        Arc::new(DishonestBatch),
        runtime.clone(),
        0,
        "tab".into(),
        "criteria".into(),
    );
    assert_eq!(
        model.decide_questions(&request(2), Instant::now() + Duration::from_secs(2)),
        Err(DecisionError::Unsupported)
    );
    assert_eq!(runtime.status(0)["calls_last_hour"], 1);
}
