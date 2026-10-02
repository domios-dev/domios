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
    assert_eq!(feed[0]["asked"].as_array().unwrap().len(), 1);
    assert!(feed[0]["asked"][0]["question"]
        .as_str()
        .unwrap()
        .contains("supplied-query"));
    assert!(!feed[0].to_string().contains(secret));
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
fn supporter_402_exhaustion_alerts_once_blocks_calls_and_new_balance_rearms() {
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
    let model = budgeted_model_with_activity(
        Arc::new(Model {
            calls: Arc::default(),
            exhausted: false,
        }),
        runtime.clone(),
        600,
        VerdictKind::Rerank,
        None,
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
