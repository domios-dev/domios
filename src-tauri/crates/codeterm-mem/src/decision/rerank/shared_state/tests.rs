use super::*;
use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionPrimitives, DecisionRequest, DecisionResponse, TokenMode,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

struct Model {
    calls: AtomicUsize,
    seen: Mutex<Option<DecisionQuestionsRequest>>,
    answers: BTreeMap<String, DecisionAnswer>,
    error: Option<DecisionError>,
}

impl DecisionModel for Model {
    fn id(&self) -> &str {
        "fixture"
    }
    fn token_mode(&self) -> TokenMode {
        TokenMode::None
    }
    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            noul: true,
            score: true,
            choice: false,
        }
    }
    fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
        panic!("shared state must use one request")
    }
    fn decide_questions(
        &self,
        request: &DecisionQuestionsRequest,
        _: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.seen.lock() = Some(request.clone());
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        Ok(DecisionResponse {
            answers: self.answers.clone(),
            usage: None,
        })
    }
}

fn candidates() -> Vec<RerankCandidate> {
    (0..3)
        .map(|i| RerankCandidate {
            key: format!("id-{i}"),
            title: format!("title-{i}"),
            kind: "fact".into(),
            body: "é日🦀long body".into(),
        })
        .collect()
}

fn cfg() -> StageConfig {
    StageConfig {
        request_mode: RerankRequestMode::SharedState,
        max_concurrency: 16,
        deadline: Duration::from_secs(2),
        low_confidence_threshold: 0.15,
        max_body_chars: 3,
    }
}

fn model() -> Arc<Model> {
    Arc::new(Model {
        calls: AtomicUsize::new(0),
        seen: Mutex::new(None),
        error: None,
        answers: BTreeMap::from([
            ("candidate_0".into(), DecisionAnswer::Noul { p: 0.2 }),
            ("candidate_1".into(), DecisionAnswer::Noul { p: 0.9 }),
            ("candidate_2".into(), DecisionAnswer::Noul { p: 0.5 }),
        ]),
    })
}

#[test]
fn rerank_stage_one_request_with_n_candidate_questions() {
    let model = model();
    let outcome = super::super::run_stage(model.clone(), "query", &candidates(), &cfg());
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    let seen = model.seen.lock();
    let request = seen.as_ref().unwrap();
    assert_eq!(request.questions.len(), 3);
    assert_eq!(request.state["query"], "query");
    assert_eq!(request.state["candidates"]["candidate_0"]["body"], "é日🦀");
    assert_eq!(
        outcome,
        StageOutcome::Applied {
            order: vec![1, 2, 0],
            relevance: vec![0.9, 0.5, 0.2],
            low_confidence: false,
            max_relevance: 0.9
        }
    );
}

#[test]
fn rerank_stage_fallback_unchanged_per_candidate() {
    for error in [
        DecisionError::Budget,
        DecisionError::CreditExhausted,
        DecisionError::Deadline,
        DecisionError::Http(500),
    ] {
        let expected = FallbackReason::from_error(&error);
        let mut fixture = model();
        Arc::get_mut(&mut fixture).unwrap().error = Some(error);
        assert_eq!(
            super::super::run_stage(fixture, "q", &candidates(), &cfg()),
            fallback(expected)
        );
    }
    for invalid in 0..3 {
        let mut fixture = model();
        Arc::get_mut(&mut fixture)
            .unwrap()
            .answers
            .remove(&name(invalid));
        assert_eq!(
            super::super::run_stage(fixture, "q", &candidates(), &cfg()),
            fallback(FallbackReason::AdapterError)
        );
    }
    let mut fixture = model();
    Arc::get_mut(&mut fixture)
        .unwrap()
        .answers
        .insert("unexpected".into(), DecisionAnswer::Noul { p: 0.9 });
    assert_eq!(
        super::super::run_stage(fixture, "q", &candidates(), &cfg()),
        fallback(FallbackReason::AdapterError)
    );
}

#[test]
fn rerank_shared_score_math_matches_per_candidate_and_ties_remain_stable() {
    let cfg = cfg();
    let request = build_request(Primitive::Score, "query", &candidates(), cfg.max_body_chars);
    let answer = DecisionAnswer::Score {
        score: 2.0,
        probabilities: BTreeMap::from([
            ("0".into(), 0.1),
            ("1".into(), 0.2),
            ("2".into(), 0.3),
            ("3".into(), 0.4),
        ]),
        confidence: None,
    };
    let relevance = relevance_of(Primitive::Score, &answer).unwrap();
    let fixture = Arc::new(Model {
        calls: AtomicUsize::new(0),
        seen: Mutex::new(None),
        error: None,
        answers: request
            .questions
            .keys()
            .map(|name| (name.clone(), answer.clone()))
            .collect(),
    });
    assert_eq!(
        run(
            fixture,
            Primitive::Score,
            "query",
            &candidates(),
            &cfg,
            Instant::now() + cfg.deadline
        ),
        StageOutcome::Applied {
            order: vec![0, 1, 2],
            relevance: vec![relevance; 3],
            low_confidence: false,
            max_relevance: relevance
        }
    );
}

#[test]
fn rerank_shared_deadline_returns_without_joining_unresponsive_model() {
    struct Held(std::sync::Mutex<mpsc::Receiver<()>>);
    impl DecisionModel for Held {
        fn id(&self) -> &str {
            "held"
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
            let _ = self.0.lock().unwrap().recv();
            Ok(DecisionAnswer::Noul { p: 0.5 })
        }
    }
    let (release, wait) = mpsc::channel();
    let start = Instant::now();
    let outcome = super::super::run_stage_until(
        Arc::new(Held(std::sync::Mutex::new(wait))),
        "q",
        &candidates(),
        &cfg(),
        start + Duration::from_millis(100),
    );
    assert_eq!(outcome, fallback(FallbackReason::Deadline));
    assert!(start.elapsed() < Duration::from_secs(1));
    drop(release);
}
