use super::*;
use crate::cron::clock::MockClock;
use crate::cron::store::{CronCancelReason, RunSession};
use crate::service::agent_runtime::model::RuntimeFailure;
use crate::service::spawn::{AgentSpawnSpec, ChatBackendLaunchMetadata, RuntimeOwnerLinkage};

fn staged(scene: &Scene, clock: Arc<MockClock>) -> (String, String, String, ChatBackendLaunchMetadata) {
    staged_with_history(scene, clock, None)
}

fn staged_with_history(scene: &Scene, clock: Arc<MockClock>, max_history: Option<usize>) -> (String, String, String, ChatBackendLaunchMetadata) {
    let svc = &scene.fixture.service;
    let workspace = svc.mux.read().list_workspaces()[0].id.clone();
    let tab = svc.mux.write().create_tab(&workspace).unwrap();
    let nonce = svc.ensure_session_binding_nonce(&tab);
    let mut store = scene.cron.lock();
    if store.list().is_empty() && !store.commits_to_storage() {
        *store = CronStore::new_with_path_and_clock(scene.fixture.temp.path().join("cron.json"), clock);
    }
    drop(store);
    let params: CronCreateParams = serde_json::from_value(json!({
        "name":"Terminal fixture", "workspace_id":workspace, "schedule":"0 * * * *",
        "task":"opaque task", "agent":PROVIDER, "jitter_max_ms":0, "timeout_seconds":1200, "max_history":max_history,
    })).unwrap();
    let job = crate::cron::store::commit(&scene.cron, |store| store.prepare_create(params.clone())).unwrap().value;
    let run = uuid::Uuid::new_v4().to_string();
    let now = scene.cron.lock().now_ms();
    let receipt: CronRunInfo = serde_json::from_value(json!({
        "run_id":run, "started_at":now, "status":"Running", "tab_id":null,
    })).unwrap();
    crate::cron::store::commit(&scene.cron, |store| store.prepare_record_run(&job.id, receipt.clone())).unwrap();
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
        params: crate::service::SpawnAgentParams { cwd: Some(scene.fixture.temp.path().display().to_string()), ..Default::default() },
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
            assert!(svc.run_watcher.finish_runtime_owned(svc, &tab, RuntimeRunKey { nonce: &nonce, kind: Some(&foreign), failed_start_epoch: None }, false, true, "foreign deadline".into()).is_none());
            let error = RuntimeFailure::SpawnFailed { reason:"authoritative fixture refusal".into() };
            let kind = AgentRunKind::Cron { job_id: job_id.clone(), run_id: run.clone() };
            assert!(svc.run_watcher.runtime_start_result(svc, &tab, &nonce, &foreign, &error).is_none());
            assert!(svc.run_watcher.runtime_start_result(svc, &tab, "foreign-nonce", &kind, &error).is_none());
            svc.run_watcher.runtime_start_result(svc, &tab, &nonce, &kind, &error).unwrap().join().unwrap();
        } else {
            let mut refused = metadata.clone();
            refused.agent.fork_session = true;
            let error = svc.prepare_chatbackend_runtime_launch(&tab, &refused).unwrap_err();
            svc.fail_prepared_runtime_cron(&tab, &refused, &error);
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
    svc.run_watcher.finish_runtime_owned(svc, &tab, RuntimeRunKey { nonce: &nonce, kind: Some(&kind), failed_start_epoch: None }, false, true, "injected hard deadline".into()).unwrap().join().unwrap();
    let receipt = scene.cron.lock().find_run_by_id(&run).unwrap().1.clone();
    assert!(svc.run_watcher.finish_runtime(svc, &tab, &nonce, true, false, "late completion".into()).is_none());
    let store = scene.cron.lock();
    assert_eq!(store.find_run_by_id(&run).unwrap().1, &receipt);
    assert_eq!(store.get(job_id).unwrap().consecutive_failures, 1);
}

struct PlannedStart {
    provider: Arc<dyn crate::agent::provider::AgentProvider>,
    fail_initialize: std::sync::atomic::AtomicBool,
    release: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    entered: Mutex<Option<mpsc::Sender<()>>>,
}

#[async_trait::async_trait]
impl crate::agent::provider::AgentProvider for PlannedStart {
    fn id(&self) -> &str { self.provider.id() }
    fn display_name(&self) -> &str { self.provider.display_name() }
    fn binary_name(&self) -> &str { self.provider.binary_name() }
    fn build_launch_command(&self, params: &crate::agent::provider::AgentLaunchParams) -> String { self.provider.build_launch_command(params) }
    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy { self.provider.readiness_strategy() }
    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery { self.provider.task_delivery() }
    fn detect_from_title(&self, text: &str) -> bool { self.provider.detect_from_title(text) }
    fn detect_from_output(&self, text: &str) -> bool { self.provider.detect_from_output(text) }
    fn session_file_path(&self, cwd: &str, session: &str) -> Option<std::path::PathBuf> { self.provider.session_file_path(cwd, session) }
    fn build_resume_command(&self, session: &str, skip: bool) -> Option<String> { self.provider.build_resume_command(session, skip) }
    fn detect_session_id(&self, cwd: &str, before: &[String], age: Option<Duration>) -> Option<String> { self.provider.detect_session_id(cwd, before, age) }
    fn agent_runtime(&self) -> Option<codeterm_plugin::agent_runtime::AgentRuntimeCapability> { self.provider.agent_runtime() }
    async fn runtime_plan(&self, context: &RuntimeContext, operation: &RuntimeOperation) -> Result<codeterm_plugin::agent_runtime::RuntimePlan, codeterm_plugin::agent_runtime::RuntimeFailure> {
        if matches!(operation, RuntimeOperation::Initialize) {
            let entered = self.entered.lock().take();
            if let Some(entered) = entered { entered.send(()).unwrap(); }
            let release = self.release.lock().take();
            if let Some(release) = release { release.await.unwrap(); }
            if self.fail_initialize.swap(false, std::sync::atomic::Ordering::AcqRel) {
                return Err(codeterm_plugin::agent_runtime::RuntimeFailure::new(codeterm_plugin::agent_runtime::RuntimeFailureCode::Deadline, "opaque initialize deadline"));
            }
        }
        self.provider.runtime_plan(context, operation).await
    }
    async fn runtime_response(&self, context: &RuntimeContext, operation: &RuntimeOperation, state: &serde_json::Value, reply: &codeterm_plugin::agent_runtime::RpcReply) -> Result<codeterm_plugin::agent_runtime::RuntimeStep, codeterm_plugin::agent_runtime::RuntimeFailure> {
        self.provider.runtime_response(context, operation, state, reply).await
    }
    async fn runtime_reduce(&self, context: &RuntimeContext, state: &serde_json::Value, frames: &[codeterm_plugin::agent_runtime::RuntimeFrame]) -> Result<codeterm_plugin::agent_runtime::RuntimeReduction, codeterm_plugin::agent_runtime::RuntimeFailure> {
        self.provider.runtime_reduce(context, state, frames).await
    }
}

#[test]
fn initialize_plan_deadline_before_any_process_finishes_its_cron_through_real_launch() {
    let scene = Scene::new();
    scene.fixture.set_agent_full_access(false);
    let (tab, _, run, mut metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
    metadata.params.skip_permissions = Some(false);
    let svc = &scene.fixture.service;
    let provider = svc.agent_registry.provider_arc(PROVIDER).unwrap();
    svc.agent_registry.register_provider(Box::new(PlannedStart {
        provider, fail_initialize: std::sync::atomic::AtomicBool::new(true), release: Mutex::new(None), entered: Mutex::new(None),
    }));
    let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = starts.clone();
    let _process = crate::service::agent_runtime::process::test_ports::register(&tab, move || {
        observed.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let (process, peer) = crate::service::agent_runtime::process::test_ports::connected();
        drop(peer);
        process
    });
    let changed = scene.ended("cron-run-finished");
    assert!(svc.launch_chatbackend_runtime_existing(&tab, &metadata, json!({})).is_err());
    let receipt = scene.cron.lock().find_run_by_id(&run).unwrap().1.clone();
    scene.await_cron_end(&changed, &receipt);
    assert_eq!(starts.load(std::sync::atomic::Ordering::Acquire), 0);
    let store = scene.cron.lock();
    let (job, receipt) = store.find_run_by_id(&run).unwrap();
    assert_eq!(receipt.status, CronRunStatus::Failed);
    assert_eq!(receipt.finished_at, Some(1_700_000_000_000));
    assert!(receipt.session_id.is_none());
    assert_eq!(receipt.report_delivery.as_deref(), Some("no report target"));
    assert_eq!(store.get(job).unwrap().consecutive_failures, 1);
}

#[test]
fn independently_pending_initialize_survives_observer_deadline_then_binds_and_completes() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let scene = Scene::new();
    scene.fixture.set_agent_full_access(false);
    let (tab, nonce, run, mut metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
    metadata.params.skip_permissions = Some(false);
    metadata.agent.task = Some("opaque first turn".into());
    let svc = &scene.fixture.service;
    let provider = svc.agent_registry.provider_arc(PROVIDER).unwrap();
    let (entered, planned) = mpsc::channel();
    let (release, ready) = tokio::sync::oneshot::channel();
    svc.agent_registry.register_provider(Box::new(PlannedStart {
        provider, fail_initialize: std::sync::atomic::AtomicBool::new(false), release: Mutex::new(Some(ready)), entered: Mutex::new(Some(entered)),
    }));
    let _process = crate::service::agent_runtime::process::test_ports::register(&tab, || {
        let (process, peer) = crate::service::agent_runtime::process::test_ports::connected();
        crate::async_runtime::spawn(async move {
            let (reader, mut writer) = tokio::io::split(peer);
            let mut lines = BufReader::new(reader).lines();
            while let Some(line) = lines.next_line().await.unwrap() {
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                let reply = json!({"id":request["id"],"result":{}});
                writer.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
            }
        });
        process
    });
    let owner = svc.clone();
    let own_tab = tab.clone();
    let launch = std::thread::spawn(move || owner.launch_chatbackend_runtime_existing(&own_tab, &metadata, json!({})));
    planned.recv_timeout(Duration::from_secs(5)).unwrap();
    let kind = svc.run_watcher.runtime_start_owner(&tab, &nonce).unwrap();
    assert!(svc.run_watcher.runtime_start_result(svc, &tab, &nonce, &kind, &RuntimeFailure::Deadline).is_none());
    assert_eq!(scene.cron.lock().find_run_by_id(&run).unwrap().1.status, CronRunStatus::Running);
    assert!(svc.run_watcher.has_runtime_run(&tab));
    release.send(()).unwrap();
    launch.join().unwrap().unwrap();
    let session = engine::for_tab(&tab).unwrap();
    assert!(session.context.lock().thread_id.is_some());
    let changed = scene.ended("cron-run-finished");
    scene.idle(&tab, true, false);
    let receipt = scene.cron.lock().find_run_by_id(&run).unwrap().1.clone();
    scene.await_cron_end(&changed, &receipt);
    let store = scene.cron.lock();
    let (_, receipt) = store.find_run_by_id(&run).unwrap();
    assert_eq!(receipt.status, CronRunStatus::Success);
    assert!(receipt.session_id.is_some());
}

fn storage_scene(clock: Arc<MockClock>) -> Scene {
    let scene = Scene::new();
    *scene.cron.lock() = CronStore::open_storage(&scene._storage, scene.fixture.temp.path().join("storage-cron-logs"), clock, crate::cron::store::CronStorageOptions::default(), PROVIDER.into()).unwrap();
    scene
}

#[test]
fn runtime_finalizer_retains_watcher_until_held_write_and_report_continuation_settle() {
    let clock = MockClock::starting_at(1_700_000_000_000);
    let scene = storage_scene(clock.clone());
    let (tab, nonce, run, metadata) = staged(&scene, clock.clone());
    let svc = &scene.fixture.service;
    let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
    svc.prepare_runtime_cron_owner(&tab, job_id, &run).unwrap();
    svc.prepare_runtime_run_watcher(&tab, &metadata).unwrap();
    let cancelled = svc.run_watcher.runtime_runs.lock().get(&tab).unwrap().cancelled.subscribe();
    scene.cron.lock().set_storage_deadline(Duration::ZERO);
    let hold = scene._storage.hold_writer().unwrap();
    hold.wait_held(Duration::from_secs(5)).unwrap();
    let (submitted, waiting) = mpsc::channel();
    scene.cron.lock().set_stage_probe(Arc::new(move |stage| { if stage == crate::cron::store::storage::CronStage::Submitted { let _ = submitted.send(()); } }));
    let finishing = svc.run_watcher.finish_runtime(svc, &tab, &nonce, false, false, "owned startup failure".into()).unwrap();
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(svc.run_watcher.has_runtime_run(&tab));
    assert!(!*cancelled.borrow());
    assert_eq!(scene.cron.lock().run_receipt(job_id, &run).unwrap().status, CronRunStatus::Running);
    assert!(svc.mux.read().get_tab(&tab).is_some());
    hold.release();
    finishing.join().unwrap();
    assert!(!svc.run_watcher.has_runtime_run(&tab));
    assert!(*cancelled.borrow());
    assert!(svc.run_watcher.cron_finalizers.lock().is_empty());
    let reopened = CronStore::open_storage(&scene._storage, scene.fixture.temp.path().join("storage-cron-logs"), clock, crate::cron::store::CronStorageOptions::default(), PROVIDER.into()).unwrap();
    let receipt = reopened.run_receipt(job_id, &run).unwrap();
    assert_eq!(receipt.status, CronRunStatus::Failed);
    assert_eq!(receipt.summary.as_deref(), Some("owned startup failure"));
    assert_eq!(receipt.report_delivery.as_deref(), Some("no report target"));
    assert_eq!(reopened.get(job_id).unwrap().consecutive_failures, 1);
    assert!(svc.run_watcher.finish_runtime(svc, &tab, &nonce, false, true, "stale deadline".into()).is_none());
}

#[test]
fn runtime_terminal_refusal_retains_original_owner_and_explicit_completion_resumes_it_once() {
    let clock = MockClock::starting_at(1_700_000_000_000);
    let scene = storage_scene(clock.clone());
    let (tab, nonce, run, metadata) = staged(&scene, clock);
    let svc = &scene.fixture.service;
    let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
    svc.prepare_runtime_cron_owner(&tab, job_id, &run).unwrap();
    svc.prepare_runtime_run_watcher(&tab, &metadata).unwrap();
    scene._storage.inject_fault(crate::storage::Fault::FailNextCommit).wait(Duration::from_secs(5)).unwrap();
    svc.run_watcher.finish_runtime(svc, &tab, &nonce, false, false, "retained failure summary".into()).unwrap().join().unwrap();
    assert!(svc.run_watcher.has_runtime_run(&tab));
    assert!(!svc.run_watcher.cron_finalizers.lock().is_empty());
    assert_eq!(scene.cron.lock().run_receipt(job_id, &run).unwrap().status, CronRunStatus::Running);
    let changed = scene.ended("cron-run-finished");
    assert!(svc.cron_complete(&scene.cron, &scene.wake, &run, Some("must not replace original"), Some(&tab), Some(&nonce), false).unwrap().is_none());
    let receipt = scene.cron.lock().run_receipt(job_id, &run).unwrap().clone();
    scene.await_cron_end(&changed, &receipt);
    let store = scene.cron.lock();
    let receipt = store.run_receipt(job_id, &run).unwrap();
    assert_eq!(receipt.status, CronRunStatus::Failed);
    assert_eq!(receipt.summary.as_deref(), Some("retained failure summary"));
    assert_eq!(store.get(job_id).unwrap().consecutive_failures, 1);
    assert!(svc.run_watcher.cron_finalizers.lock().is_empty());
}

#[test]
fn runtime_cancellation_wins_between_durable_tab_prepare_and_owner_admission() {
    let clock = MockClock::starting_at(1_700_000_000_000);
    let scene = storage_scene(clock.clone());
    let (tab, _, run, metadata) = staged(&scene, clock);
    let svc = &scene.fixture.service;
    let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
    let (prepared, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let held = Mutex::new(Some(resume));
    scene.cron.lock().set_stage_probe(Arc::new(move |stage| {
        if stage == crate::cron::store::storage::CronStage::Prepared {
            let event = held.lock().take();
            if let Some(event) = event { prepared.send(()).unwrap(); event.recv_timeout(Duration::from_secs(5)).unwrap(); }
        }
    }));
    let owner = svc.clone();
    let own_tab = tab.clone();
    let own_job = job_id.clone();
    let own_run = run.clone();
    let admitting = std::thread::spawn(move || owner.prepare_runtime_cron_owner(&own_tab, &own_job, &own_run));
    reached.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(svc.cron_cancel_running(&scene.cron, job_id, CronCancelReason::UserCancel).unwrap(), 0);
    release.send(()).unwrap();
    assert!(admitting.join().unwrap().is_err());
    assert!(scene.cron.lock().get_tab_owner(&tab).is_none());
    assert!(!scene.cron.lock().is_job_running(job_id));
    assert!(svc.prepare_runtime_run_watcher(&tab, &metadata).is_err());
    assert!(!svc.run_watcher.has_runtime_run(&tab));
    assert_eq!(scene.cron.lock().run_receipt(job_id, &run).unwrap().status, CronRunStatus::Cancelled);
}

#[test]
fn last_run_only_runtime_receipt_completes_and_releases_its_owner() {
    let scene = Scene::new();
    let (tab, nonce, run, metadata) = staged_with_history(&scene, MockClock::starting_at(1_700_000_000_000), Some(0));
    let svc = &scene.fixture.service;
    let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
    assert!(scene.cron.lock().get_history(job_id).is_empty());
    svc.prepare_runtime_cron_owner(&tab, job_id, &run).unwrap();
    svc.prepare_runtime_run_watcher(&tab, &metadata).unwrap();
    svc.run_watcher.finish_runtime(svc, &tab, &nonce, false, false, "last-run-only failure".into()).unwrap().join().unwrap();
    let store = scene.cron.lock();
    assert!(store.get_history(job_id).is_empty());
    let receipt = store.get(job_id).unwrap().last_run.as_ref().unwrap();
    assert_eq!(receipt.status, CronRunStatus::Failed);
    assert_eq!(receipt.summary.as_deref(), Some("last-run-only failure"));
    assert_eq!(receipt.report_delivery.as_deref(), Some("no report target"));
    assert!(store.get_tab_owner(&tab).is_none());
    assert!(!svc.run_watcher.has_runtime_run(&tab));
    assert!(svc.mux.read().get_tab(&tab).is_none());
    assert!(store.get(job_id).unwrap().next_run.is_some());
}
