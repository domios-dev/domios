use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crate::multiplexer::state::MultiplexerState;
use crate::plan::{
    ExecutionIntent, OrchestratorRef, Plan, ProviderAllowance, TaskLaneMetadata, TaskStatus,
};
use crate::service::plan::{NewTask, PlanStore};
use crate::service::test_fixture::ServiceFixture;
use crate::service::SpawnAgentParams;

use super::super::complete_dispatch_transaction;

const WORKER_TAB: &str = "worker-tab";

/// Bounded failure deadline around an EVENT wait — never a readiness signal.
const EVENT_DEADLINE: Duration = Duration::from_secs(30);

fn mutating_allowance() -> ProviderAllowance {
    ProviderAllowance {
        remaining_pct: 90.0,
        observed_at: 3,
        stale_after_ms: u64::MAX,
        reserve_pct: 10.0,
    }
}

fn dispatched_task() -> (tempfile::TempDir, Arc<PlanStore>, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    let mut plan = Plan::new(
        "race".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "mutate".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let attempt_id = store
        .begin_task_dispatch_with_policy(
            &plan.id,
            &task_id,
            WORKER_TAB,
            "Worker",
            TaskLaneMetadata::default(),
            ExecutionIntent::Mutating,
            Some(mutating_allowance()),
            Some("claude"),
            3,
        )
        .unwrap();
    (dir, store, plan.id, task_id, attempt_id)
}

/// Sequenced on explicit channel handshakes, never on elapsed time: the close is
/// observed to have completed before the commit is allowed to proceed.
#[test]
fn late_dispatch_ready_after_close_cannot_resurrect_ownership() {
    let (_dir, store, plan_id, task_id, attempt_id) = dispatched_task();
    let (session_bound_tx, session_bound_rx) = mpsc::channel::<()>();
    let (close_done_tx, close_done_rx) = mpsc::channel::<()>();

    let commit_store = store.clone();
    let commit_plan = plan_id.clone();
    let commit_task = task_id.clone();
    let commit_attempt = attempt_id.clone();
    let worker = std::thread::spawn(move || {
        complete_dispatch_transaction(
            Instant::now() + EVENT_DEADLINE,
            |_| Ok(()),
            |_| {
                // Observable event: the session bound. Then wait for the close to
                // be fully applied before attempting the commit.
                session_bound_tx.send(()).unwrap();
                close_done_rx.recv_timeout(EVENT_DEADLINE).unwrap();
                Ok("worker-session".to_string())
            },
            || Ok(None),
            |_, session_id| {
                commit_store
                    .finish_task_dispatch(
                        &commit_plan,
                        &commit_task,
                        WORKER_TAB,
                        &commit_attempt,
                        session_id,
                        "worker-nonce",
                        TaskLaneMetadata::default(),
                        20,
                    )
                    .map_err(|error| error.to_string())
            },
            || Ok(()),
        )
    });

    session_bound_rx.recv_timeout(EVENT_DEADLINE).unwrap();
    // The Worker tab goes away mid-`awaiting_dispatch_ready`.
    store
        .flag_tab_gone(&plan_id, &task_id, WORKER_TAB, 10)
        .unwrap();
    close_done_tx.send(()).unwrap();

    let outcome = worker.join().unwrap();
    assert!(
        outcome.is_err(),
        "a dispatch_ready landing after close must not commit"
    );

    let task = store
        .get(&plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_ne!(task.status, TaskStatus::InProgress);
    assert!(task.assigned_tab_id.is_none());
    assert!(task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap()
        .retired_at
        .is_some());
    assert!(!store.worker_startup_attempt_owns(&plan_id, &task_id, WORKER_TAB, &attempt_id, None));
}

fn fixture() -> ServiceFixture {
    let (mux, _) = MultiplexerState::new();
    ServiceFixture::new(mux)
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn general_agent_creation_is_unobstructed() {
    let fixture = fixture();
    let canonical = fixture.temp.path().display().to_string();
    for role in ["Orchestrator", "Manager"] {
        let mut params = SpawnAgentParams {
            cwd: Some(canonical.clone()),
            role: Some(role.into()),
            ..Default::default()
        };
        // No lane is provisioned and the requested cwd is left exactly as asked.
        let lane = fixture
            .service
            .provision_worker_lane_for_spawn(&mut params, "ga")
            .unwrap();
        assert!(lane.is_none(), "{role} must not be lane-provisioned");
        assert_eq!(params.cwd.as_deref(), Some(canonical.as_str()));
        assert!(params.provisioned_cwd.is_none());
    }
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn plain_shell_tab_creation_is_unobstructed() {
    let fixture = fixture();
    let canonical = fixture.temp.path().display().to_string();
    let mut params = SpawnAgentParams {
        cwd: Some(canonical.clone()),
        role: None,
        ..Default::default()
    };

    let lane = fixture
        .service
        .provision_worker_lane_for_spawn(&mut params, "shell")
        .unwrap();

    assert!(lane.is_none());
    assert_eq!(params.cwd.as_deref(), Some(canonical.as_str()));
    assert!(params.provisioned_cwd.is_none());
}
