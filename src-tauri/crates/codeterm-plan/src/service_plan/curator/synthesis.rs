use super::digest::{bounded_text, MAX_DIGEST_BYTES};
use crate::plan::{CuratorRound, Milestone, Plan};

const HISTORY_LIMIT: usize = 4096;
const ROUND_RECORD_LIMIT: usize = 16 * 1024;
mod inputs;
pub use inputs::{
    context_prefix, history_budget, history_learning_kind, history_line, round_history_lines,
    SynthesisGoalInput,
};

pub fn due(
    real_rounds_started: u64,
    group_close: Option<u64>,
    consumed_close: Option<u64>,
) -> bool {
    real_rounds_started.saturating_add(1).is_multiple_of(6)
        || group_close.is_some_and(|at| consumed_close.is_none_or(|consumed| at > consumed))
}

pub fn group_close(plan: &Plan) -> Option<u64> {
    plan.task_groups
        .iter()
        .filter_map(|group| super::super::milestone_gates::group_completion(plan, &group.key))
        .chain(
            plan.strides
                .iter()
                .filter(|stride| stride.kind == "group_close")
                .map(|stride| stride.ts),
        )
        .max()
}

fn round(stride: &Milestone) -> Option<CuratorRound> {
    if stride.kind != "curator_round" || stride.text.len() > ROUND_RECORD_LIMIT {
        return None;
    }
    serde_json::from_str::<CuratorRound>(&stride.text)
        .ok()
        .filter(|round| !round.digest_hash.is_empty())
}

pub fn real_rounds_started(plan: &Plan) -> u64 {
    plan.control_loop.real_rounds_started.max(
        plan.strides
            .iter()
            .filter(|stride| round(stride).is_some())
            .count() as u64,
    )
}

pub fn context_header(plan: &Plan, base: &str) -> String {
    let goal = plan.goal.as_ref();
    let open_criteria: usize = plan
        .tasks
        .iter()
        .filter(|task| !task.status.is_terminal())
        .map(|task| task.acceptance_criteria.len())
        .sum();
    let mut body = context_prefix(
        SynthesisGoalInput {
            id: &plan.id,
            title: &plan.title,
            vision: goal.map_or("", |goal| goal.vision.as_str()),
            vector: goal.map_or("", |goal| goal.vector.as_str()),
            open_criteria,
        },
        base,
    );
    let budget = history_budget(&body);
    body.push_str(&history(plan, budget));
    body
}

fn history(plan: &Plan, limit: usize) -> String {
    let mut records = plan
        .strides
        .iter()
        .rev()
        .filter_map(|stride| round(stride).map(|round| (stride, round)))
        .take(64)
        .collect::<Vec<_>>();
    records.sort_by(|(a, _), (b, _)| b.ts.cmp(&a.ts).then_with(|| b.id.cmp(&a.id)));
    let mut body = String::new();
    for (stride, round) in records {
        let ended_at = round.ended_at;
        let fallback = plan
            .heartbeat
            .learnings
            .iter()
            .filter(|entry| entry.ts == ended_at && entry.tag.as_deref() == Some("curator"))
            .map(|entry| entry.text.clone());
        for (kind, text) in round_history_lines(round, fallback) {
            let line = history_line(&stride.id, kind, &text);
            if body.len() + line.len() > limit {
                return body;
            }
            body.push_str(&line);
        }
    }
    body
}

#[cfg(test)]
mod tests;
