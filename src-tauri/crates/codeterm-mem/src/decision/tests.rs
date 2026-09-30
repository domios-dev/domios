use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::rerank::{
    run_stage, run_stage_until, truncate_chars, FallbackReason, RerankCandidate, StageConfig,
    StageOutcome, SCORE_LEVELS,
};
use super::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionPrimitives, DecisionQuestion,
    DecisionRequest,
};

type Respond = Box<dyn Fn(&DecisionRequest) -> Result<DecisionAnswer, DecisionError> + Send + Sync>;

struct StubModel {
    primitives: DecisionPrimitives,
    respond: Respond,
    calls: AtomicUsize,
}

impl DecisionModel for StubModel {
    fn id(&self) -> &str {
        "stub"
    }

    fn primitives(&self) -> DecisionPrimitives {
        self.primitives
    }

    fn decide(
        &self,
        req: &DecisionRequest,
        _deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (self.respond)(req)
    }
}

fn stub(
    primitives: DecisionPrimitives,
    respond: impl Fn(&DecisionRequest) -> Result<DecisionAnswer, DecisionError> + Send + Sync + 'static,
) -> Arc<StubModel> {
    Arc::new(StubModel {
        primitives,
        respond: Box::new(respond),
        calls: AtomicUsize::new(0),
    })
}

const NOUL: DecisionPrimitives = DecisionPrimitives {
    choice: false,
    noul: true,
    score: false,
};
const SCORE: DecisionPrimitives = DecisionPrimitives {
    choice: false,
    noul: false,
    score: true,
};

fn candidates(n: usize) -> Vec<RerankCandidate> {
    (0..n)
        .map(|i| RerankCandidate {
            key: format!("k{i}"),
            title: format!("candidate {i}"),
            kind: "fact".into(),
            body: format!("<<cand-{i}>>"),
        })
        .collect()
}

fn cand_index(req: &DecisionRequest) -> usize {
    let state = req.state.as_str().expect("string request state");
    let start = state.find("<<cand-").expect("candidate marker") + "<<cand-".len();
    let end = start + state[start..].find(">>").expect("marker end");
    state[start..end].parse().expect("marker index")
}

fn noul_by_index(ps: Vec<f64>) -> Arc<StubModel> {
    stub(NOUL, move |req| {
        Ok(DecisionAnswer::Noul {
            p: ps[cand_index(req)],
        })
    })
}

fn cfg() -> StageConfig {
    StageConfig {
        max_concurrency: 4,
        deadline: Duration::from_secs(10),
        low_confidence_threshold: 0.15,
        max_body_chars: 4000,
    }
}

fn applied(outcome: StageOutcome) -> (Vec<usize>, Vec<f64>, bool, f64) {
    match outcome {
        StageOutcome::Applied {
            order,
            relevance,
            low_confidence,
            max_relevance,
        } => (order, relevance, low_confidence, max_relevance),
        other => panic!("expected Applied, got {other:?}"),
    }
}

fn fallback_reason(outcome: StageOutcome) -> FallbackReason {
    match outcome {
        StageOutcome::Fallback { reason } => reason,
        other => panic!("expected Fallback, got {other:?}"),
    }
}

#[test]
fn decision_stage_sorts_by_relevance_desc() {
    let model = noul_by_index(vec![0.2, 0.9, 0.5]);
    let (order, relevance, low, max) = applied(run_stage(model, "q", &candidates(3), &cfg()));
    assert_eq!(order, vec![1, 2, 0]);
    assert_eq!(relevance, vec![0.9, 0.5, 0.2]);
    assert!(!low);
    assert_eq!(max, 0.9);
}

#[test]
fn decision_stage_ties_keep_original_rank() {
    let model = noul_by_index(vec![0.4, 0.7, 0.4, 0.7, 0.4]);
    let (order, _, _, _) = applied(run_stage(model, "q", &candidates(5), &cfg()));
    assert_eq!(order, vec![1, 3, 0, 2, 4]);
}

#[test]
fn decision_stage_low_confidence_flag_threshold() {
    let below = noul_by_index(vec![0.05, 0.10]);
    let (_, _, low, max) = applied(run_stage(below, "q", &candidates(2), &cfg()));
    assert!(low);
    assert_eq!(max, 0.10);

    let above = noul_by_index(vec![0.05, 0.20]);
    let (_, _, low, max) = applied(run_stage(above, "q", &candidates(2), &cfg()));
    assert!(!low);
    assert_eq!(max, 0.20);
}

#[test]
fn decision_stage_score_primitive_normalized() {
    let dists: Vec<[f64; 4]> = vec![
        [0.7, 0.2, 0.1, 0.0],
        [0.0, 0.1, 0.2, 0.7],
        [0.1, 0.4, 0.4, 0.1],
    ];
    let expected: Vec<f64> = dists
        .iter()
        .map(|d| (0..4).map(|i| d[i] * i as f64 / 3.0).sum())
        .collect();
    let recorded_levels = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let seen = recorded_levels.clone();
    let scripted = dists.clone();
    let model = stub(SCORE, move |req| {
        if let DecisionQuestion::Score { levels } = &req.question {
            seen.lock().push(levels.clone());
        }
        let probabilities: BTreeMap<String, f64> = scripted[cand_index(req)]
            .iter()
            .enumerate()
            .map(|(i, p)| (i.to_string(), *p))
            .collect();
        Ok(DecisionAnswer::Score {
            score: 0.0,
            probabilities,
            confidence: None,
        })
    });

    let (order, relevance, _, max) = applied(run_stage(model, "q", &candidates(3), &cfg()));
    assert_eq!(SCORE_LEVELS.len(), 4);
    let expected_levels: Vec<String> = SCORE_LEVELS
        .iter()
        .map(|level| (*level).to_string())
        .collect();
    assert_eq!(*recorded_levels.lock(), vec![expected_levels; 3]);
    assert_eq!(order, vec![1, 2, 0]);
    for (rank, &index) in order.iter().enumerate() {
        assert!((relevance[rank] - expected[index]).abs() < 1e-12);
    }
    assert!((max - expected[1]).abs() < 1e-12);
}

#[test]
fn decision_stage_prefers_noul_when_both_declared() {
    let questions = Arc::new(Mutex::new(Vec::new()));
    let seen = questions.clone();
    let both = DecisionPrimitives {
        choice: true,
        noul: true,
        score: true,
    };
    let model = stub(both, move |req| {
        seen.lock().push(req.question.clone());
        Ok(DecisionAnswer::Noul { p: 0.5 })
    });
    applied(run_stage(model, "q", &candidates(2), &cfg()));
    assert!(questions
        .lock()
        .iter()
        .all(|q| matches!(q, DecisionQuestion::Noul { .. })));
}

#[test]
fn decision_stage_choice_only_is_unsupported() {
    let choice_only = DecisionPrimitives {
        choice: true,
        noul: false,
        score: false,
    };
    let model = stub(choice_only, |_| Ok(DecisionAnswer::Noul { p: 1.0 }));
    let reason = fallback_reason(run_stage(model.clone(), "q", &candidates(3), &cfg()));
    assert_eq!(reason, FallbackReason::Unsupported);
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn decision_stage_error_falls_back_all_or_nothing() {
    let model = stub(NOUL, |req| match cand_index(req) {
        3 => Err(DecisionError::Http(500)),
        i => Ok(DecisionAnswer::Noul { p: i as f64 / 10.0 }),
    });
    let reason = fallback_reason(run_stage(model, "q", &candidates(5), &cfg()));
    assert_eq!(reason, FallbackReason::AdapterError);
}

#[test]
fn decision_stage_unavailable_maps_reason() {
    let model = stub(NOUL, |_| Err(DecisionError::Unavailable("no key".into())));
    let reason = fallback_reason(run_stage(model, "q", &candidates(3), &cfg()));
    assert_eq!(reason, FallbackReason::Unavailable);
}

#[test]
fn decision_stage_error_classes_map_to_stable_reasons() {
    let cases = [
        (DecisionError::Unavailable("x".into()), "unavailable"),
        (DecisionError::Unsupported, "unsupported"),
        (DecisionError::Deadline, "deadline"),
        (DecisionError::Http(503), "adapter_error"),
        (DecisionError::Transport("reset".into()), "adapter_error"),
        (DecisionError::Parse("bad json".into()), "adapter_error"),
    ];
    for (error, expected) in cases {
        let model = stub(NOUL, move |_| Err(error.clone()));
        let reason = fallback_reason(run_stage(model, "q", &candidates(2), &cfg()));
        assert_eq!(reason.as_str(), expected);
    }
    assert_eq!(FallbackReason::NoAdapter.as_str(), "no_adapter");
}

#[test]
fn decision_stage_mismatched_answer_is_adapter_error() {
    let model = stub(NOUL, |_| {
        Ok(DecisionAnswer::Score {
            score: 1.0,
            probabilities: BTreeMap::from([("3".to_string(), 1.0)]),
            confidence: None,
        })
    });
    let reason = fallback_reason(run_stage(model, "q", &candidates(2), &cfg()));
    assert_eq!(reason, FallbackReason::AdapterError);
}

#[test]
fn decision_stage_deadline_falls_back_without_joining() {
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Mutex::new(release_rx);
    let unblocked = Arc::new(AtomicBool::new(false));
    let flag = unblocked.clone();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let done_tx = Mutex::new(done_tx);
    let model = stub(NOUL, move |req| {
        if cand_index(req) == 1 {
            let _ = release_rx.lock().recv();
            flag.store(true, Ordering::SeqCst);
            let _ = done_tx.lock().send(());
        }
        Ok(DecisionAnswer::Noul { p: 0.5 })
    });
    let config = StageConfig {
        deadline: Duration::from_millis(200),
        ..cfg()
    };

    let started = Instant::now();
    let reason = fallback_reason(run_stage(model, "q", &candidates(3), &config));
    let elapsed = started.elapsed();

    assert_eq!(reason, FallbackReason::Deadline);
    assert!(elapsed < Duration::from_millis(1200), "elapsed {elapsed:?}");
    assert!(
        !unblocked.load(Ordering::SeqCst),
        "blocked worker was waited on"
    );
    drop(release_tx);
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("released worker finishes");
}

#[test]
fn decision_stage_concurrency_is_bounded() {
    let barrier = Arc::new(Barrier::new(2));
    let inflight = Arc::new(AtomicUsize::new(0));
    let max_inflight = Arc::new(AtomicUsize::new(0));
    let (gate, now, peak) = (barrier.clone(), inflight.clone(), max_inflight.clone());
    let model = stub(NOUL, move |req| {
        let current = now.fetch_add(1, Ordering::SeqCst) + 1;
        peak.fetch_max(current, Ordering::SeqCst);
        gate.wait();
        now.fetch_sub(1, Ordering::SeqCst);
        Ok(DecisionAnswer::Noul {
            p: cand_index(req) as f64 / 10.0,
        })
    });
    let config = StageConfig {
        max_concurrency: 2,
        ..cfg()
    };

    let (order, _, _, _) = applied(run_stage(model.clone(), "q", &candidates(6), &config));
    assert_eq!(order, vec![5, 4, 3, 2, 1, 0]);
    assert_eq!(max_inflight.load(Ordering::SeqCst), 2);
    assert_eq!(model.calls.load(Ordering::SeqCst), 6);
}

#[test]
fn decision_stage_empty_candidates_makes_no_call() {
    let model = noul_by_index(Vec::new());
    let outcome = run_stage(model.clone(), "q", &[], &cfg());
    assert_eq!(
        outcome,
        StageOutcome::Applied {
            order: Vec::new(),
            relevance: Vec::new(),
            low_confidence: true,
            max_relevance: 0.0,
        }
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn stage_uses_the_shared_deadline_after_optional_source_work() {
    let outcome = run_stage_until(
        noul_by_index(vec![0.5]),
        "q",
        &candidates(1),
        &cfg(),
        Instant::now(),
    );

    assert_eq!(fallback_reason(outcome), FallbackReason::Deadline);
}

#[test]
fn decision_stage_body_capped_on_char_boundary() {
    let cap = 7;
    let body: String = "aé日🦀".repeat(10);
    let states = Arc::new(Mutex::new(Vec::new()));
    let seen = states.clone();
    let model = stub(NOUL, move |req| {
        seen.lock().push(req.state.clone());
        Ok(DecisionAnswer::Noul { p: 0.5 })
    });
    let config = StageConfig {
        max_body_chars: cap,
        ..cfg()
    };
    let title = "An untruncated title";
    let candidate = RerankCandidate {
        key: "k".into(),
        title: title.into(),
        kind: "fact".into(),
        body: body.clone(),
    };

    applied(run_stage(model, "q", &[candidate], &config));

    let capped: String = body.chars().take(cap).collect();
    let recorded = states.lock();
    assert_eq!(recorded.len(), 1);
    let state = recorded[0].as_str().expect("string request state");
    assert_eq!(state, format!("MEMORY:\n{title}\n{capped}"));
    assert_eq!(capped.chars().count(), cap);
    assert_eq!(truncate_chars(&body, cap).chars().count(), cap);
    assert_eq!(truncate_chars("abc", cap), "abc");
    assert_eq!(truncate_chars(&body, 0), "");
}

#[test]
fn decision_stage_request_uses_supplied_query_and_memory_fields() {
    let query = "where is the migration runbook";
    let title = "Migration Runbook";
    let body = "The operations manual contains the answer.";
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    let model = stub(NOUL, move |req| {
        seen.lock().push(req.clone());
        Ok(DecisionAnswer::Noul { p: 0.5 })
    });
    let candidate = RerankCandidate {
        key: "runbook".into(),
        title: title.into(),
        kind: "fact".into(),
        body: body.into(),
    };

    applied(run_stage(model, query, &[candidate], &cfg()));

    let recorded = requests.lock();
    assert_eq!(recorded.len(), 1);
    assert!(recorded[0].instructions.contains(query));
    let expected_state = format!("MEMORY:\n{title}\n{body}");
    assert_eq!(recorded[0].state.as_str(), Some(expected_state.as_str()));
    assert!(matches!(
        &recorded[0].question,
        DecisionQuestion::Noul { criteria: None }
    ));
}

#[test]
fn decision_stage_score_instruction_uses_supplied_query() {
    let query = "what is the fallback path";
    let instructions = Arc::new(Mutex::new(Vec::new()));
    let seen = instructions.clone();
    let model = stub(SCORE, move |req| {
        seen.lock().push(req.instructions.clone());
        Ok(DecisionAnswer::Score {
            score: 0.0,
            probabilities: BTreeMap::from([("0".into(), 1.0)]),
            confidence: None,
        })
    });

    applied(run_stage(model, query, &candidates(1), &cfg()));

    assert_eq!(instructions.lock().len(), 1);
    assert!(instructions.lock()[0].contains(query));
}
