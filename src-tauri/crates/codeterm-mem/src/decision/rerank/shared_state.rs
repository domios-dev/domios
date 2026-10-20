use std::collections::BTreeMap;

use codeterm_decision::{DecisionQuestionsRequest, NamedDecisionQuestion};
use serde_json::json;

use super::*;

pub(super) const INSTRUCTIONS: &str =
    "Evaluate only candidate {name} in the shared candidates map. ";

pub(super) fn run(
    model: Arc<dyn DecisionModel>,
    primitive: Primitive,
    query: &str,
    candidates: &[RerankCandidate],
    cfg: &StageConfig,
    deadline: Instant,
) -> StageOutcome {
    let request = build_request(primitive, query, candidates, cfg.max_body_chars);
    let expected = request.clone();
    let (tx, rx) = deadline::channel();
    let context = deadline::capture();
    let worker = std::thread::Builder::new()
        .name("mem-supporter-shared-state".into())
        .spawn(move || {
            let _scope = context.enter();
            let _ = tx.send(model.decide_questions(&request, deadline));
        });
    if worker.is_err() {
        return fallback(FallbackReason::AdapterError);
    }
    let response = match rx.recv_until(deadline) {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => return fallback(FallbackReason::from_error(&error)),
        Err(RecvTimeoutError::Timeout) => return fallback(FallbackReason::Deadline),
        Err(RecvTimeoutError::Disconnected) => return fallback(FallbackReason::AdapterError),
    };
    if deadline::now() >= deadline {
        return fallback(FallbackReason::Deadline);
    }
    if expected.validate_answers(&response.answers).is_err() {
        return fallback(FallbackReason::AdapterError);
    }
    let relevance = (0..candidates.len())
        .map(|index| {
            response
                .answers
                .get(&name(index))
                .and_then(|answer| relevance_of(primitive, answer))
        })
        .collect::<Option<Vec<_>>>();
    match relevance {
        Some(relevance) => applied(relevance, cfg.low_confidence_threshold),
        None => fallback(FallbackReason::AdapterError),
    }
}

fn name(index: usize) -> String {
    format!("candidate_{index}")
}

fn build_request(
    primitive: Primitive,
    query: &str,
    candidates: &[RerankCandidate],
    max_chars: usize,
) -> DecisionQuestionsRequest {
    let mut questions = BTreeMap::new();
    let mut state = BTreeMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let name = name(index);
        let single = super::build_request(primitive, query, candidate, max_chars);
        state.insert(
            name.clone(),
            json!({ "title": candidate.title, "kind": candidate.kind,
            "body": truncate_chars(&candidate.body, max_chars) }),
        );
        questions.insert(
            name.clone(),
            NamedDecisionQuestion {
                instructions: format!(
                    "{}{}",
                    INSTRUCTIONS.replace("{name}", &name),
                    single.instructions
                ),
                question: single.question,
            },
        );
    }
    DecisionQuestionsRequest {
        state: json!({ "query": query, "candidates": state }),
        questions,
    }
}

#[cfg(test)]
mod tests;
