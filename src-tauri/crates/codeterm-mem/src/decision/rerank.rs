//! Stage-2 re-rank: one decision per candidate, all-or-nothing, bounded by a failure deadline.

use std::collections::VecDeque;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;

use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionQuestion, DecisionRequest,
};

pub const NOUL_INSTRUCTIONS_TEMPLATE: &str =
    "Does this memory directly and usefully answer: {query}?";
pub const SCORE_INSTRUCTIONS_TEMPLATE: &str =
    "How directly and usefully does this memory answer: {query}?";
pub const SCORE_LEVELS: [&str; 4] = [
    "Does not help answer the question.",
    "On topic, but lacks an answer-bearing fact.",
    "Contains a useful fact that partly answers the question.",
    "Directly answers the question or gives the decisive fact.",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RerankCandidate {
    pub key: String,
    pub title: String,
    pub kind: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageConfig {
    pub max_concurrency: usize,
    pub deadline: Duration,
    pub low_confidence_threshold: f64,
    pub max_body_chars: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StageOutcome {
    Applied {
        order: Vec<usize>,
        relevance: Vec<f64>,
        low_confidence: bool,
        max_relevance: f64,
    },
    Fallback {
        reason: FallbackReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    NoAdapter,
    Unsupported,
    Unavailable,
    Deadline,
    AdapterError,
}

impl FallbackReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoAdapter => "no_adapter",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Deadline => "deadline",
            Self::AdapterError => "adapter_error",
        }
    }

    fn from_error(error: &DecisionError) -> Self {
        match error {
            DecisionError::Unavailable(_) => Self::Unavailable,
            DecisionError::Unsupported => Self::Unsupported,
            DecisionError::Deadline => Self::Deadline,
            DecisionError::Http(_) | DecisionError::Transport(_) | DecisionError::Parse(_) => {
                Self::AdapterError
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Primitive {
    Noul,
    Score,
}

type Job = (usize, DecisionRequest);
type JobResult = (usize, Result<DecisionAnswer, DecisionError>);

pub fn run_stage(
    model: Arc<dyn DecisionModel>,
    query: &str,
    candidates: &[RerankCandidate],
    cfg: &StageConfig,
) -> StageOutcome {
    run_stage_until(model, query, candidates, cfg, Instant::now() + cfg.deadline)
}

pub fn run_stage_until(
    model: Arc<dyn DecisionModel>,
    query: &str,
    candidates: &[RerankCandidate],
    cfg: &StageConfig,
    deadline: Instant,
) -> StageOutcome {
    if candidates.is_empty() {
        return StageOutcome::Applied {
            order: Vec::new(),
            relevance: Vec::new(),
            low_confidence: true,
            max_relevance: 0.0,
        };
    }
    if Instant::now() >= deadline {
        return fallback(FallbackReason::Deadline);
    }
    let primitives = model.primitives();
    let primitive = if primitives.noul {
        Primitive::Noul
    } else if primitives.score {
        Primitive::Score
    } else {
        return fallback(FallbackReason::Unsupported);
    };

    let n = candidates.len();
    let jobs: VecDeque<Job> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (i, build_request(primitive, query, c, cfg.max_body_chars)))
        .collect();
    let queue = Arc::new(Mutex::new(jobs));
    let (tx, rx) = mpsc::channel::<JobResult>();
    // A zero cap would spawn no worker and silently turn every call into a fallback.
    for _ in 0..cfg.max_concurrency.clamp(1, n) {
        spawn_worker(model.clone(), queue.clone(), tx.clone(), deadline);
    }
    drop(tx);

    let mut relevance = vec![0.0f64; n];
    let mut received = 0usize;
    while received < n {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let reason = match rx.recv_timeout(remaining) {
            Ok((index, Ok(answer))) => match relevance_of(primitive, &answer) {
                Some(value) => {
                    relevance[index] = value;
                    received += 1;
                    continue;
                }
                None => FallbackReason::AdapterError,
            },
            Ok((_, Err(error))) => FallbackReason::from_error(&error),
            Err(RecvTimeoutError::Timeout) => FallbackReason::Deadline,
            Err(RecvTimeoutError::Disconnected) => FallbackReason::AdapterError,
        };
        queue.lock().clear();
        return fallback(reason);
    }

    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| relevance[b].total_cmp(&relevance[a]).then(a.cmp(&b)));
    let sorted: Vec<f64> = order.iter().map(|&i| relevance[i]).collect();
    let max_relevance = sorted[0];
    StageOutcome::Applied {
        order,
        relevance: sorted,
        low_confidence: max_relevance < cfg.low_confidence_threshold,
        max_relevance,
    }
}

fn fallback(reason: FallbackReason) -> StageOutcome {
    StageOutcome::Fallback { reason }
}

/// Detached: the caller never joins it, and a send to a dropped receiver ends the loop.
fn spawn_worker(
    model: Arc<dyn DecisionModel>,
    queue: Arc<Mutex<VecDeque<Job>>>,
    tx: mpsc::Sender<JobResult>,
    deadline: Instant,
) {
    let spawned = std::thread::Builder::new()
        .name("mem-supporter-rerank".into())
        .spawn(move || loop {
            if Instant::now() >= deadline {
                break;
            }
            let job = queue.lock().pop_front();
            let Some((index, request)) = job else { break };
            let result = model.decide(&request, deadline);
            if tx.send((index, result)).is_err() {
                break;
            }
        });
    if let Err(error) = spawned {
        log::warn!("[mem.decision] failed to spawn rerank worker: {error}");
    }
}

fn build_request(
    primitive: Primitive,
    query: &str,
    candidate: &RerankCandidate,
    max_chars: usize,
) -> DecisionRequest {
    let state = Value::String(format!(
        "MEMORY:\n{}\n{}",
        candidate.title,
        truncate_chars(&candidate.body, max_chars)
    ));
    let (instructions, question) = match primitive {
        Primitive::Noul => (
            NOUL_INSTRUCTIONS_TEMPLATE.replace("{query}", query),
            DecisionQuestion::Noul { criteria: None },
        ),
        Primitive::Score => (
            SCORE_INSTRUCTIONS_TEMPLATE.replace("{query}", query),
            DecisionQuestion::Score {
                levels: SCORE_LEVELS.iter().map(|l| l.to_string()).collect(),
            },
        ),
    };
    DecisionRequest {
        state,
        instructions,
        question,
    }
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((byte, _)) => &text[..byte],
        None => text,
    }
}

/// `None` marks an answer that cannot be scored; the whole stage then falls back.
fn relevance_of(primitive: Primitive, answer: &DecisionAnswer) -> Option<f64> {
    match (primitive, answer) {
        (Primitive::Noul, DecisionAnswer::Noul { p }) if p.is_finite() => Some(p.clamp(0.0, 1.0)),
        (Primitive::Score, DecisionAnswer::Score { probabilities, .. }) => {
            score_relevance(probabilities, SCORE_LEVELS.len())
        }
        _ => None,
    }
}

fn score_relevance(
    probabilities: &std::collections::BTreeMap<String, f64>,
    levels: usize,
) -> Option<f64> {
    if levels < 2 || probabilities.is_empty() {
        return None;
    }
    let top = (levels - 1) as f64;
    let mut sum = 0.0;
    for (key, p) in probabilities {
        let level: usize = key.parse().ok()?;
        if level >= levels || !p.is_finite() {
            return None;
        }
        sum += p * level as f64 / top;
    }
    Some(sum.clamp(0.0, 1.0))
}
