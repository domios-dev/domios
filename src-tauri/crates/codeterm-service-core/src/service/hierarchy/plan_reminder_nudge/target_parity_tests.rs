use super::*;
use crate::service::plan::state::TaskPresentation;
use codeterm_plan::service_plan::{ReminderPlanRead as LegacyPlan, ReminderTaskRead as Task};
mod reference_projection;
fn legacy_interval(plans: &[LegacyPlan], target: &PlanReminderTarget, default: u64) -> u64 {
    match target {
        PlanReminderTarget::Owner { plan_id } => plans
            .iter()
            .find(|plan| &plan.id == plan_id)
            .map_or(default, |plan| plan.control_loop.interval(default)),
        PlanReminderTarget::Worker => default,
    }
}
fn active_execution(plan: &LegacyPlan) -> bool {
    plan.status == PlanStatus::Active
        && plan.mode == PlanMode::Execution
        && plan.control_loop.mode != crate::plan::ControlLoopMode::Off
        && !plan.complete()
}

fn owner_reminder_eligible(plan: &LegacyPlan) -> bool {
    active_execution(plan)
        && plan.control_loop.owner_heartbeat()
        && plan
            .tasks
            .iter()
            .any(|task| task.status.counts_as_open_work() && task.status != TaskStatus::Paused)
}

fn worker_reminder_eligible(plan: &LegacyPlan, task: &Task) -> bool {
    if !active_execution(plan)
        || !matches!(
            task.status,
            TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::Blocked
        )
    {
        return false;
    }
    !matches!(
        task.projection.clone().presentation,
        TaskPresentation::WaitingOnDependencies | TaskPresentation::ParkedForLaterGroup
    )
}

fn legacy_targets(plans: &[LegacyPlan]) -> Vec<TimedPlanReminderTarget> {
    let mut targets = BTreeMap::<(String, String), PlanReminderTarget>::new();
    for plan in plans.iter().filter(|plan| owner_reminder_eligible(plan)) {
        if let Some(tab_id) = non_empty(plan.owner_tab_id.as_deref()) {
            targets.insert(
                (tab_id.to_string(), plan.id.clone()),
                PlanReminderTarget::Owner {
                    plan_id: plan.id.clone(),
                },
            );
        }
    }
    for plan in plans {
        for task in plan
            .tasks
            .iter()
            .filter(|task| worker_reminder_eligible(plan, task))
        {
            let Some(tab_id) = non_empty(task.assigned_tab_id.as_deref()) else {
                continue;
            };
            if !targets.keys().any(|(owner, _)| owner == tab_id) {
                targets
                    .entry((tab_id.to_string(), String::new()))
                    .or_insert(PlanReminderTarget::Worker);
            }
        }
    }
    targets
        .into_iter()
        .map(|((tab_id, _), target)| TimedPlanReminderTarget { tab_id, target })
        .collect()
}

#[test]
fn scheduler_scope_deadline_worker_and_popup_consumers_match_the_previous_shared_dto() {
    use super::tests::{plan, popup_reads, reads, task};
    use crate::plan::{ControlLoopMode, CuratorFailure};
    for case in 0..10 {
        let mut tasks = (0..8)
            .map(|i| task(&format!("task-{i}"), TaskStatus::InProgress, Some("worker")))
            .collect::<Vec<_>>();
        tasks.push(task("closed", TaskStatus::Done, Some(" retired ")));
        tasks.push(task("blocked", TaskStatus::Blocked, Some("blocked-worker")));
        let mut primary = plan("primary", "owner", PlanMode::Execution, tasks);
        primary.control_loop.heartbeat_interval_secs = Some(27);
        let mut secondary = plan(
            "secondary",
            "other-owner",
            PlanMode::Execution,
            vec![task("pending", TaskStatus::Pending, Some("owner"))],
        );
        match case {
            1 => primary.control_loop.mode = ControlLoopMode::Off,
            2 => primary.mode = PlanMode::Paused,
            3 => primary.mode = PlanMode::Planning,
            4 => {
                for task in &mut primary.tasks {
                    task.status = TaskStatus::Done
                }
            }
            5 => primary.tasks[0].depends_on = vec!["missing".into()],
            6 => {
                primary.tasks[0].status = TaskStatus::Pending;
                primary.tasks[0].group = Some("later".into());
                primary.tasks[1].group = Some("first".into());
                primary.task_groups=serde_json::from_value(serde_json::json!([{"key":"first","label":"First","color":null},{"key":"later","label":"Later","color":null}])).unwrap()
            }
            7 => {
                primary.control_loop.mode = ControlLoopMode::Curator;
                primary.control_loop.fallback_reason = Some(CuratorFailure::LaunchFailed)
            }
            8 => {
                primary.owner.tab_id = Some(" worker ".into());
                secondary.tasks[0].assigned_tab_id = Some("worker".into())
            }
            9 => {
                primary.control_loop.mode = ControlLoopMode::Off;
                secondary.tasks[0].status = TaskStatus::InProgress;
                secondary.tasks[0].assigned_tab_id = Some("worker".into())
            }
            _ => {}
        }
        let plans = vec![primary, secondary];
        let legacy = plans
            .iter()
            .map(|plan| LegacyPlan::for_test(plan, heartbeat_gate::scrubbed))
            .collect::<Vec<_>>();
        let candidates = reads(&plans);
        let expected = legacy_targets(&legacy);
        let actual = timed_plan_reminder_targets(&candidates);
        assert_eq!(actual, expected, "case {case}");
        let scopes = legacy
            .iter()
            .flat_map(|plan| {
                non_empty(plan.owner_tab_id.as_deref())
                    .into_iter()
                    .chain(
                        plan.tasks
                            .iter()
                            .filter_map(|task| non_empty(task.assigned_tab_id.as_deref())),
                    )
                    .map(move |tab| approval::scope_key(&plan.id, tab))
            })
            .collect::<HashSet<_>>();
        assert_eq!(
            approval::scope_keys(&candidates),
            scopes,
            "closed assignments retain their gate scope"
        );
        let counter = TurnCounter::default();
        assert_eq!(
            deadline_tabs(&candidates, &counter),
            expected
                .iter()
                .map(|target| target.tab_id.clone())
                .collect()
        );
        for target in &expected {
            assert_eq!(
                effective_interval(&candidates, &target.target, 90),
                legacy_interval(&legacy, &target.target, 90)
            );
        }
        for tab in [
            "owner",
            "worker",
            "other-owner",
            "blocked-worker",
            "retired",
            "",
        ] {
            let popup = popup_reads(&plans, tab);
            let ticks = vec![
                heartbeat_gate::delivery_tick(tab, "primary", 70, TickFact::Failed),
                heartbeat_gate::delivery_tick(tab, "", 80, TickFact::NotDue),
            ];
            for (enabled, interval, deadline) in [
                (true, 90, None),
                (true, 90, Some(123)),
                (true, 0, None),
                (false, 90, None),
            ] {
                assert_eq!(
                    project_for_tab(&popup, tab, interval, 200, deadline, &ticks, enabled),
                    reference_projection::project_for_tab(
                        &legacy, tab, interval, 200, deadline, &ticks, enabled
                    ),
                    "case {case}, tab {tab}"
                );
            }
        }
    }
}
