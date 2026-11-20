use super::*;
use crate::cron::clock::MockClock;
use crate::cron::store::{CronCancelReason, RunSession};
use crate::service::agent_runtime::model::RuntimeFailure;
use crate::service::spawn::{AgentSpawnSpec, ChatBackendLaunchMetadata, RuntimeOwnerLinkage};

fn staged(scene: &Scene, clock: Arc<MockClock>) -> (String, String, String, ChatBackendLaunchMetadata) {
    let svc = &scene.fixture.service;
    let workspace = svc.mux.read().list_workspaces()[0].id.clone();
    let tab = svc.mux.write().create_tab(&workspace).unwrap();
    let nonce = svc.ensure_session_binding_nonce(&tab);
    let mut store = scene.cron.lock();
    if store.list().is_empty() {
        *store = CronStore::new_with_path_and_clock(scene.fixture.temp.path().join("cron.json"), clock);
    }
    let job = store.create(serde_json::from_value(json!({
        "name":"Terminal fixture", "workspace_id":workspace, "schedule":"0 * * * *",
        "task":"opaque task", "agent":PROVIDER, "jitter_max_ms":0, "timeout_seconds":1200,
    })).unwrap()).unwrap();
    let run = uuid::Uuid::new_v4().to_string();
    let now = store.now_ms();
    store.record_run(&job.id, serde_json::from_value(json!({
        "run_id":run, "started_at":now, "status":"Running", "tab_id":tab,
    })).unwrap()).unwrap();
    drop(store);
    {
        let mut mux = svc.mux.write();
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_type = Some(PROVIDER.into());
        target.backing = crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: PROVIDER.into(), session_id: String::new(),
        };
    }
    let metadata = ChatBackendLaunchMetadata {
        agent: AgentSpawnSpec { provider: PROVIDER.into(), ..Default::default() },
        params: Default::default(),
        owner_linkage: Some(RuntimeOwnerLinkage::CronRun { job_id: job.id, run_id: run.clone() }),
        runtime_fork: None,
    };
    (tab, nonce, run, metadata)
}

#[test]
fn cancellation_retires_deadline_and_stale_completion_preserves_receipt() {
    let scene = Scene::new();
    let clock = MockClock::starting_at(1_700_000_000_000);
    let (tab, nonce, run, metadata) = staged(&scene, clock.clone());
    let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
    let svc = &scene.fixture.service;
    svc.prepare_runtime_cron_owner(&tab, job_id, &run).unwrap();
    svc.prepare_runtime_run_watcher(&tab, &metadata).unwrap();
    let mut cancelled = svc.run_watcher.runtime_runs.lock().get(&tab).unwrap().cancelled.subscribe();
    scene.cron.lock().set_run_session(job_id, &run, "original-session", Some("original-cwd"));
    scene.cron.lock().set_run_report(job_id, &run, "original-summary", "delivered".into()).unwrap();
    clock.advance_ms(10);
    assert_eq!(svc.cron_cancel_running(&scene.cron, job_id, CronCancelReason::UserCancel).unwrap(), 1);
    let original = scene.cron.lock().find_run_by_id(&run).unwrap().1.clone();
    assert_eq!(original.finished_at, Some(1_700_000_000_010));
    let deadline_fired = crate::async_runtime::block_on(async {
        let (release, expired) = tokio::sync::oneshot::channel();
        release.send(()).unwrap();
        runtime_deadline(&mut cancelled, async { expired.await.unwrap() }).await
    });
    assert!(!deadline_fired);
    assert!(svc.run_watcher.finish_runtime(svc, &tab, &nonce, false, true, "stale timeout".into()).is_none());
    assert!(!svc.run_watcher.has_runtime_run(&tab));
    let duplicate = crate::cron::store::commit(&scene.cron, |store| store.prepare_finish_run_with_summary(
        job_id, &run, CronRunStatus::Failed, Some(RunSession { session_id:"late", cwd:None }), Some("stale timeout"), None,
    )).unwrap();
    assert!(!duplicate.value.fresh);
    assert_eq!(scene.cron.lock().find_run_by_id(&run).unwrap().1, &original);
}

#[test]
fn terminal_startup_failure_before_and_after_registration_retires_only_its_run() {
    for registered in [false, true] {
        let scene = Scene::new();
        let clock = MockClock::starting_at(1_700_000_000_000);
        let (tab, nonce, run, metadata) = staged(&scene, clock.clone());
        let (other_tab, _, other_run, other_metadata) = staged(&scene, clock.clone());
        let svc = &scene.fixture.service;
        let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
        let RuntimeOwnerLinkage::CronRun { job_id: other_job, .. } = other_metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
        svc.prepare_runtime_cron_owner(&other_tab, other_job, &other_run).unwrap();
        svc.prepare_runtime_run_watcher(&other_tab, &other_metadata).unwrap();
        let changed = scene.ended("cron-run-finished");
        if registered {
            svc.prepare_runtime_cron_owner(&tab, job_id, &run).unwrap();
            svc.prepare_runtime_run_watcher(&tab, &metadata).unwrap();
            let foreign = AgentRunKind::Cron { job_id: other_job.clone(), run_id: other_run.clone() };
            assert!(svc.run_watcher.finish_runtime_owned(svc, &tab, RuntimeRunKey { nonce: &nonce, kind: Some(&foreign) }, false, true, "foreign deadline".into()).is_none());
            let error = RuntimeFailure::SpawnFailed { reason:"authoritative fixture refusal".into() };
            let kind = AgentRunKind::Cron { job_id: job_id.clone(), run_id: run.clone() };
            assert!(svc.run_watcher.runtime_start_result(svc, &tab, &nonce, &foreign, &error).is_none());
            assert!(svc.run_watcher.runtime_start_result(svc, &tab, "foreign-nonce", &kind, &error).is_none());
            svc.run_watcher.runtime_start_result(svc, &tab, &nonce, &kind, &error).unwrap().join().unwrap();
        } else {
            svc.fail_prepared_runtime_cron(&tab, &metadata, "pre-registration refusal");
        }
        let receipt = scene.cron.lock().find_run_by_id(&run).unwrap().1.clone();
        scene.await_cron_end(&changed, &receipt);
        assert!(svc.run_watcher.finish_runtime(svc, &tab, &nonce, false, false, "duplicate".into()).is_none());
        let store = scene.cron.lock();
        let ended = store.find_run_by_id(&run).unwrap().1;
        assert_eq!(ended.status, CronRunStatus::Failed);
        assert_eq!(ended.finished_at, Some(1_700_000_000_000));
        assert_eq!(ended.report_delivery.as_deref(), Some("no report target"));
        assert_eq!(store.get(job_id).unwrap().consecutive_failures, 1);
        assert!(store.get(job_id).unwrap().next_run.is_some());
        assert_eq!(store.find_run_by_id(&other_run).unwrap().1.status, CronRunStatus::Running);
        assert!(svc.run_watcher.has_runtime_run(&other_tab));
        assert!(svc.mux.read().get_tab(&other_tab).is_some());
        assert!(svc.mux.read().get_tab(&tab).is_none());
    }
}

#[test]
fn provider_start_pipe_failure_flows_through_launch_to_cron_terminal_owner() {
    let scene = Scene::new();
    let (tab, _, run, mut metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
    metadata.params.skip_permissions = Some(false);
    let svc = &scene.fixture.service;
    scene.fixture.set_agent_full_access(false);
    let _process = crate::service::agent_runtime::process::test_ports::register(&tab, || {
        let (process, peer) = crate::service::agent_runtime::process::test_ports::connected();
        drop(peer);
        process
    });
    let changed = scene.ended("cron-run-finished");
    assert!(svc.launch_chatbackend_runtime_existing(&tab, &metadata, json!({})).is_err());
    let receipt = scene.cron.lock().find_run_by_id(&run).unwrap().1.clone();
    scene.await_cron_end(&changed, &receipt);
    let store = scene.cron.lock();
    let (job, ended) = store.find_run_by_id(&run).unwrap();
    assert_eq!(ended.status, CronRunStatus::Failed);
    assert_eq!(ended.finished_at, Some(1_700_000_000_000));
    assert!(ended.session_id.is_none());
    assert_eq!(ended.report_delivery.as_deref(), Some("no report target"));
    assert_eq!(store.get(job).unwrap().consecutive_failures, 1);
}

#[test]
fn pending_start_deadline_preserves_watcher_for_late_binding() {
    let scene = Scene::new();
    let (tab, nonce, run, metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
    let svc = &scene.fixture.service;
    let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
    svc.prepare_runtime_cron_owner(&tab, job_id, &run).unwrap();
    svc.prepare_runtime_run_watcher(&tab, &metadata).unwrap();
    let kind = AgentRunKind::Cron { job_id: job_id.clone(), run_id: run.clone() };
    assert!(svc.run_watcher.runtime_start_result(svc, &tab, &nonce, &kind, &RuntimeFailure::Deadline).is_none());
    assert!(svc.run_watcher.has_runtime_run(&tab));
    assert_eq!(scene.cron.lock().find_run_by_id(&run).unwrap().1.status, CronRunStatus::Running);
    svc.mux.write().get_tab_mut(&tab).unwrap().agent_session_id = Some("late-valid-session".into());
    svc.run_watcher.finish_runtime(svc, &tab, &nonce, true, false, "late success".into()).unwrap().join().unwrap();
    let store = scene.cron.lock();
    let ended = store.find_run_by_id(&run).unwrap().1;
    assert_eq!(ended.status, CronRunStatus::Success);
    assert_eq!(ended.session_id.as_deref(), Some("late-valid-session"));
}

#[test]
fn injected_run_deadline_has_one_terminal_winner() {
    let scene = Scene::new();
    let (tab, nonce, run, metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
    let svc = &scene.fixture.service;
    let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
    svc.prepare_runtime_cron_owner(&tab, job_id, &run).unwrap();
    svc.prepare_runtime_run_watcher(&tab, &metadata).unwrap();
    let mut completion = svc.run_watcher.runtime_runs.lock().get(&tab).unwrap().cancelled.subscribe();
    assert!(crate::async_runtime::block_on(async {
        let (release, expired) = tokio::sync::oneshot::channel();
        release.send(()).unwrap();
        runtime_deadline(&mut completion, async { expired.await.unwrap() }).await
    }));
    let kind = AgentRunKind::Cron { job_id: job_id.clone(), run_id: run.clone() };
    svc.run_watcher.finish_runtime_owned(svc, &tab, RuntimeRunKey { nonce: &nonce, kind: Some(&kind) }, false, true, "injected hard deadline".into()).unwrap().join().unwrap();
    let receipt = scene.cron.lock().find_run_by_id(&run).unwrap().1.clone();
    assert!(svc.run_watcher.finish_runtime(svc, &tab, &nonce, true, false, "late completion".into()).is_none());
    let store = scene.cron.lock();
    assert_eq!(store.find_run_by_id(&run).unwrap().1, &receipt);
    assert_eq!(store.get(job_id).unwrap().consecutive_failures, 1);
}
