use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanMode, TaskStatus};
use crate::service_plan::{load_audit, state};

fn snapshot() -> PlanStateSnapshot {
    let mut plan = Plan::new(
        "Windowed state".into(),
        "/repo".into(),
        OrchestratorRef::default(),
    );
    plan.mode = PlanMode::Execution;
    plan.tasks = (0..120)
        .map(|index| load_audit::task(&format!("pending-{index}"), TaskStatus::Pending, None))
        .collect();
    let mut blocked = load_audit::task("blocked-last", TaskStatus::Blocked, None);
    blocked.depends_on.push("missing".into());
    plan.tasks.push(blocked);
    plan.tasks
        .push(load_audit::task("review-last", TaskStatus::InReview, None));
    state::build_plan_state_snapshot(
        &plan,
        crate::plan::coverage::compute_coverage_report(&plan),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &HashSet::new(),
        None,
        100,
    )
}

#[test]
fn windows_preserve_full_counts_decisions_and_remaining_pages() {
    let full = snapshot();
    let mut first = full.clone();
    first.window_details(StateWindowRequest::default());
    let details = first.details.as_ref().unwrap();
    assert_eq!(first.task_counts.total, 122);
    assert_eq!(details.totals["open_lanes"], 121);
    assert_eq!(first.open_lanes.len(), STATE_DETAIL_LIMIT);
    assert_eq!(first.blocked_or_gated_count, 1);
    assert_eq!(details.next_action_counts[&NextActionKind::Dispatch], 120);
    assert_eq!(details.next_action_counts[&NextActionKind::Gate], 1);
    assert_eq!(details.next_action_counts[&NextActionKind::Advance], 1);
    assert!(first
        .next_actions
        .iter()
        .any(|action| action.task_id.as_deref() == Some("review-last")));
    assert_eq!(details.windows["open_lanes"].next_page, Some(2));
    assert_eq!(details.windows["open_lanes"].more, 71);
    let mut second = full.clone();
    second.window_details(StateWindowRequest {
        page: 2,
        all: false,
    });
    assert_eq!(second.open_lanes, full.open_lanes[50..100]);
    assert_eq!(
        second.details.as_ref().unwrap().next_action_counts,
        details.next_action_counts
    );
    let next = first.into_next_actions();
    assert_eq!(next.details.as_ref().unwrap().totals.len(), 1);
    assert!(next
        .details
        .as_ref()
        .unwrap()
        .windows
        .keys()
        .all(|key| key == "next_actions"));
}

#[test]
fn explicit_all_preserves_details_and_out_of_range_pages_keep_totals() {
    let full = snapshot();
    let mut all = full.clone();
    all.window_details(StateWindowRequest { page: 1, all: true });
    assert_eq!(all.open_lanes, full.open_lanes);
    assert_eq!(all.next_actions, full.next_actions);
    assert!(all.details.as_ref().unwrap().windows.is_empty());
    let mut beyond = full;
    beyond.window_details(StateWindowRequest {
        page: usize::MAX,
        all: false,
    });
    assert!(beyond.open_lanes.is_empty());
    assert_eq!(beyond.details.as_ref().unwrap().totals["open_lanes"], 121);
    assert!(beyond.details.as_ref().unwrap().windows["open_lanes"]
        .hint
        .contains("--page 1"));
}
