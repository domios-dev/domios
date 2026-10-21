use super::digest::{bounded_text, MAX_DIGEST_BYTES};
use crate::plan::{CuratorRound, Milestone, Plan};

const HISTORY_LIMIT: usize = 4096;
const ROUND_RECORD_LIMIT: usize = 16 * 1024;

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
    let goal = format!("\ngoal [{}]: {} | {}\ncorrectness vector [{}]: {}\nopen acceptance criteria [{}]: {open_criteria}\n", plan.id, bounded_text(&plan.title, 256), bounded_text(goal.map_or("", |goal| goal.vision.as_str()), 512), plan.id, bounded_text(goal.map_or("", |goal| goal.vector.as_str()), 512), plan.id);
    let context_limit = MAX_DIGEST_BYTES - 2048;
    let mut body = bounded_text(base, context_limit.saturating_sub(goal.len()));
    body.push_str(&goal);
    let history_limit = HISTORY_LIMIT.min(context_limit.saturating_sub(body.len()));
    body.push_str(&history(plan, history_limit));
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
        let mut lines = vec![(
            "MOVE",
            if round.move_line.is_empty() {
                round.summary
            } else {
                round.move_line
            },
        )];
        let mut retro = round.retro;
        let mut learn = round.learn;
        if retro.is_empty() && learn.is_empty() {
            for entry in plan.heartbeat.learnings.iter().filter(|entry| {
                entry.ts == round.ended_at && entry.tag.as_deref() == Some("curator")
            }) {
                if entry.text.starts_with(['+', '-']) {
                    retro.push(entry.text.clone());
                } else {
                    learn.push(entry.text.clone());
                }
            }
        }
        lines.extend(retro.into_iter().map(|line| ("RETRO", line)));
        lines.extend(learn.into_iter().map(|line| ("LEARN", line)));
        for (kind, line) in lines {
            let line = format!(
                "{} | {kind} | {}\n",
                stride.id,
                line.replace(['\n', '\r'], " ")
            );
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
