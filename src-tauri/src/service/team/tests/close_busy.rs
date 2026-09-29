use super::*;
use crate::plan::{OrchestratorRef, PlanStatus, TaskExecutionAttempt, TaskStatus};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;

fn busy_team_fixture() -> (ServiceFixture, String, String, String, String, String) {
    let (mux, team_id, orchestrator, _, worker) = team_members();
    let fixture = ServiceFixture::new(mux);
    let store = &fixture.service.plan_store;
    let plan = store
        .create_plan_for_team(
            "Busy Team plan".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab(&orchestrator),
            team_id.clone(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Busy task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .with_plan(&plan.id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = TaskStatus::InProgress;
            task.assigned_tab_id = Some(worker.clone());
            task.execution_attempts.push(TaskExecutionAttempt {
                id: "attempt_busy".into(),
                executor_tab_id: worker.clone(),
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    (fixture, team_id, orchestrator, worker, plan.id, task_id)
}

#[tokio::test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
async fn team_close_refuses_busy_worker() {
    let (fixture, team_id, orchestrator, worker, plan_id, task_id) = busy_team_fixture();

    let error = fixture
        .service
        .close_team(
            &team_id,
            TeamMutationCaller::AgentTab(orchestrator.clone()),
            false,
        )
        .await
        .unwrap_err();

    assert!(error.starts_with(crate::service::TEAM_CLOSE_BUSY_REFUSAL_PREFIX));
    assert!(error.contains(&worker) && error.contains(&task_id));
    assert!(fixture.service.mux.read().get_team(&team_id).is_some());
    assert!(fixture.service.mux.read().get_tab(&worker).is_some());
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    assert_eq!(plan.team_id.as_deref(), Some(team_id.as_str()));
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::InProgress);
}

#[tokio::test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
async fn forced_team_close_reports_killed_work_and_keeps_plan_flagged() {
    let (fixture, team_id, orchestrator, worker, plan_id, task_id) = busy_team_fixture();

    let result = fixture
        .service
        .close_team(&team_id, TeamMutationCaller::AgentTab(orchestrator), true)
        .await
        .unwrap();

    assert!(fixture.service.mux.read().get_team(&team_id).is_none());
    assert_eq!(result.killed_workers.len(), 1);
    assert_eq!(result.killed_workers[0].tab_id, worker);
    assert_eq!(result.killed_workers[0].task_ids, vec![task_id.clone()]);
    assert_eq!(result.unfinished_tasks.len(), 1);
    assert_eq!(result.unfinished_tasks[0].task_id, task_id);
    assert_eq!(result.flagged_plans.len(), 1);
    assert_eq!(result.flagged_plans[0].plan_id, plan_id);
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    assert_eq!(plan.status, PlanStatus::Active);
    assert!(plan.is_orphaned_active());
    assert!(fixture.service.validate_active_team_plan_bindings().is_ok());
}

#[test]
fn working_worker_without_a_task_still_blocks_close() {
    let (mut mux, team_id, orchestrator, planner, worker) = team_members();
    mux.get_tab_mut(&worker).unwrap().agent_state = Some(
        codeterm_types::pty::TabVerdict {
            state: codeterm_types::pty::TimelineState::Working,
            subtype: None,
        }
        .into(),
    );
    mux.get_tab_mut(&planner).unwrap().agent_state = Some(
        codeterm_types::pty::TabVerdict {
            state: codeterm_types::pty::TimelineState::Working,
            subtype: None,
        }
        .into(),
    );
    let members = vec![orchestrator, planner, worker.clone()];

    let busy = super::super::close_busy::team_close_busy(&mux, &[], &team_id, &members);

    assert_eq!(busy.tasks, vec![]);
    assert_eq!(busy.workers.len(), 1, "only Worker members count");
    assert_eq!(busy.workers[0].tab_id, worker);
    assert!(busy.workers[0].working);
    assert!(busy
        .refusal(&team_id)
        .starts_with(crate::service::TEAM_CLOSE_BUSY_REFUSAL_PREFIX));
}
