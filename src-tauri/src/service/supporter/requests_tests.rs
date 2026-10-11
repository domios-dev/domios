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
fn supporter_feed_multi_question_request_one_row_expands() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let calls = Arc::new(AtomicUsize::new(0));
    let query = "fixture-query";
    let titles = ["fixture-first", "fixture-second", "fixture-third"];
    let model = budgeted_model_with_activity(
        Arc::new(Model {
            calls: calls.clone(),
            exhausted: false,
        }),
        runtime.clone(),
        0,
        VerdictKind::Rerank,
        None,
        None,
        Some(ActivityContext::Rerank {
            query: query.into(),
            titles: titles.iter().map(|title| (*title).into()).collect(),
        }),
    );
    let response = model
        .decide_questions(
            &request(titles.len()),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(response.answers.len(), titles.len());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let feed = runtime.feed(10, None).unwrap();
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0]["decision_point"], "memory_rerank");
    let asked = feed[0]["asked"].as_array().unwrap();
    assert_eq!(asked.len(), titles.len());
    for (index, title) in titles.iter().enumerate() {
        assert_eq!(asked[index]["name"], format!("candidate_{index}"));
        assert!(asked[index]["question"].as_str().unwrap().contains(*title));
        assert!(asked[index]["question"].as_str().unwrap().contains(query));
        assert!(asked[index]["answer"].as_str().unwrap().contains("80"));
    }
    for private in ["private screen", "private token", "opaque"] {
        assert!(!feed[0].to_string().contains(private));
    }
}

#[test]
fn supporter_successful_tab_state_response_keeps_decision_point() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let model = budgeted_model_with_activity(
        Arc::new(Model {
            calls: Arc::default(),
            exhausted: false,
        }),
        runtime.clone(),
        0,
        VerdictKind::TabState,
        Some("fixture-tab".into()),
        Some("fixture-criteria".into()),
        Some(ActivityContext::TabState {
            title: "fixture-tab".into(),
            declared_safe_choice: None,
            decision_point: "launch_ready".into(),
        }),
    );
    model
        .decide_questions(&request(1), Instant::now() + Duration::from_secs(2))
        .unwrap();

    let feed = runtime.feed(10, None).unwrap();
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0]["decision_point"], "launch_ready");
}

#[test]
fn supporter_feed_failed_request_preserves_fallback_and_scrubbed_question() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let secret = "supplied-credit-secret";
    let model = budgeted_model_with_activity(
        Arc::new(Model {
            calls: Arc::default(),
            exhausted: true,
        }),
        runtime.clone(),
        0,
        VerdictKind::Rerank,
        None,
        None,
        Some(ActivityContext::Rerank {
            query: format!("supplied-query OPENAI_API_KEY={secret}"),
            titles: vec!["supplied-title".into()],
        }),
    );
    assert_eq!(
        model.decide_questions(&request(1), Instant::now() + Duration::from_secs(2)),
        Err(DecisionError::CreditExhausted)
    );
    let feed = runtime.feed(10, None).unwrap();
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0]["fallback"], true);
    assert_eq!(feed[0]["reason"], "credit_exhausted");
    assert_eq!(feed[0]["decision_point"], "memory_rerank");
    assert_eq!(feed[0]["asked"].as_array().unwrap().len(), 1);
    assert!(feed[0]["asked"][0]["question"]
        .as_str()
        .unwrap()
        .contains("supplied-query"));
    assert!(!feed[0].to_string().contains(secret));
}

#[test]
fn supporter_failure_activity_labels_the_tab_state_decision_point() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    let context = ActivityContext::TabState {
        title: "supplied-tab".into(),
        declared_safe_choice: None,
        decision_point: "launch_ready".into(),
    };

    runtime.failure_with_activity(
        None,
        VerdictKind::TabState,
        Some("tab"),
        &DecisionError::Transport("private detail".into()),
        Some(&context),
    );

    let text = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    let failure: Value = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|line| line["action"] == "notify_only")
        .unwrap();
    assert_eq!(failure["decision_point"], "launch_ready");
    assert_eq!(
        runtime.feed(10, None).unwrap()[0]["decision_point"],
        "launch_ready"
    );
    assert!(!text.contains("private detail"));
}

#[test]
fn supporter_cold_adapter_decision_gets_a_startup_budget_until_ready() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let model = budgeted_model_with_activity(
        Arc::new(Model {
            calls: Arc::default(),
            exhausted: false,
        }),
        runtime.clone(),
        0,
        VerdictKind::TabState,
        None,
        None,
        None,
    );
    let now = Instant::now();

    assert_eq!(
        runtime.decision_deadline("fixture", 3_000, now),
        now + Duration::from_secs(5)
    );
    model
        .decide_questions(&request(1), now + Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        runtime.decision_deadline("fixture", 3_000, now),
        now + Duration::from_secs(3)
    );
    assert_eq!(
        runtime.decision_deadline("other", 3_000, now),
        now + Duration::from_secs(5)
    );
}

#[test]
fn supporter_shared_request_counts_once_and_journals_all_distributions_and_usage() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let calls = Arc::new(AtomicUsize::new(0));
    let model = budgeted_model_with_activity(
        Arc::new(Model {
            calls: calls.clone(),
            exhausted: false,
        }),
        runtime.clone(),
        1,
        VerdictKind::Rerank,
        Some("tab".into()),
        None,
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
fn supporter_402_exhaustion_alerts_once_and_blocks_calls() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let alerts = Arc::new(Mutex::new(Vec::new()));
    let captured = alerts.clone();
    runtime.attach_alert_sink(Arc::new(move |alert| captured.lock().push(alert)));
    let calls = Arc::new(AtomicUsize::new(0));
    let model = budgeted_model_with_activity(
        Arc::new(Model {
            calls: calls.clone(),
            exhausted: true,
        }),
        runtime.clone(),
        600,
        VerdictKind::TabState,
        None,
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
    assert_eq!(
        model.decide_questions(&request(2), Instant::now() + Duration::from_secs(2)),
        Err(DecisionError::CreditExhausted)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(alerts.lock().len(), 1);
    let text = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    assert_eq!(
        text.lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|line| line["kind"] == "spend_alert")
            .count(),
        1
    );
}

#[test]
fn supporter_rerank_rejects_sequential_wire_requests_after_one_call() {
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
    let local = budgeted_model_with_activity(
        Arc::new(Local),
        runtime.clone(),
        2,
        VerdictKind::Rerank,
        None,
        None,
        None,
    );
    local
        .decide_questions(&request(3), Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(runtime.status(2)["calls_last_hour"], 0);
    let wire = budgeted_model_with_activity(
        Arc::new(Sequential),
        runtime.clone(),
        2,
        VerdictKind::Rerank,
        None,
        None,
        None,
    );
    assert_eq!(
        wire.decide_questions(&request(3), Instant::now() + Duration::from_secs(2)),
        Err(DecisionError::Unsupported)
    );
    assert_eq!(runtime.status(2)["calls_last_hour"], 1);
    let other = budgeted_model_with_activity(
        Arc::new(Sequential),
        runtime.clone(),
        2,
        VerdictKind::Sentinel,
        None,
        None,
        None,
    );
    assert_eq!(
        other
            .decide_questions(&request(3), Instant::now() + Duration::from_secs(2))
            .unwrap()
            .answers
            .len(),
        3
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
    let model = budgeted_model_with_activity(
        Arc::new(DishonestBatch),
        runtime.clone(),
        0,
        VerdictKind::TabState,
        Some("tab".into()),
        Some("criteria".into()),
        Some(ActivityContext::TabState {
            title: "supplied-tab".into(),
            declared_safe_choice: None,
            decision_point: "launch_ready".into(),
        }),
    );
    assert_eq!(
        model.decide_questions(&request(2), Instant::now() + Duration::from_secs(2)),
        Err(DecisionError::Unsupported)
    );
    assert_eq!(runtime.status(0)["calls_last_hour"], 1);
}

#[test]
fn supporter_unavailable_backoff_emits_once_and_does_not_extend_on_blocked_calls() {
    let clock = codeterm_types::decision::deadline::FixtureClock::new(Instant::now());
    let origin = clock.now();
    let _scope = clock.enter();
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    runtime.failure_with_activity(
        Some("fixture"),
        VerdictKind::TabState,
        Some("tab"),
        &DecisionError::Unavailable("offline".into()),
        None,
    );
    assert!(!runtime.adapter_available("fixture"));
    for second in 1..60 {
        clock.advance_to(origin + Duration::from_secs(second));
        let error = runtime.admit_adapter("fixture").unwrap_err();
        runtime.failure_with_activity(
            Some("fixture"),
            VerdictKind::TabState,
            Some("tab"),
            &error,
            None,
        );
    }
    assert_eq!(runtime.feed(100, None).unwrap().len(), 1);
    clock.advance_to(origin + Duration::from_secs(60));
    assert!(runtime.adapter_available("fixture"));
    runtime.failure_with_activity(
        Some("fixture"),
        VerdictKind::TabState,
        Some("tab"),
        &DecisionError::Unavailable("offline".into()),
        None,
    );
    assert_eq!(runtime.feed(100, None).unwrap().len(), 1);
    runtime.mark_adapter_ready("fixture");
    assert!(runtime.adapter_available("fixture"));
    runtime.failure_with_activity(
        Some("fixture"),
        VerdictKind::TabState,
        Some("tab"),
        &DecisionError::Unavailable("new outage".into()),
        None,
    );
    assert_eq!(runtime.feed(100, None).unwrap().len(), 2);
}
