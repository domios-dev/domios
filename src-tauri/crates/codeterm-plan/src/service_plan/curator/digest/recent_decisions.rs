use super::Row;
use crate::plan::Plan;

fn row(id: &str, kind: &str, at: u64, note: &str) -> Row {
    Row {
        id: id.into(),
        kind: kind.into(),
        state: format!("{at:20}"),
        summary: super::clean(note.split('\n').next().unwrap_or_default(), 120),
    }
}

pub(super) fn rows(plan: &Plan) -> Vec<Row> {
    let mut rows = Vec::new();
    for stride in plan
        .strides
        .iter()
        .chain(plan.tasks.iter().flat_map(|task| &task.milestones))
    {
        let decision = matches!(
            stride.kind.as_str(),
            "decision" | "plan_decision" | "decision_answer"
        );
        let owner = plan.owner.tab_id.is_some()
            && stride.from.tab_id == plan.owner.tab_id
            && !matches!(
                stride.kind.as_str(),
                "curator_round" | "report" | "worker_report"
            );
        if decision || owner {
            rows.push(row(&stride.id, &stride.kind, stride.ts, &stride.text));
        }
    }
    for task in &plan.tasks {
        for review in &task.review_evidence {
            rows.push(row(
                &review.id,
                &format!("review:{:?}", review.verdict).to_lowercase(),
                review.at,
                &review.note,
            ));
        }
        if task.status.is_terminal() {
            rows.push(row(
                &task.id,
                task.status.key(),
                task.status_changed_at,
                &task.name,
            ));
        }
    }
    rows.sort();
    rows.dedup();
    rows.sort_by(|a, b| b.state.cmp(&a.state).then_with(|| a.cmp(b)));
    rows.truncate(10);
    rows
}
