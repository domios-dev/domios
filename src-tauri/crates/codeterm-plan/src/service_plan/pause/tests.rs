use std::cell::RefCell;
use std::collections::HashMap;

use super::*;
use crate::plan::{OrchestratorRef, Plan};

struct FakePorts {
    sent: RefCell<Vec<String>>,
    schedules: RefCell<HashMap<String, bool>>,
}

impl PlanPausePorts for FakePorts {
    fn send_notice(&self, tab_id: &str, _text: &str) -> Result<(), String> {
        self.sent.borrow_mut().push(tab_id.into());
        Ok(())
    }

    fn schedule_enabled(&self, job_id: &str) -> Option<bool> {
        self.schedules.borrow().get(job_id).copied()
    }

    fn set_schedule_enabled(&self, job_id: &str, enabled: bool) -> Result<(), String> {
        self.schedules.borrow_mut().insert(job_id.into(), enabled);
        Ok(())
    }
}

impl FakePorts {
    fn recipients(&self) -> Vec<String> {
        self.sent.borrow().clone()
    }

    fn enabled(&self, job_id: &str) -> bool {
        self.schedule_enabled(job_id).unwrap()
    }
}

struct World {
    _dir: tempfile::TempDir,
    store: PlanStore,
    ports: FakePorts,
    plan_id: String,
    team_job: String,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Pause fixture".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.team_id = Some("team".into());
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    World {
        _dir: dir,
        store,
        plan_id: plan.id,
        team_job: "job".into(),
        ports: FakePorts {
            sent: RefCell::new(vec![]),
            schedules: RefCell::new(HashMap::from([("job".into(), true)])),
        },
    }
}

fn pause(w: &World) -> PlanLifecycleOutcome {
    execute_plan_pause(
        &w.store,
        &w.plan_id,
        "operator hold",
        AgentRef::default(),
        vec![PlanPauseAgent {
            tab_id: "worker".into(),
            name: "Worker".into(),
            role: PlanPauseRole::Worker,
            team_id: Some("team".into()),
            session_id: Some("session".into()),
            pause_notice: PlanPauseStepOutcome::Pending,
            resume_notice: None,
        }],
        vec![PlanPauseSchedule {
            job_id: w.team_job.clone(),
            name: "Job".into(),
            prior_enabled: true,
            pause_outcome: PlanPauseStepOutcome::Pending,
            resume_outcome: None,
        }],
        &w.ports,
        10,
    )
    .unwrap()
}

fn resume(w: &World, now_ms: u64) -> PlanLifecycleOutcome {
    execute_plan_resume(&w.store, &w.plan_id, &|_| true, &w.ports, now_ms).unwrap()
}

#[test]
fn plan_pause_record_round_trips() {
    let w = world();
    pause(&w);
    let plan = w.store.get(&w.plan_id).unwrap();
    assert_eq!(plan.pause_operations.len(), 1);

    let json = serde_json::to_value(&plan).unwrap();
    let decoded: Plan = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(decoded.pause_operations, plan.pause_operations);
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
    let preview = resume_preview(&(&w.store.get(&w.plan_id).unwrap()).into(), |_| true);
    assert!(!preview.applicable);
    assert!(preview.agents.is_empty());
}
