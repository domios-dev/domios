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
            assert!(svc.prepare_chatbackend_runtime_launch(&tab, &refused).is_err());
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
    let original = svc.run_watcher.cron_binding(&tab, match &kind { AgentRunKind::Cron { job_id, .. } => job_id, _ => unreachable!() }, &run).unwrap();
    assert!(svc.run_watcher.fail_sealed_cron_launch(svc, &original, "independent pre-transport observation refusal").is_none());
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
    let (tab, _, run, mut metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
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
    let job = scene.cron.lock().find_run_by_id(&run).unwrap().0.to_owned();
    let nonce = svc.run_watcher.cron_binding(&tab, &job, &run).unwrap().nonce().unwrap().to_string();
    let kind = svc.run_watcher.runtime_start_owner(&tab, &nonce).unwrap();
    assert!(svc.run_watcher.runtime_start_result(svc, &tab, &nonce, &kind, &RuntimeFailure::Deadline).is_none());
    let original = svc.run_watcher.cron_binding(&tab, match &kind { AgentRunKind::Cron { job_id, .. } => job_id, _ => unreachable!() }, &run).unwrap();
    assert!(svc.run_watcher.fail_sealed_cron_launch(svc, &original, "independent pre-transport observation refusal").is_none());
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
    let mut launch_metadata = metadata.clone();
    launch_metadata.params.existing_tab_id = Some(tab.clone());
    launch_metadata.params.skip_permissions = Some(false);
    scene.fixture.set_agent_full_access(false);
    {
        let mut mux = svc.mux.write();
        let picker = mux.get_tab_mut(&tab).unwrap();
        picker.agent_type = None;
        picker.backing = crate::multiplexer::types::TabBacking::Pty;
    }
    let mut spec: crate::service::spawn::SpawnTabSpec = serde_json::from_value(json!({
        "workspace_id":svc.mux.read().find_workspace_for_tab(&tab).unwrap(),
        "command":{"kind":"chat_backend","plugin_id":PROVIDER,"config":{}},
    })).unwrap();
    spec.chatbackend_launch = Some(Box::new(launch_metadata));
    let ended = scene.ended("cron-run-finished");
    let owner = svc.clone();
    let admitting = std::thread::spawn(move || owner.stage_chatbackend_spawn(spec, false).map(|_| ()));

    reached.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(svc.cron_cancel_running(&scene.cron, job_id, CronCancelReason::UserCancel).unwrap(), 0);
    assert!(svc.run_watcher.cron_allocation_pending(job_id, &run));
    assert!(svc.run_watcher.cron_job_finalizing(job_id));
    assert!(svc.run_watcher.reserve_cron_slot(svc, &scene.cron, job_id, "must-wait-for-original").is_err());
    release.send(()).unwrap();
    assert!(admitting.join().unwrap().is_err());
    ended.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!svc.run_watcher.cron_allocation_pending(job_id, &run));
    assert!(!svc.run_watcher.cron_job_finalizing(job_id));
    assert!(scene.cron.lock().get_tab_owner(&tab).is_none());
    assert!(!scene.cron.lock().is_job_running(job_id));
    assert!(svc.prepare_runtime_run_watcher(&tab, &metadata).is_err());
    assert!(!svc.run_watcher.has_runtime_run(&tab));
    assert!(svc.mux.read().get_tab(&tab).is_none());
    assert!(crate::service::agent_pad::AgentPad::open(&svc.agent_pad_data_dir, &tab).is_err());
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


fn reopened(scene: &Scene) -> CronStore {
    CronStore::open_storage(&scene._storage, scene.fixture.temp.path().join("storage-cron-logs"), crate::cron::clock::system_clock(), crate::cron::store::CronStorageOptions::default(), PROVIDER.into()).unwrap()
}

#[test]
fn cancellation_commit_stale_loser_and_cleanup_scan_keep_one_original_owner() {
    let clock = MockClock::starting_at(1_700_000_000_000);
    let scene = storage_scene(clock.clone());
    let (tab, nonce, run, metadata) = staged(&scene, clock);
    let svc = &scene.fixture.service;
    let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
    svc.prepare_runtime_cron_owner(&tab, job_id, &run).unwrap();
    svc.prepare_runtime_run_watcher(&tab, &metadata).unwrap();
    let (sent, committed) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let held = Mutex::new(Some(resume));
    scene.cron.lock().set_stage_probe(Arc::new(move |stage| {
        if stage == crate::cron::store::storage::CronStage::Committed {
            if let Some(wait) = held.lock().take() {
                sent.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(5)).unwrap();
            }
        }
    }));
    let cancelling = svc.clone();
    let cron = scene.cron.clone();
    let job = job_id.clone();
    let cancelled = std::thread::spawn(move || cancelling.cron_cancel_running(&cron, &job, CronCancelReason::UserCancel));
    committed.recv_timeout(Duration::from_secs(5)).unwrap();
    let original = reopened(&scene).run_receipt(job_id, &run).unwrap().clone();
    assert_eq!(original.status, CronRunStatus::Cancelled);
    assert!(svc.run_watcher.cron_job_finalizing(job_id));
    let before = scene.cron.lock().get(job_id).unwrap().clone();
    assert!(svc.run_watcher.reserve_cron_slot(svc, &scene.cron, job_id, "concurrent-occurrence").is_err());
    assert_eq!(scene.cron.lock().get(job_id), Some(&before));
    svc.run_watcher.finish_runtime(svc, &tab, &nonce, false, true, "stale timeout".into()).unwrap().join().unwrap();
    assert!(svc.mux.read().get_tab(&tab).is_none());
    release.send(()).unwrap();
    assert_eq!(cancelled.join().unwrap().unwrap(), 1, "receipt count uses original cancellation snapshot");
    assert_eq!(reopened(&scene).run_receipt(job_id, &run), Some(&original));
    assert!(svc.run_watcher.cron_cleanup_owners(job_id).is_empty());
    assert!(!svc.run_watcher.cron_job_finalizing(job_id));
    svc.run_watcher.reserve_cron_slot(svc, &scene.cron, job_id, "next-occurrence").unwrap();
}

#[test]
fn runtime_pre_reservation_refusal_retires_provisional_pad_and_tab() {
    let scene = Scene::new();
    let (tab, _, run, metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
    let svc = &scene.fixture.service;
    let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
    crate::cron::store::commit(&scene.cron, |store| store.prepare_cancel_running(job_id, store.now_ms(), CronCancelReason::UserCancel)).unwrap();
    let original = scene.cron.lock().run_receipt(job_id, &run).unwrap().clone();
    let changed = scene.ended("cron-run-finished");
    assert!(svc.prepare_chatbackend_runtime_launch(&tab, &metadata).is_err());
    changed.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(svc.mux.read().get_tab(&tab).is_none());
    assert!(crate::service::agent_pad::AgentPad::open(&svc.agent_pad_data_dir, &tab).is_err());
    assert_eq!(scene.cron.lock().run_receipt(job_id, &run), Some(&original));
}


#[test]
fn deferred_runtime_pre_transport_failure_terminalizes_original_even_after_tab_disappears() {
    for disappeared in [false, true] {
        let scene = Scene::new();
        let (tab, _, run, mut metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
        let svc = &scene.fixture.service;
        metadata.params.existing_tab_id = Some(tab.clone());
        metadata.params.skip_permissions = Some(false);
        scene.fixture.set_agent_full_access(false);
        {
            let mut mux = svc.mux.write();
            let picker = mux.get_tab_mut(&tab).unwrap();
            picker.agent_type = None;
            picker.backing = crate::multiplexer::types::TabBacking::Pty;
        }
        let mut spec: crate::service::spawn::SpawnTabSpec = serde_json::from_value(json!({
            "workspace_id":svc.mux.read().find_workspace_for_tab(&tab).unwrap(),
            "command":{"kind":"chat_backend","plugin_id":PROVIDER,"config":{}},
        })).unwrap();
        spec.chatbackend_launch = Some(Box::new(metadata.clone()));
        let (handle, launch) = svc.stage_chatbackend_spawn(spec, false).unwrap();
        assert_eq!(handle.tab_id, tab);
        let changed = scene.ended("cron-run-finished");
        if disappeared {
            svc.mux.write().remove_tab(&tab).unwrap();
        } else {
            let mut mux = svc.mux.write();
            let replacement = mux.get_tab_mut(&tab).unwrap();
            replacement.session_binding_nonce = Some("replacement-nonce".into());
            replacement.pty_id = Some("replacement-pty".into());
            replacement.restorable_launch.args = Some(vec!["--refused-runtime-option".into()]);
        }
        let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = starts.clone();
        let _process = crate::service::agent_runtime::process::test_ports::register(&tab, move || {
            observed.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            let (process, peer) = crate::service::agent_runtime::process::test_ports::connected();
            drop(peer);
            process
        });
        assert!(launch.run().is_err());
        assert_eq!(starts.load(std::sync::atomic::Ordering::Acquire), 0);
        let receipt = scene.cron.lock().find_run_by_id(&run).unwrap().1.clone();
        scene.await_cron_end(&changed, &receipt);
        let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
        let receipt = scene.cron.lock().run_receipt(job_id, &run).unwrap().clone();
        assert_eq!(receipt.status, CronRunStatus::Failed);
        assert_eq!(receipt.finished_at, Some(1_700_000_000_000));
        assert_eq!(receipt.report_delivery.as_deref(), Some("no report target"));
        assert!(!svc.run_watcher.has_runtime_run(&tab));
        if !disappeared {
            assert_eq!(svc.mux.read().get_tab(&tab).unwrap().pty_id.as_deref(), Some("replacement-pty"));
            assert_eq!(svc.mux.read().get_tab(&tab).unwrap().session_binding_nonce.as_deref(), Some("replacement-nonce"));
        }
    }
}


#[test]
fn restart_known_report_with_replacement_requires_adjudication_and_preserves_replacement() {
    let scene = storage_scene(MockClock::starting_at(1_700_000_000_000));
    let (tab, nonce, run, metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
    let svc = &scene.fixture.service;
    let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
    svc.prepare_runtime_cron_owner(&tab, job_id, &run).unwrap();
    svc.prepare_runtime_run_watcher(&tab, &metadata).unwrap();
    let replaced = std::sync::atomic::AtomicBool::new(false);
    let writer = scene._storage.clone();
    let owner = svc.clone();
    let own_tab = tab.clone();
    scene.cron.lock().set_stage_probe(Arc::new(move |stage| {
        if stage == crate::cron::store::storage::CronStage::Committed && !replaced.swap(true, std::sync::atomic::Ordering::AcqRel) {
            {
                let mut mux = owner.mux.write();
                let replacement = mux.get_tab_mut(&own_tab).unwrap();
                replacement.session_binding_nonce = Some("replacement-after-commit".into());
                replacement.pty_id = Some("replacement-pty".into());
            }
            writer.inject_fault(crate::storage::Fault::FailNextCommit).wait(Duration::from_secs(5)).unwrap();
        }
    }));
    svc.run_watcher.finish_runtime(svc, &tab, &nonce, false, false, "original terminal".into()).unwrap().join().unwrap();
    let original = scene.cron.lock().run_receipt(job_id, &run).unwrap().clone();
    assert_eq!(original.status, CronRunStatus::Failed);
    assert!(original.report_delivery.is_none());
    let mut restarted = svc.clone();
    restarted.run_watcher = Arc::new(RunWatcherRegistry::new());
    restarted.run_watcher.register(restarted.clone(), "replacement-pty".into(), tab.clone(), AgentRunKind::MemWorker { worker_id:"replacement-worker".into() }, 1200, None);
    restarted.run_watcher.ensure_cron_recovery(&restarted, &scene.cron).unwrap();
    assert!(restarted.run_watcher.unknown_cron_outcome(&run));
    assert_eq!(scene.cron.lock().run_receipt(job_id, &run), Some(&original));
    assert!(restarted.run_watcher.runs.lock().contains_key("replacement-pty"));
    assert_eq!(restarted.cron_cancel_running(&scene.cron, job_id, CronCancelReason::UserCancel).unwrap(), 0);
    assert_eq!(scene.cron.lock().run_receipt(job_id, &run), Some(&original));
    assert_eq!(svc.mux.read().get_tab(&tab).unwrap().session_binding_nonce.as_deref(), Some("replacement-after-commit"));
    assert_eq!(svc.mux.read().get_tab(&tab).unwrap().pty_id.as_deref(), Some("replacement-pty"));
    assert!(restarted.run_watcher.runs.lock().contains_key("replacement-pty"));
    assert!(!restarted.run_watcher.cron_job_finalizing(job_id));
    restarted.run_watcher.mark_completed_externally("replacement-pty");
}


#[test]
fn staged_cron_expected_generation_normal_and_fork_preserve_valid_original_launch() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    for fork in [false, true] {
        let scene = Scene::new();
        scene.fixture.set_agent_full_access(false);
        let (tab, _, run, mut metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
        let svc = &scene.fixture.service;
        metadata.agent.task = Some("opaque valid initial input".into());
        metadata.params.existing_tab_id = Some(tab.clone());
        metadata.params.skip_permissions = Some(false);
        if fork { metadata.runtime_fork = Some(crate::service::spawn::RuntimeForkLaunch { source_thread_id:"source-thread".into(), turn_id:"source-turn".into() }); }
        {
            let mut mux = svc.mux.write();
            let picker = mux.get_tab_mut(&tab).unwrap();
            picker.agent_type = None;
            picker.backing = crate::multiplexer::types::TabBacking::Pty;
        }
        let mut spec: crate::service::spawn::SpawnTabSpec = serde_json::from_value(json!({
            "workspace_id":svc.mux.read().find_workspace_for_tab(&tab).unwrap(),
            "command":{"kind":"chat_backend","plugin_id":PROVIDER,"config":{}},
        })).unwrap();
        spec.chatbackend_launch = Some(Box::new(metadata.clone()));
        let (handle, launch) = svc.stage_chatbackend_spawn(spec, false).unwrap();
        assert_eq!(handle.tab_id, tab);
        let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
        let original = svc.run_watcher.cron_binding(&tab, job_id, &run).unwrap();
        let nonce = original.nonce().unwrap().to_string();
        let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = starts.clone();
        let _process = crate::service::agent_runtime::process::test_ports::register(&tab, move || {
            observed.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            let (process, peer) = crate::service::agent_runtime::process::test_ports::connected();
            crate::async_runtime::spawn(async move {
                let (reader, mut writer) = tokio::io::split(peer);
                let mut lines = BufReader::new(reader).lines();
                while let Some(line) = lines.next_line().await.unwrap() {
                    let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                    writer.write_all(format!("{}\n", json!({"id":request["id"],"result":{}})).as_bytes()).await.unwrap();
                }
            });
            process
        });
        launch.run().unwrap();
        assert_eq!(starts.load(std::sync::atomic::Ordering::Acquire), 1);
        let session = engine::for_tab(&tab).unwrap();
        assert_eq!(session.context.lock().binding_nonce, nonce);
        assert!(session.thread_loaded.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(svc.mux.read().get_tab(&tab).unwrap().session_binding_nonce.as_deref(), Some(nonce.as_str()));
        let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
        assert_eq!(scene.cron.lock().run_receipt(job_id, &run).unwrap().status, CronRunStatus::Running);
        assert!(svc.run_watcher.has_runtime_run(&tab));
        assert_eq!(svc.cron_cancel_running(&scene.cron, job_id, CronCancelReason::UserCancel).unwrap(), 1);
    }
}


#[test]
fn staged_cron_replacement_refusal_preserves_original_and_replacement_with_acquired_controls() {
    for fork in [false, true] {
        for acquired in [false, true] {
            let scene = Scene::new();
            scene.fixture.set_agent_full_access(false);
            let (tab, _, run, mut metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
            let svc = &scene.fixture.service;
            metadata.params.existing_tab_id = Some(tab.clone());
            metadata.params.skip_permissions = Some(false);
            if fork { metadata.runtime_fork = Some(crate::service::spawn::RuntimeForkLaunch { source_thread_id:"source-thread".into(), turn_id:"source-turn".into() }); }
            {
                let mut mux = svc.mux.write(); let picker = mux.get_tab_mut(&tab).unwrap();
                picker.agent_type = None; picker.backing = crate::multiplexer::types::TabBacking::Pty;
            }
            let mut spec: crate::service::spawn::SpawnTabSpec = serde_json::from_value(json!({
                "workspace_id":svc.mux.read().find_workspace_for_tab(&tab).unwrap(),
                "command":{"kind":"chat_backend","plugin_id":PROVIDER,"config":{}},
            })).unwrap();
            spec.chatbackend_launch = Some(Box::new(metadata.clone()));
            let (_, launch) = svc.stage_chatbackend_spawn(spec, false).unwrap();
            let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
            let original = svc.mux.read().get_tab(&tab).unwrap().clone();
            let owned = svc.run_watcher.cron_binding(&tab, job_id, &run).unwrap();
            if acquired {
                crate::service::agent_runtime::authority::record_spawn_for_test(svc, &tab, PROVIDER, owned.nonce().unwrap());
                let mut loaded = original.clone(); loaded.agent_session_id = Some("earlier-original-session".into());
                let earlier = engine::test_support::loaded_runtime(&loaded, svc.agent_registry.provider_arc(PROVIDER));
                assert!(crate::async_runtime::block_on(engine::ensure_with_invocation(svc, &tab, None)).is_err());
                engine::test_support::stop_test(&tab);
                drop(earlier);
            }
            let replacement = Arc::new(Mutex::new(None));
            let live = Arc::new(Mutex::new(None));
            let captured = replacement.clone(); let retained = live.clone(); let expected = original.clone(); let retiring = owned.clone();
            assert!(svc.set_ext(engine::ExpectedLaunchCommit { stage:"account_provision", callback:Mutex::new(Some(Box::new(move |svc| {
                let (_, committed) = svc.reserve_agent_launch_binding_nonce_if_current(&expected, None, None).unwrap();
                let nonce = committed.session_binding_nonce.clone().unwrap();
                {
                    let mut mux = svc.mux.write(); let current = mux.get_tab_mut(&committed.id).unwrap();
                    current.agent_session_id = Some("replacement-session".into());
                    current.reported_session_binding_nonce = Some(nonce.clone());
                    current.agent_runtime_config = Some(json!({"opaque":"replacement-config"}));
                }
                crate::service::agent_runtime::authority::record_spawn_for_test(svc, &committed.id, PROVIDER, &nonce);
                svc.ensure_agent_pad(&committed.id).unwrap();
                let current = svc.mux.read().get_tab(&committed.id).unwrap().clone();
                *retained.lock() = Some(engine::test_support::loaded_runtime(&current, svc.agent_registry.provider_arc(PROVIDER)));
                svc.run_watcher.retire_cron(&retiring);
                svc.run_watcher.register_runtime(svc, current.id.clone(), AgentRunKind::MemWorker { worker_id:"replacement-worker".into() }, 1200).unwrap();
                *captured.lock() = Some(serde_json::to_value(current).unwrap());
            }))) }).is_ok());
            let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0)); let observed = starts.clone();
            let _process = crate::service::agent_runtime::process::test_ports::register(&tab, move || {
                observed.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                let (process, peer) = crate::service::agent_runtime::process::test_ports::connected(); drop(peer); process
            });
            let ended = scene.ended("cron-run-finished");
            assert!(launch.run().is_err());
            assert_eq!(starts.load(std::sync::atomic::Ordering::Acquire), 0);
            assert_eq!(scene.cron.lock().run_receipt(job_id, &run).unwrap().status, CronRunStatus::Running);
            assert!(ended.try_recv().is_err());
            assert_eq!(serde_json::to_value(svc.mux.read().get_tab(&tab).unwrap()).unwrap(), replacement.lock().clone().expect("actual account provision boundary reached"));
            assert!(svc.run_watcher.runtime_runs.lock().get(&tab).is_some_and(|watcher| matches!(&watcher.kind, AgentRunKind::MemWorker { worker_id } if worker_id == "replacement-worker")));
            assert_eq!(engine::for_tab(&tab).unwrap().context.lock().thread_id.as_deref(), Some("replacement-session"));
            {
                assert_eq!(svc.cron_cancel_running(&scene.cron, job_id, CronCancelReason::UserCancel).unwrap(), 1);
                assert_eq!(serde_json::to_value(svc.mux.read().get_tab(&tab).unwrap()).unwrap(), replacement.lock().clone().unwrap());
                assert!(svc.run_watcher.runtime_runs.lock().get(&tab).is_some_and(|watcher| matches!(&watcher.kind, AgentRunKind::MemWorker { worker_id } if worker_id == "replacement-worker")));
            }
            drop(live.lock().take());
        }
    }
}

#[test]
fn staged_cron_retained_provisioning_stays_pending_without_process_or_payload_retry() {
    use crate::service::provider_accounts::{provisioning, test_support};
    let scene = Scene::new(); scene.fixture.set_agent_full_access(false);
    let (tab, _, run, mut metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
    let svc = &scene.fixture.service;
    metadata.params.existing_tab_id = Some(tab.clone()); metadata.params.skip_permissions = Some(false);
    {
        let mut mux = svc.mux.write(); let picker = mux.get_tab_mut(&tab).unwrap();
        picker.agent_type = None; picker.backing = crate::multiplexer::types::TabBacking::Pty;
    }
    let mut spec: crate::service::spawn::SpawnTabSpec = serde_json::from_value(json!({
        "workspace_id":svc.mux.read().find_workspace_for_tab(&tab).unwrap(),
        "command":{"kind":"chat_backend","plugin_id":PROVIDER,"config":{}},
    })).unwrap(); spec.chatbackend_launch = Some(Box::new(metadata.clone()));
    let (_, launch) = svc.stage_chatbackend_spawn(spec, false).unwrap();
    let (entered, reached) = mpsc::channel(); let (release, wait) = mpsc::channel();
    let mut work = test_support::work_receipt(svc);
    let held = provisioning::start(svc, move || {
        entered.send(()).unwrap(); wait.recv_timeout(Duration::from_secs(5)).unwrap(); Ok(None)
    }).unwrap();
    reached.recv_timeout(Duration::from_secs(5)).unwrap();
    let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0)); let observed = starts.clone();
    let _process = crate::service::agent_runtime::process::test_ports::register(&tab, move || {
        observed.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let (process, peer) = crate::service::agent_runtime::process::test_ports::connected(); drop(peer); process
    });
    assert!(launch.run().is_err());
    let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
    assert_eq!(scene.cron.lock().run_receipt(job_id, &run).unwrap().status, CronRunStatus::Running);
    assert!(svc.run_watcher.has_runtime_run(&tab)); assert_eq!(starts.load(std::sync::atomic::Ordering::Acquire), 0);
    release.send(()).unwrap(); held.wait_blocking(Duration::from_secs(5)).unwrap();
    crate::async_runtime::block_on(test_support::settled(&mut work)).unwrap();
    assert_eq!(scene.cron.lock().run_receipt(job_id, &run).unwrap().status, CronRunStatus::Running);
    assert_eq!(svc.cron_cancel_running(&scene.cron, job_id, CronCancelReason::UserCancel).unwrap(), 1);
}

#[test]
fn staged_cron_completed_account_refusal_requires_original_prepared_input_and_native_history() {
    for fork in [false, true] {
        for input in ["empty", "created", "existing-missing-ledger", "admitted", "acquired", "removed"] {
            let scene = Scene::new(); scene.fixture.set_agent_full_access(false);
            let (tab, _, run, mut metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
            let svc = &scene.fixture.service;
            metadata.params.existing_tab_id = Some(tab.clone()); metadata.params.skip_permissions = Some(false);
            if matches!(input, "created" | "acquired" | "removed") { metadata.agent.task = Some("opaque initial input".into()); }
            if fork { metadata.runtime_fork = Some(crate::service::spawn::RuntimeForkLaunch { source_thread_id:"source-thread".into(), turn_id:"source-turn".into() }); }
            {
                let mut mux = svc.mux.write(); let picker = mux.get_tab_mut(&tab).unwrap();
                picker.agent_type = None; picker.backing = crate::multiplexer::types::TabBacking::Pty;
            }
            let existing_send = if matches!(input, "existing-missing-ledger" | "admitted") {
                let send = uuid::Uuid::new_v4().to_string();
                svc.create_pending_delivery_with_send_id(&tab, crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput, "opaque existing input", Some(&send)).unwrap();
                Some(send)
            } else { None };
            let mut spec: crate::service::spawn::SpawnTabSpec = serde_json::from_value(json!({
                "workspace_id":svc.mux.read().find_workspace_for_tab(&tab).unwrap(),
                "command":{"kind":"chat_backend","plugin_id":PROVIDER,"config":{}},
            })).unwrap(); spec.chatbackend_launch = Some(Box::new(metadata.clone()));
            let (_, launch) = svc.stage_chatbackend_spawn(spec, false).unwrap();
            let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
            let owner = svc.run_watcher.cron_binding(&tab, job_id, &run).unwrap();
            if input == "admitted" { svc.chat_send_ledger.start_runtime_claim(&tab, "opaque existing input", existing_send.as_deref().unwrap()).unwrap(); }
            let prepared_send = svc.mux.read().get_tab(&tab).unwrap().pending_delivery.as_ref().and_then(|pending| pending.send_id().map(str::to_string));
            if input == "acquired" {
                crate::service::agent_runtime::authority::record_spawn_for_test(svc, &tab, PROVIDER, owner.nonce().unwrap());
                let mut original = svc.mux.read().get_tab(&tab).unwrap().clone(); original.agent_session_id = Some("earlier-original-session".into());
                let earlier = engine::test_support::loaded_runtime(&original, svc.agent_registry.provider_arc(PROVIDER));
                assert!(crate::async_runtime::block_on(engine::ensure_with_invocation(svc, &tab, None)).is_err());
                engine::test_support::stop_test(&tab); drop(earlier);
            }
            // The actual account path refuses a nonexistent isolated account without rotating the tab.
            svc.mux.write().get_tab_mut(&tab).unwrap().restorable_launch.account_id = Some("missing-fixture-account".into());
            let original_pending = serde_json::to_value(&svc.mux.read().get_tab(&tab).unwrap().pending_delivery).unwrap();
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0)); let observed = calls.clone();
            let _process = crate::service::agent_runtime::process::test_ports::register(&tab, move || {
                observed.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                let (process, peer) = crate::service::agent_runtime::process::test_ports::connected(); drop(peer); process
            });
            let reached = Arc::new(std::sync::atomic::AtomicBool::new(false)); let seen = reached.clone(); let target = tab.clone();
            assert!(svc.set_ext(engine::ExpectedLaunchCommit { stage:"account_provision", callback:Mutex::new(Some(Box::new(move |svc| {
                seen.store(true, std::sync::atomic::Ordering::Release);
                if input == "removed" { svc.mux.write().remove_tab(&target).unwrap(); }
            }))) }).is_ok());
            let ended = scene.ended("cron-run-finished");
            assert!(launch.run().is_err());
            assert!(reached.load(std::sync::atomic::Ordering::Acquire));
            assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 0);
            if input == "created" { assert!(svc.chat_send_ledger.status(prepared_send.as_deref().unwrap()).is_none(), "rejected native start never reached bootstrap admission"); }
            let positive = matches!(input, "empty" | "created" | "admitted");
            if positive {
                ended.recv_timeout(Duration::from_secs(5)).unwrap();
                let receipt = scene.cron.lock().run_receipt(job_id, &run).unwrap().clone();
                assert_eq!(receipt.status, CronRunStatus::Failed);
                assert_eq!(receipt.finished_at, Some(1_700_000_000_000));
                assert_eq!(scene.cron.lock().get(job_id).unwrap().consecutive_failures, 1);
                assert_eq!(receipt.report_delivery.as_deref(), Some("no report target"));
                assert!(ended.try_recv().is_err());
                assert!(svc.mux.read().get_tab(&tab).is_none());
            } else {
                assert_eq!(scene.cron.lock().run_receipt(job_id, &run).unwrap().status, CronRunStatus::Running);
                assert!(ended.try_recv().is_err());
                if input != "removed" {
                    let current = svc.mux.read().get_tab(&tab).unwrap().clone();
                    assert_eq!(current.session_binding_nonce.as_deref(), owner.nonce());
                    assert_eq!(serde_json::to_value(&current.pending_delivery).unwrap(), original_pending);
                }
                svc.cron_cancel_running(&scene.cron, job_id, CronCancelReason::UserCancel).unwrap();
            }
        }
    }
}

#[test]
fn runtime_cron_completion_materializes_bounded_nonrereadable_cache_evidence() {
    for oversized in [false, true] {
        let scene = Scene::new(); let svc = &scene.fixture.service;
        let (tab, _, run, metadata) = staged(&scene, MockClock::starting_at(1_700_000_000_000));
        let RuntimeOwnerLinkage::CronRun { job_id, .. } = metadata.owner_linkage.as_ref().unwrap() else { unreachable!() };
        svc.prepare_runtime_cron_owner(&tab, job_id, &run).unwrap();
        svc.prepare_runtime_run_watcher(&tab, &metadata).unwrap();
        let owner = svc.run_watcher.cron_binding(&tab, job_id, &run).unwrap();
        let mut original = svc.mux.read().get_tab(&tab).unwrap().clone(); original.agent_session_id = Some("output-session".into());
        let live = engine::test_support::loaded_runtime(&original, svc.agent_registry.provider_arc(PROVIDER));
        let session = engine::for_tab(&tab).unwrap();
        let ordinary: ChatMsg = serde_json::from_value(json!({"id":"ordinary", "type":"assistant", "content":"ordinary output", "seq":0})).unwrap();
        session.cache.lock().reconcile(ordinary);
        if oversized {
            for n in 0..192 {
                let row: ChatMsg = serde_json::from_value(json!({"id":format!("row-{n}"), "type":"assistant", "content":"x".repeat(32 * 1024), "seq":n+1})).unwrap();
                session.cache.lock().reconcile(row);
            }
            let mut deep = json!("opaque"); for _ in 0..24 { deep = json!([deep]); }
            let row: ChatMsg = serde_json::from_value(json!({"id":"recent-large", "type":"assistant", "content":"🙂".repeat(1024 * 1024), "tool_input":deep, "seq":194})).unwrap();
            session.cache.lock().reconcile(row);
        }
        let before = session.cache.lock().retained();
        let (evidence, complete) = session.cache.lock().terminal_evidence();
        assert!(evidence.len() <= 8 * 1024);
        if oversized {
            assert!(!complete); assert!(evidence.starts_with("PARTIAL"));
            let cache = session.cache.lock();
            assert_eq!(cache.retained(), before);
            assert_eq!(cache.get("recent-large").unwrap().content.len(), 4 * 1024 * 1024);
            assert!(cache.get("recent-large").unwrap().tool_input.is_some());
        } else {
            assert!(complete);
            assert_eq!(evidence, serde_json::to_string(&session.cache.lock().snapshot()).unwrap());
        }
        svc.run_watcher.finish_runtime(svc, &tab, owner.nonce().unwrap(), true, false, "completed".into()).unwrap().join().unwrap();
        assert_eq!(session.cache.lock().retained(), before);
        let log = scene.cron.lock().log_file(job_id, &run).unwrap();
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains(&evidence));
        assert!(text.contains("full transcript bytecount/digest unavailable"));
        let record: serde_json::Value = serde_json::from_slice(&std::fs::read(log.with_extension("terminal-settled.json")).unwrap()).unwrap();
        if oversized { assert!(record["input_bytes"].is_null()); } else { assert_eq!(record["input_bytes"], evidence.len()); }
        assert_eq!(record["projection_digest"].as_str().unwrap().len(), 64);
        assert_eq!(scene.cron.lock().run_receipt(job_id, &run).unwrap().status, CronRunStatus::Success);
        drop(live);
    }
}

#[test]
fn runtime_completion_projection_bounds_scalar_escaping_depth_and_utf8_before_serialization() {
    for (content, nesting) in [("🙂".repeat(1024 * 1024), 0), ("\u{0001}".repeat(1000), 0), ("ordinary".into(), 24)] {
        let mut cache = crate::service::agent_runtime::model::ItemCache::default();
        let mut value = json!("opaque"); for _ in 0..nesting { value = json!([value]); }
        let row: ChatMsg = serde_json::from_value(json!({"id":"recent", "type":"assistant", "content":content, "tool_input":value})).unwrap();
        cache.reconcile(row);
        let retained = cache.retained();
        let (evidence, complete) = cache.terminal_evidence();
        assert!(!complete); assert!(evidence.starts_with("PARTIAL")); assert!(evidence.len() <= 8 * 1024);
        assert_eq!(cache.retained(), retained);
        assert_eq!(cache.get("recent").unwrap().content, content);
    }
}
