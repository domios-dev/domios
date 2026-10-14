use super::*;
use crate::cron::store::{CronCreateParams, CronRunInfo, CronRunStatus, CronStore};
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use codeterm_plugin::agent_runtime::{RpcCall, RuntimeOperation};
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use parking_lot::Mutex;
use serde_json::json;
use std::sync::{mpsc, Arc};
use tauri::{Listener, Manager};

const PROVIDER: &str = "run-end-fixture";
const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) { return {action:{kind:'request',call:{method:op.kind,params:op}},state:null}; },
  runtimeResponse(ctx, op) {
    let result = {kind:'done'}, events = [];
    if (op.kind === 'initialize') result = {kind:'initialized'};
    if (op.kind === 'resumeThread' || op.kind === 'openThread')
      result = {kind:'threadLoaded',threadId:op.threadId || ctx.bindingNonce,settings:op.settings};
    if (op.kind === 'readHistory') {
      result = {kind:'historyPage',threadId:op.threadId,items:[],openItems:[]};
      events = [{kind:'turnState',threadId:op.threadId,turnId:'old-turn',state:'idle'}];
    }
    if (op.kind === 'submitTurn') result = {kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:'own-turn'};
    if (op.kind === 'control') events = op.value;
    return {kind:'complete',result,events};
  },
  runtimeReduce() { return {state:null,events:[]}; }
};
"#;

struct Scene {
    fixture: ServiceFixture,
    cron: crate::CronState,
    wake: crate::CronWake,
    _storage: crate::storage::Storage,
}
impl Drop for Scene {
    fn drop(&mut self) {
        for tab in self.fixture.service.mux.read().snapshot().tabs.keys() {
            engine::stop(tab);
        }
        crate::service::session_persist::stop_worker(self.fixture._app.handle()).unwrap();
        for worker in self.fixture.service.workers.list() {
            if let Some(path) = worker.log_path {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}
impl Scene {
    fn new() -> Self {
        let (mux, _) = MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let storage = fixture.install_seeded_storage_desk();
        let cron = Arc::new(Mutex::new(CronStore::new_with_path(
            fixture.temp.path().join("cron.json"),
        )));
        let wake = Arc::new(std::sync::Condvar::new());
        fixture._app.manage(cron.clone());
        fixture._app.manage(wake.clone());
        let methods = [
            "initialize",
            "resumeThread",
            "openThread",
            "readHistory",
            "submitTurn",
            "control",
        ]
        .into_iter()
        .map(|method| (method.to_string(), json!([method])))
        .collect::<serde_json::Map<String, serde_json::Value>>();
        let manifest = PluginManifest::from_json_str(&json!({
            "id":PROVIDER,"version":"1.0.0","displayName":"Fixture",
            "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
                "agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":"node","args":[format!("{}/../tests/fixtures/agent-runtime/fake-peer.mjs",crate::test_support::fixture_manifest_dir().display())]},"requestMethods":methods,"notificationMethods":[]}},
            "permissions":{"subprocess":{"allow":["node"]}}
        }).to_string()).unwrap();
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(JsProvider::new(
                manifest,
                Arc::new(PluginVm::from_source(PROVIDER, SOURCE.into()).unwrap()),
            )));
        Self {
            fixture,
            cron,
            wake,
            _storage: storage,
        }
    }
    fn cron(&self) -> (String, CronRunInfo) {
        let workspace = self.fixture.service.mux.read().list_workspaces()[0]
            .id
            .clone();
        let job = self
            .cron
            .lock()
            .create(CronCreateParams {
                name: "Runtime".into(),
                workspace_id: workspace,
                team_id: None,
                detach_from_team: false,
                schedule: "in 1h".into(),
                task: "opaque supplied job".into(),
                description: None,
                agent: Some(PROVIDER.into()),
                model: None,
                reasoning_effort: None,
                cwd: Some(self.fixture.temp.path().display().to_string()),
                role: None,
                skill: None,
                creator_tab_id: None,
                report_target_tab_id: None,
                enabled: Some(false),
                max_history: None,
                oneshot: Some(true),
                jitter_max_ms: Some(0),
                tz: None,
                timeout_seconds: Some(60),
            })
            .unwrap();
        let run = self
            .fixture
            .service
            .cron_trigger(&self.cron, &job.id)
            .unwrap();
        assert!(run.tab_id.is_some());
        (job.id, run)
    }
    fn worker(&self, intent: &str) -> (String, String) {
        let worker = format!("wk-{}", uuid::Uuid::new_v4());
        self.fixture
            .service
            .workers
            .register(crate::mem::workers::make_info(
                worker.clone(),
                String::new(),
                intent.into(),
                "fixture".into(),
                "red".into(),
            ));
        let spec = crate::mem_host::worker_spawn_spec::build_mem_worker_spawn_spec(
            &self.fixture.service.chatbackend_registry,
            self.fixture.temp.path().to_path_buf(),
            crate::mem_host::worker_spawn_spec::MemWorkerSpawnInputs {
                agent_provider: PROVIDER.into(),
                model: String::new(),
                task: "opaque memory task".into(),
                worker_id: worker.clone(),
                spawned_by: "fixture".into(),
                intent: intent.into(),
                instance_cli_path: None,
                inherited_path: std::ffi::OsString::new(),
            },
        )
        .unwrap();
        let handle = self.fixture.service.spawn_tab(spec).unwrap();
        assert!(
            self.fixture
                .service
                .mux
                .read()
                .get_tab(&handle.tab_id)
                .unwrap()
                .hidden
        );
        (worker, handle.tab_id)
    }
    fn ended(&self, event: &str) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        self.fixture._app.listen(event, move |_| {
            let _ = tx.send(());
        });
        rx
    }
    fn events(&self, tab: &str, events: serde_json::Value) {
        let session = engine::for_tab(tab).unwrap();
        let thread = session.context.lock().thread_id.clone();
        crate::service::agent_runtime::transport::control(
            &self.fixture.service,
            tab,
            RuntimeOperation::Control {
                thread_id: thread,
                action: "fixture-events".into(),
                value: events,
            },
        )
        .unwrap();
    }
    fn idle(&self, tab: &str, answer: bool, idle_first: bool) {
        let session = engine::for_tab(tab).unwrap();
        let thread = session.context.lock().thread_id.clone().unwrap();
        let idle = json!({"kind":"turnState","threadId":thread,"turnId":"own-turn","state":"idle"});
        let message = json!({"kind":"chatItem","threadId":thread,"item":{"id":"own-answer","type":"assistant","content":"opaque final answer"}});
        let events = if !answer {
            json!([idle])
        } else if idle_first {
            json!([idle, message])
        } else {
            json!([message, idle])
        };
        self.events(tab, events);
    }
    fn assert_cron_end(&self, job: &str, run: &CronRunInfo, status: CronRunStatus) {
        let store = self.cron.lock();
        let (_, history) = store.find_run_by_id(&run.run_id).unwrap();
        assert_eq!(history.status, status);
        assert!(history.finished_at.is_some());
        assert!(history.session_id.is_some());
        assert!(history.summary.is_some());
        assert_eq!(history.report_delivery.as_deref(), Some("no report target"));
        assert!(store.get(job).is_none());
        assert!(store.get_tab_owner_by_run(&run.run_id).is_none());
        assert!(!self
            .fixture
            .service
            .run_watcher
            .has_runtime_run(run.tab_id.as_ref().unwrap()));
        assert!(self
            .fixture
            .service
            .mux
            .read()
            .get_tab(run.tab_id.as_ref().unwrap())
            .is_none());
    }
}

#[test]
fn runtime_oneshot_explicit_complete_retains_final_answer_then_closes_in_both_event_orders() {
    for idle_first in [true, false] {
        let scene = Scene::new();
        let (job, run) = scene.cron();
        let tab = run.tab_id.as_ref().unwrap();
        let session = engine::for_tab(tab).unwrap();
        let nonce = session.context.lock().binding_nonce.clone();
        let thread = session.context.lock().thread_id.clone().unwrap();
        let result = scene
            .fixture
            .service
            .cron_complete(
                &scene.cron,
                &scene.wake,
                &run.run_id,
                Some("opaque report"),
                Some(tab),
                Some(&nonce),
                false,
            )
            .unwrap();
        assert!(result.is_none());
        scene.events(
            tab,
            json!([{"kind":"turnState","threadId":thread,"turnId":"own-turn","state":"idle"}]),
        );
        assert_eq!(
            scene
                .cron
                .lock()
                .find_run_by_id(&run.run_id)
                .unwrap()
                .1
                .status,
            CronRunStatus::Running
        );
        let ended = scene.ended("cron-run-finished");
        scene.idle(tab, true, idle_first);
        ended.recv_timeout(Duration::from_secs(5)).unwrap();
        scene.assert_cron_end(&job, &run, CronRunStatus::Success);
        let path = scene.cron.lock().log_file(&job, &run.run_id).unwrap();
        let log = std::fs::read_to_string(path).unwrap();
        assert!(log.contains("opaque report"));
        assert!(log.contains("opaque final answer"));
    }
}

#[test]
fn runtime_oneshot_idle_without_report_settles_success_and_keeps_history() {
    let scene = Scene::new();
    let (job, run) = scene.cron();
    let ended = scene.ended("cron-run-finished");
    scene.idle(run.tab_id.as_ref().unwrap(), true, false);
    ended.recv_timeout(Duration::from_secs(5)).unwrap();
    scene.assert_cron_end(&job, &run, CronRunStatus::Success);
}

#[test]
fn runtime_oneshot_process_exit_settles_failure_and_keeps_history() {
    let scene = Scene::new();
    let (job, run) = scene.cron();
    let ended = scene.ended("cron-run-finished");
    let session = engine::for_tab(run.tab_id.as_ref().unwrap()).unwrap();
    let process = session.process.lock().clone().unwrap();
    let _ = tauri::async_runtime::block_on(process.client.request_guarded(
        &RpcCall {
            target: None,
            method: "fixture/exit".into(),
            params: json!({"code":23}),
        },
        None,
    ));
    ended.recv_timeout(Duration::from_secs(5)).unwrap();
    scene.assert_cron_end(&job, &run, CronRunStatus::Failed);
}

#[test]
fn runtime_memory_report_succeeds_and_idle_cleanup_keeps_reported_status() {
    for intent in ["extract:item", "dreaming:rem"] {
        let scene = Scene::new();
        let (worker, tab) = scene.worker(intent);
        let response = crate::socket::server::dispatch_service_rpc(
            "mem.worker.report",
            Some(json!({"id":worker,"summary":"opaque report"})),
            &Arc::new(scene.fixture.service.clone()),
        )
        .unwrap();
        assert_eq!(response["ok"], true);
        assert!(scene.fixture.service.mux.read().get_tab(&tab).is_some());
        let ended = scene.ended("graph-worker-done");
        scene.idle(&tab, true, false);
        ended.recv_timeout(Duration::from_secs(5)).unwrap();
        let info = scene.fixture.service.workers.get(&worker).unwrap();
        assert_eq!(info.status, crate::mem::workers::WorkerStatus::Done);
        assert!(matches!(
            info.execution_end,
            Some(codeterm_mem::dream_status::ExecutionEnd::Reported)
        ));
        assert!(info.session_id.is_some());
        assert!(scene.fixture.service.mux.read().get_tab(&tab).is_none());
    }
}

#[test]
fn runtime_memory_idle_without_report_records_unreported_execution_end() {
    for intent in ["extract:item", "dreaming:rem"] {
        let scene = Scene::new();
        let (worker, tab) = scene.worker(intent);
        let ended = scene.ended("graph-worker-done");
        scene.idle(&tab, true, false);
        ended.recv_timeout(Duration::from_secs(5)).unwrap();
        let info = scene.fixture.service.workers.get(&worker).unwrap();
        assert_eq!(info.status, crate::mem::workers::WorkerStatus::Error);
        assert!(matches!(
            info.execution_end,
            Some(codeterm_mem::dream_status::ExecutionEnd::ProcessEnded { .. })
        ));
        assert!(info.session_id.is_some());
        assert!(scene.fixture.service.mux.read().get_tab(&tab).is_none());
    }
}

#[test]
fn runtime_dreamer_process_exit_records_failure_and_closes() {
    let scene = Scene::new();
    let (worker, tab) = scene.worker("dreaming:rem");
    let ended = scene.ended("graph-worker-done");
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let _ = tauri::async_runtime::block_on(process.client.request_guarded(
        &RpcCall {
            target: None,
            method: "fixture/exit".into(),
            params: json!({"code":23}),
        },
        None,
    ));
    ended.recv_timeout(Duration::from_secs(5)).unwrap();
    let info = scene.fixture.service.workers.get(&worker).unwrap();
    assert_eq!(info.status, crate::mem::workers::WorkerStatus::Error);
    assert!(matches!(
        info.execution_end,
        Some(codeterm_mem::dream_status::ExecutionEnd::ProcessEnded { .. })
    ));
    assert!(scene.fixture.service.mux.read().get_tab(&tab).is_none());
}

#[test]
fn runtime_end_rejects_foreign_thread_and_nonce() {
    let scene = Scene::new();
    let (job, run) = scene.cron();
    let tab = run.tab_id.as_ref().unwrap();
    let session = engine::for_tab(tab).unwrap();
    let mut context = session.context.lock().clone();
    context.binding_nonce = "foreign-nonce".into();
    scene
        .fixture
        .service
        .run_watcher
        .runtime_turn_completed(&scene.fixture.service, &context);
    context = session.context.lock().clone();
    context.epoch += 1;
    scene
        .fixture
        .service
        .run_watcher
        .runtime_turn_completed(&scene.fixture.service, &context);
    context = session.context.lock().clone();
    context.thread_id = Some("foreign-thread".into());
    scene.fixture.service.run_watcher.runtime_process_exited(
        &scene.fixture.service,
        &context,
        "foreign-exit".into(),
    );
    assert_eq!(
        scene
            .cron
            .lock()
            .find_run_by_id(&run.run_id)
            .unwrap()
            .1
            .status,
        CronRunStatus::Running
    );
    let ended = scene.ended("cron-run-finished");
    scene.idle(tab, true, false);
    ended.recv_timeout(Duration::from_secs(5)).unwrap();
    scene.assert_cron_end(&job, &run, CronRunStatus::Success);
}
