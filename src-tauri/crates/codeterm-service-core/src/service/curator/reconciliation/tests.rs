use super::*;
use crate::cron::store::{commit, CronChange, CronRunInfo, CronRunStatus, CronStore, Prepared};
use crate::plan::{curator_delay, CuratorFailure, PlanMode};
use crate::service::curator::CuratorTabs;
use crate::service::test_fixture::ServiceFixture;
use crate::service::AuthenticatedPlanCaller;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Barrier};
use std::time::Duration;

const NOW: u64 = 1_700_000_000_000;
const DEADLINE: Duration = Duration::from_secs(3);

fn fixture(running_cron: bool) -> (ServiceFixture, String, crate::CronState) {
    let (fixture, id, _, owner) = crate::service::plan_authority::test_fixture::fixture();
    let service = &fixture.service;
    let owner = owner.tab_id.unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(NOW);
    let cron = Arc::new(Mutex::new(CronStore::new_with_path_and_clock(
        fixture.temp.path().join("crons.json"),
        clock,
    )));
    assert!(service.set_ext(cron.clone()).is_ok());
    service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("provider-neutral"),
    ));
    service
        .mux
        .write()
        .get_tab_mut(&owner)
        .unwrap()
        .agent_session_model = Some("fixture-model".into());
    service
        .plan_store
        .with_plan(&id, NOW, |plan| {
            plan.mode = PlanMode::Execution;
            Ok(())
        })
        .unwrap();
    if running_cron {
        let state = service.control_loop_show(&id).unwrap();
        let job_id = state.cron_id.unwrap();
        let mut job = cron.lock().get(&job_id).unwrap().clone();
        job.last_run = Some(CronRunInfo {
            run_id: "running-round".into(),
            started_at: NOW,
            finished_at: None,
            status: CronRunStatus::Running,
            tab_id: Some(owner.clone()),
            session_id: None,
            cwd: None,
            summary: None,
            report_delivery: None,
            cancel_reason: None,
        });
        commit(&cron, |_| {
            Ok(Prepared::new(CronChange::Patch(job.clone()), ()))
        })
        .unwrap();
        cron.lock().register_tab(&owner, &job_id, "running-round");
    }
    service
        .plan_store
        .with_plan(&id, NOW, |plan| {
            plan.control_loop.cron_id = None;
            plan.control_loop.next_at = None;
            Ok(())
        })
        .unwrap();
    fixture.ui.0.lock().clear();
    (fixture, id, cron)
}

#[derive(Clone, Copy, Debug)]
enum Repair {
    Read,
    Event,
    Notification,
}

fn concurrent_repairs(second: Repair, running_cron: bool) {
    let (fixture, id, cron) = fixture(running_cron);
    let service = &fixture.service;
    let before = service.plan_store.get(&id).unwrap().control_loop;
    assert_eq!(before.next_at, None);
    assert_eq!(before.cron_id, None);
    let (snapshot_tx, snapshot_rx) = mpsc::channel();
    let (contended_tx, contended_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let first = AtomicBool::new(true);
    let boundary = service
        .extensions
        .get_or_init(test_boundary::Boundary::default);
    *boundary.0.lock() = Some(Arc::new(test_boundary::Probe {
        plan_id: id.clone(),
        snapshot: Arc::new(move || {
            if first.swap(false, Ordering::SeqCst) {
                snapshot_tx.send(()).unwrap();
                release_rx.lock().recv_timeout(DEADLINE).unwrap();
            }
        }),
        contended: Arc::new(move || {
            contended_tx.send(()).unwrap();
        }),
    }));
    let start = Barrier::new(3);
    std::thread::scope(|scope| {
        let read = scope.spawn(|| {
            start.wait();
            service.control_loop_show(&id).unwrap()
        });
        let repair = scope.spawn(|| {
            start.wait();
            match second {
                Repair::Read => {
                    service.control_loop_show(&id).unwrap();
                }
                Repair::Event => service.reconcile_curator_for_test(&id, NOW),
                Repair::Notification => service.clear_notifications(None),
            }
        });
        start.wait();
        let snapshot = snapshot_rx.recv_timeout(DEADLINE);
        let contended = contended_rx.recv_timeout(DEADLINE);
        let _ = release_tx.send(());
        read.join().unwrap();
        repair.join().unwrap();
        assert!(
            snapshot.is_ok(),
            "first snapshot: {second:?}, running={running_cron}"
        );
        assert!(
            contended.is_ok(),
            "second caller contends: {second:?}, running={running_cron}"
        );
    });
    *boundary.0.lock() = None;
    let state = service.plan_store.get(&id).unwrap().control_loop;
    assert_eq!(state.next_at, Some(NOW + curator_delay(None) * 1000));
    assert!(state.cron_id.is_some());
    assert_eq!(state.last_round, before.last_round);
    assert_eq!(state.running, before.running);
    let jobs = cron
        .lock()
        .list()
        .iter()
        .filter(|job| job.protected && job.task == id)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        jobs.len(),
        1,
        "no orphan cron: {second:?}, running={running_cron}"
    );
    assert!(jobs[0].enabled);
    assert_eq!(Some(&jobs[0].id), state.cron_id.as_ref());
    assert_eq!(jobs[0].next_run, state.next_at);
    if running_cron {
        assert!(cron.lock().is_job_running(&jobs[0].id));
    }
    let events = fixture.ui.0.lock().clone();
    for _ in 0..3 {
        assert_eq!(service.control_loop_show(&id).unwrap(), state);
    }
    assert_eq!(*fixture.ui.0.lock(), events);
    assert_eq!(
        cron.lock()
            .list()
            .iter()
            .filter(|job| job.protected && job.task == id)
            .count(),
        1
    );
}

#[test]
fn curator_concurrent_read_repairs_publish_one_protected_cron() {
    concurrent_repairs(Repair::Read, false);
}

#[test]
fn curator_event_and_read_repairs_share_the_plan_gate() {
    concurrent_repairs(Repair::Event, false);
}

#[test]
fn curator_notification_and_read_repairs_share_the_plan_gate() {
    concurrent_repairs(Repair::Notification, false);
}

#[test]
fn curator_concurrent_repairs_reuse_the_running_protected_cron() {
    for second in [Repair::Read, Repair::Event, Repair::Notification] {
        concurrent_repairs(second, true);
    }
}

#[test]
fn curator_prepare_failure_keeps_fallback_and_rearms_protected_job() {
    use crate::service::role_policy::RolePolicy;
    let (fixture, id, cron) = fixture(false);
    let service = &fixture.service;
    service
        .set_ext(CuratorTabs::default())
        .unwrap_or_else(|_| panic!("runtime"));
    let state = service.control_loop_show(&id).unwrap();
    let job = cron.lock().get(&state.cron_id.unwrap()).unwrap().clone();
    let owner = service.plan_store.get(&id).unwrap().owner.tab_id.unwrap();
    service
        .mux
        .write()
        .get_tab_mut(&owner)
        .unwrap()
        .agent_session_model = None;
    assert!(super::super::policy::POLICY
        .prepare_cron(service, &job, NOW)
        .unwrap()
        .is_none());
    let state = service.plan_store.get(&id).unwrap().control_loop;
    assert_eq!(
        state.fallback_reason,
        Some(CuratorFailure::MirrorUnavailable)
    );
    assert_eq!(state.next_at, Some(NOW + curator_delay(None) * 1000));
    let persisted = cron.lock().get(&job.id).unwrap().clone();
    assert!(persisted.enabled);
    assert_eq!(persisted.next_run, state.next_at);
    assert!(state.owner_heartbeat());
}

#[test]
fn curator_owner_setting_disables_job_and_agents_cannot_change_it() {
    use crate::plan::ControlLoopMode;
    use codeterm_plan::service_plan::curator::ControlLoopUpdate;
    let (fixture, id, cron) = fixture(false);
    let service = &fixture.service;
    let job_id = service.control_loop_show(&id).unwrap().cron_id.unwrap();
    let owner = service.plan_store.get(&id).unwrap().owner.tab_id.unwrap();
    let agent = AuthenticatedPlanCaller {
        tab_id: Some(owner),
        ..Default::default()
    };
    for mode in [
        ControlLoopMode::Curator,
        ControlLoopMode::Heartbeat,
        ControlLoopMode::Off,
    ] {
        assert!(service
            .control_loop_set(
                &id,
                &agent,
                ControlLoopUpdate {
                    mode,
                    heartbeat_interval_secs: None
                },
                NOW
            )
            .is_err());
    }
    let state = service
        .control_loop_set(
            &id,
            &Default::default(),
            ControlLoopUpdate {
                mode: ControlLoopMode::Off,
                heartbeat_interval_secs: None,
            },
            NOW,
        )
        .unwrap();
    assert_eq!(state.mode, ControlLoopMode::Off);
    assert_eq!(state.next_at, None);
    let job = cron.lock().get(&job_id).unwrap().clone();
    assert!(!job.enabled);
    assert_eq!(job.next_run, None);
    service
        .curator_fire_failed(&id, CuratorFailure::LaunchFailed, NOW + 1)
        .unwrap();
    service.reconcile_curator_for_test(&id, NOW + 1);
    let state = service.plan_store.get(&id).unwrap().control_loop;
    assert_eq!(state.mode, ControlLoopMode::Off);
    assert!(!state.owner_heartbeat());
    assert_eq!(state.next_at, None);
    assert!(!cron.lock().get(&job_id).unwrap().enabled);
}
