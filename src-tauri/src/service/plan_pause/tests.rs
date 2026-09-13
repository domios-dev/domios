use std::cell::RefCell;
use std::collections::HashSet;

use codeterm_types::plan::{
    AgentRef, OrchestratorRef, Plan, PlanMode, PlanPauseRole, PlanPauseStage,
    PlanPauseStepOutcome, PlannerRef, PLAN_PAUSE_EFFECTS,
};

use super::*;
use crate::cron::store::{CronCreateParams, CronStore};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::service::hierarchy::plan_reminder_nudge::timed_plan_reminder_targets;
use crate::service::plan::pause_operation::plan_notices_suppressed;
use crate::service::plan::{NewTask, PlanStore};

const FOREIGN_TEAM: &str = "team_foreign";

struct FakePorts {
    sent: RefCell<Vec<(String, String)>>,
    cron: RefCell<CronStore>,
}

impl PlanPausePorts for FakePorts {
    fn send_notice(&self, tab_id: &str, text: &str) -> Result<(), String> {
        self.sent.borrow_mut().push((tab_id.into(), text.into()));
        Ok(())
    }

    fn schedule_enabled(&self, job_id: &str) -> Option<bool> {
        self.cron.borrow().get(job_id).map(|job| job.enabled)
    }

    fn set_schedule_enabled(&self, job_id: &str, enabled: bool) -> Result<(), String> {
        self.cron
            .borrow_mut()
            .set_enabled(job_id, enabled)
            .map(|_| ())
    }
}

impl FakePorts {
    fn recipients(&self) -> Vec<String> {
        self.sent.borrow().iter().map(|(tab, _)| tab.clone()).collect()
    }

    fn enabled(&self, job_id: &str) -> bool {
        self.cron.borrow().get(job_id).unwrap().enabled
    }
}

struct World {
    dir: tempfile::TempDir,
    store: PlanStore,
    mux: MultiplexerState,
    ports: FakePorts,
    plan_id: String,
    other_plan_id: String,
    task_id: String,
    team_id: String,
    workspace_id: String,
    orchestrator: String,
    planner: String,
    worker: String,
    foreign: String,
    team_job: String,
    disabled_team_job: String,
    foreign_job: String,
}

fn make_live(mux: &mut MultiplexerState, tab_id: &str, title: &str, team_id: &str) {
    let tab = mux.get_tab_mut(tab_id).unwrap();
    tab.title = title.into();
    tab.team_id = Some(team_id.into());
    tab.agent_type = Some("provider-neutral".into());
    tab.agent_session_id = Some(format!("session-{tab_id}"));
    tab.session_binding_nonce = Some(format!("nonce-{tab_id}"));
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    tab.chat_view = true;
    tab.pty_id = Some(format!("pty-{tab_id}"));
}

fn cron_params(name: &str, workspace_id: &str, team_id: &str, enabled: bool) -> CronCreateParams {
    CronCreateParams {
        name: name.into(),
        workspace_id: workspace_id.into(),
        team_id: Some(team_id.into()),
        detach_from_team: false,
        schedule: "0 * * * *".into(),
        task: "run the job".into(),
        description: None,
        agent: Some("provider-neutral".into()),
        model: None,
        reasoning_effort: None,
        cwd: None,
        role: None,
        skill: None,
        creator_tab_id: None,
        report_target_tab_id: None,
        enabled: Some(enabled),
        max_history: None,
        oneshot: None,
        jitter_max_ms: Some(0),
        tz: None,
        timeout_seconds: None,
    }
}

fn add_worker(mux: &mut MultiplexerState, workspace_id: &str, orchestrator: &str) -> String {
    let tab = mux.create_tab(workspace_id).unwrap();
    mux.set_tab_role(&tab, AgentRole::Worker, Some(orchestrator))
        .unwrap();
    tab
}

fn save_plan(store: &PlanStore, plan: &mut Plan, assignee: &str) -> String {
    store.save(plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker lane".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, assignee, None, 3)
        .unwrap();
    task_id
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let (mut mux, orchestrator) = MultiplexerState::new();
    mux.set_orchestrator_role(&orchestrator).unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let planner = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role_kind(
        &planner,
        AgentRole::Manager,
        Some("planner"),
        Some(&orchestrator),
    )
    .unwrap();
    let worker = add_worker(&mut mux, &workspace_id, &orchestrator);
    let idle_worker = add_worker(&mut mux, &workspace_id, &orchestrator);
    let utility = mux.create_tab(&workspace_id).unwrap();
    let foreign = add_worker(&mut mux, &workspace_id, &orchestrator);

    make_live(&mut mux, &orchestrator, "Orchestrator", &team_id);
    make_live(&mut mux, &planner, "Planner", &team_id);
    make_live(&mut mux, &worker, "Worker A", &team_id);
    make_live(&mut mux, &utility, "Team shell", &team_id);
    make_live(&mut mux, &foreign, "Foreign lead", FOREIGN_TEAM);
    mux.get_tab_mut(&idle_worker).unwrap().team_id = Some(team_id.clone());

    let mut plan = Plan::new(
        "Pause fixture".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(&orchestrator),
    );
    plan.team_id = Some(team_id.clone());
    plan.mode = PlanMode::Execution;
    plan.planner = Some(PlannerRef::Tab {
        tab_id: planner.clone(),
        session_id: None,
        title: None,
    });
    let task_id = save_plan(&store, &mut plan, &worker);

    let mut other = Plan::new(
        "Unrelated plan".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(&foreign),
    );
    other.team_id = Some(FOREIGN_TEAM.into());
    other.mode = PlanMode::Execution;
    save_plan(&store, &mut other, &foreign);

    let mut cron = CronStore::new_with_path(dir.path().join("crons.json"));
    let team_job = cron
        .create(cron_params("Team digest", &workspace_id, &team_id, true))
        .unwrap()
        .id;
    let disabled_team_job = cron
        .create(cron_params("Team parked", &workspace_id, &team_id, false))
        .unwrap()
        .id;
    let foreign_job = cron
        .create(cron_params("Foreign digest", &workspace_id, FOREIGN_TEAM, true))
        .unwrap()
        .id;

    World {
        dir,
        store,
        mux,
        ports: FakePorts {
            sent: RefCell::new(vec![]),
            cron: RefCell::new(cron),
        },
        plan_id: plan.id,
        other_plan_id: other.id,
        task_id,
        team_id,
        workspace_id,
        orchestrator,
        planner,
        worker,
        foreign,
        team_job,
        disabled_team_job,
        foreign_job,
    }
}

fn pause(w: &World) -> PlanLifecycleOutcome {
    let plan = w.store.get(&w.plan_id).unwrap();
    let agents = live_plan_agents(&w.mux, &plan);
    let schedules = enabled_team_schedules(w.ports.cron.borrow().list(), &w.team_id);
    execute_plan_pause(
        &w.store,
        &w.plan_id,
        "operator hold",
        AgentRef::default(),
        agents,
        schedules,
        &w.ports,
        10,
    )
    .unwrap()
}

fn resume(w: &World, now_ms: u64) -> PlanLifecycleOutcome {
    let plan = w.store.get(&w.plan_id).unwrap();
    let live: HashSet<String> = open_pause_operation(&plan)
        .map(|op| {
            op.agents
                .iter()
                .filter(|agent| recorded_agent_is_live(&w.mux, agent))
                .map(|agent| agent.tab_id.clone())
                .collect()
        })
        .unwrap_or_default();
    execute_plan_resume(
        &w.store,
        &w.plan_id,
        &|agent| live.contains(&agent.tab_id),
        &w.ports,
        now_ms,
    )
    .unwrap()
}

fn task_binding(w: &World) -> (codeterm_types::plan::TaskStatus, Option<String>) {
    let plan = w.store.get(&w.plan_id).unwrap();
    let task = plan.tasks.iter().find(|t| t.id == w.task_id).unwrap();
    (task.status.clone(), task.assigned_tab_id.clone())
}

fn join_after_pause(w: &mut World) -> String {
    let joiner = add_worker(&mut w.mux, &w.workspace_id, &w.orchestrator);
    make_live(&mut w.mux, &joiner, "Late joiner", &w.team_id.clone());
    joiner
}

#[test]
fn plan_pause_resume_record_round_trips_and_legacy_plan_loads_without_it() {
    let w = world();
    pause(&w);
    let plan = w.store.get(&w.plan_id).unwrap();
    assert_eq!(plan.pause_operations.len(), 1);

    let json = serde_json::to_value(&plan).unwrap();
    let decoded: Plan = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(decoded.pause_operations, plan.pause_operations);

    let mut legacy = json;
    legacy
        .as_object_mut()
        .unwrap()
        .remove("pause_operations");
    let legacy: Plan = serde_json::from_value(legacy).unwrap();
    assert!(legacy.pause_operations.is_empty());
    let untouched = w.store.get(&w.other_plan_id).unwrap();
    assert!(serde_json::to_value(&untouched)
        .unwrap()
        .get("pause_operations")
        .is_none());
    assert!(w.dir.path().exists());
}

#[test]
fn plan_pause_resume_preview_is_computed_from_live_state_without_mutation() {
    let w = world();
    let plan = w.store.get(&w.plan_id).unwrap();
    let agents = live_plan_agents(&w.mux, &plan);
    let schedules = enabled_team_schedules(w.ports.cron.borrow().list(), &w.team_id);

    let preview = pause_preview(&plan, &agents, &schedules);

    assert!(preview.applicable);
    let listed: Vec<_> = preview
        .agents
        .iter()
        .map(|a| (a.tab_id.clone(), a.role, a.available))
        .collect();
    assert_eq!(
        listed,
        vec![
            (w.orchestrator.clone(), PlanPauseRole::Orchestrator, true),
            (w.planner.clone(), PlanPauseRole::Planner, true),
            (w.worker.clone(), PlanPauseRole::Worker, true),
        ]
    );
    let jobs: Vec<_> = preview.schedules.iter().map(|s| s.job_id.clone()).collect();
    assert_eq!(jobs, vec![w.team_job.clone()]);
    assert_eq!(preview.effects, PLAN_PAUSE_EFFECTS.to_vec());
    assert_eq!(w.store.get(&w.plan_id).unwrap().mode, PlanMode::Execution);
    assert!(w.ports.enabled(&w.team_job));
    assert!(w.ports.recipients().is_empty());
}

#[test]
fn plan_pause_resume_first_pause_records_exact_agents_and_schedules_and_notifies_once() {
    let w = world();
    let binding_before = task_binding(&w);

    let outcome = pause(&w);

    assert!(outcome.applied);
    assert!(outcome.stride_id.is_some());
    let plan = w.store.get(&w.plan_id).unwrap();
    assert_eq!(plan.mode, PlanMode::Paused);
    assert_eq!(plan.paused_from_mode, Some(PlanMode::Execution));
    let op = plan.pause_operations.last().unwrap();
    assert_eq!(Some(op), outcome.operation.as_ref());
    assert_eq!(op.stage, PlanPauseStage::Complete);
    assert_eq!(op.paused_from_mode, PlanMode::Execution);
    let agents: Vec<_> = op
        .agents
        .iter()
        .map(|a| (a.tab_id.clone(), a.name.clone(), a.role, a.pause_notice.clone()))
        .collect();
    assert_eq!(
        agents,
        vec![
            (
                w.orchestrator.clone(),
                "Orchestrator".to_string(),
                PlanPauseRole::Orchestrator,
                PlanPauseStepOutcome::Delivered
            ),
            (
                w.planner.clone(),
                "Planner".to_string(),
                PlanPauseRole::Planner,
                PlanPauseStepOutcome::Delivered
            ),
            (
                w.worker.clone(),
                "Worker A".to_string(),
                PlanPauseRole::Worker,
                PlanPauseStepOutcome::Delivered
            ),
        ]
    );
    let schedules: Vec<_> = op
        .schedules
        .iter()
        .map(|s| (s.job_id.clone(), s.prior_enabled, s.pause_outcome.clone()))
        .collect();
    assert_eq!(
        schedules,
        vec![(w.team_job.clone(), true, PlanPauseStepOutcome::Disabled)]
    );
    assert_eq!(op.effects, PLAN_PAUSE_EFFECTS.to_vec());
    assert_eq!(
        w.ports.recipients(),
        vec![w.orchestrator.clone(), w.planner.clone(), w.worker.clone()]
    );

    assert!(!w.ports.enabled(&w.team_job));
    assert!(!w.ports.enabled(&w.disabled_team_job));
    assert!(w.ports.enabled(&w.foreign_job));
    assert_eq!(task_binding(&w), binding_before);
    let other = w.store.get(&w.other_plan_id).unwrap();
    assert_eq!(other.mode, PlanMode::Execution);
    assert!(other.pause_operations.is_empty());
}

#[test]
fn plan_pause_resume_second_pause_is_noop_and_keeps_original_record() {
    let mut w = world();
    pause(&w);
    let original = w.store.get(&w.plan_id).unwrap().pause_operations;
    join_after_pause(&mut w);

    let again = pause(&w);

    assert!(!again.applied);
    assert_eq!(again.operation.as_ref(), original.last());
    assert_eq!(w.store.get(&w.plan_id).unwrap().pause_operations, original);
    assert_eq!(w.ports.recipients().len(), 3);
}

#[test]
fn plan_pause_resume_paused_plan_suppresses_reminders_heartbeat_and_notices_only_for_it() {
    let w = world();
    let tabs = |plans: &[Plan]| -> HashSet<String> {
        timed_plan_reminder_targets(plans)
            .into_iter()
            .map(|target| target.tab_id)
            .collect()
    };
    let before = tabs(&w.store.list_full());
    assert!(before.contains(&w.orchestrator));
    assert!(before.contains(&w.worker));

    pause(&w);

    let after = tabs(&w.store.list_full());
    assert!(!after.contains(&w.orchestrator));
    assert!(!after.contains(&w.worker));
    assert!(after.contains(&w.foreign));
    assert_eq!(
        w.store.prepend_heartbeat_for_plan(&w.plan_id, "body"),
        "body"
    );
    assert_ne!(
        w.store.prepend_heartbeat_for_plan(&w.other_plan_id, "body"),
        "body"
    );
    assert!(plan_notices_suppressed(&w.store.get(&w.plan_id).unwrap()));
    assert!(!plan_notices_suppressed(
        &w.store.get(&w.other_plan_id).unwrap()
    ));
}

#[test]
fn plan_pause_resume_preview_marks_dead_recorded_tab_and_omits_post_pause_joiner() {
    let mut w = world();
    let op_id = pause(&w).operation.unwrap().id;
    let joiner = join_after_pause(&mut w);
    let worker = w.worker.clone();
    w.mux.remove_tab(&worker);

    let plan = w.store.get(&w.plan_id).unwrap();
    let preview = resume_preview(&plan, |agent| recorded_agent_is_live(&w.mux, agent));

    assert!(preview.applicable);
    assert_eq!(preview.operation_id.as_deref(), Some(op_id.as_str()));
    assert_eq!(preview.restore_mode, Some(PlanMode::Execution));
    let listed: Vec<_> = preview
        .agents
        .iter()
        .map(|a| (a.tab_id.clone(), a.available))
        .collect();
    assert_eq!(
        listed,
        vec![
            (w.orchestrator.clone(), true),
            (w.planner.clone(), true),
            (w.worker.clone(), false),
        ]
    );
    assert!(preview.agents.iter().all(|a| a.tab_id != joiner));
    let jobs: Vec<_> = preview.schedules.iter().map(|s| s.job_id.clone()).collect();
    assert_eq!(jobs, vec![w.team_job.clone()]);
    assert!(!pause_preview(&plan, &[], &[]).applicable);
}

#[test]
fn plan_pause_resume_restores_recorded_effects_and_notifies_live_recorded_agents_once() {
    let mut w = world();
    let binding_before = task_binding(&w);
    pause(&w);
    let joiner = join_after_pause(&mut w);
    let worker = w.worker.clone();
    w.mux.remove_tab(&worker);
    w.ports.sent.borrow_mut().clear();

    let outcome = resume(&w, 20);

    assert!(outcome.applied);
    assert_eq!(outcome.mode, PlanMode::Execution);
    let plan = w.store.get(&w.plan_id).unwrap();
    assert_eq!(plan.mode, PlanMode::Execution);
    assert_eq!(plan.paused_from_mode, None);
    assert_eq!(
        w.ports.recipients(),
        vec![w.orchestrator.clone(), w.planner.clone()]
    );
    assert!(!w.ports.recipients().contains(&joiner));
    assert!(w.ports.enabled(&w.team_job));
    assert!(!w.ports.enabled(&w.disabled_team_job));
    assert!(w.ports.enabled(&w.foreign_job));

    let op = plan.pause_operations.last().unwrap();
    let resume = op.resume.as_ref().unwrap();
    assert_eq!(resume.restored_mode, PlanMode::Execution);
    assert_eq!(resume.stage, PlanPauseStage::Complete);
    let notices: Vec<_> = op
        .agents
        .iter()
        .map(|a| (a.tab_id.clone(), a.resume_notice.clone()))
        .collect();
    assert_eq!(
        notices,
        vec![
            (
                w.orchestrator.clone(),
                Some(PlanPauseStepOutcome::Delivered)
            ),
            (w.planner.clone(), Some(PlanPauseStepOutcome::Delivered)),
            (w.worker.clone(), Some(PlanPauseStepOutcome::Unavailable)),
        ]
    );
    assert_eq!(
        op.schedules[0].resume_outcome,
        Some(PlanPauseStepOutcome::Enabled)
    );
    assert_eq!(task_binding(&w), binding_before);
}

#[test]
fn plan_pause_resume_repeated_resume_is_noop_without_duplicate_notice_or_reenable() {
    let w = world();
    pause(&w);
    resume(&w, 20);
    let record = w.store.get(&w.plan_id).unwrap().pause_operations;
    let sent = w.ports.recipients().len();
    w.ports.set_schedule_enabled(&w.team_job, false).unwrap();

    let again = resume(&w, 30);

    assert!(!again.applied);
    assert_eq!(again.mode, PlanMode::Execution);
    assert_eq!(w.ports.recipients().len(), sent);
    assert!(!w.ports.enabled(&w.team_job));
    assert_eq!(w.store.get(&w.plan_id).unwrap().pause_operations, record);
}

#[test]
fn plan_pause_resume_resume_on_non_paused_plan_is_noop() {
    let w = world();

    let outcome = resume(&w, 20);

    assert!(!outcome.applied);
    assert_eq!(outcome.mode, PlanMode::Execution);
    assert!(outcome.operation.is_none());
    assert!(w.ports.recipients().is_empty());
    assert!(w.ports.enabled(&w.team_job));
    assert!(w.store.get(&w.plan_id).unwrap().pause_operations.is_empty());
    let preview = resume_preview(&w.store.get(&w.plan_id).unwrap(), |_| true);
    assert!(!preview.applicable);
    assert!(preview.agents.is_empty());
}

#[test]
fn plan_pause_resume_legacy_pause_without_record_resumes_without_notices() {
    let w = world();
    w.store
        .pause_plan(&w.plan_id, "legacy hold", AgentRef::default(), 10)
        .unwrap();

    let outcome = resume(&w, 20);

    assert!(outcome.applied);
    assert!(outcome.operation.is_none());
    assert_eq!(w.store.get(&w.plan_id).unwrap().mode, PlanMode::Execution);
    assert!(w.ports.recipients().is_empty());
}
