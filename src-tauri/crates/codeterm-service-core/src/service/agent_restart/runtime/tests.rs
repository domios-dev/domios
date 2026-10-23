use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::agent_restart::options::RestartOptions;
use crate::service::test_fixture::ServiceFixture;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use serde_json::{json, Value};
use std::sync::Arc;

mod provider_switch;
mod rebind;

#[test]
fn restart_routing_requires_runtime_capability_and_chat_backing() {
    let chat = TabBacking::ChatBackend {
        plugin_id: "opaque-provider".into(),
        session_id: "thread".into(),
    };
    assert_eq!(restart_route(true, &chat), RestartRoute::Runtime);
    assert_eq!(restart_route(false, &chat), RestartRoute::Pty);
    assert_eq!(restart_route(true, &TabBacking::Pty), RestartRoute::Pty);
    assert_eq!(restart_route(false, &TabBacking::Pty), RestartRoute::Pty);
}

const PROVIDER: &str = "restart-fixture";
const PROMPT: &str = "opaque supplied identity";
const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    return {action:{kind:'request',call:{method:op.kind,params:op}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    if ((op.kind === 'resumeThread' && op.instructions.includes('opaque refused identity')) ||
        (op.kind === 'openThread' && op.settings.model === 'refused-model'))
      return {kind:'refused',reason:{code:'provider_refused',message:'fixture load refused'},beforeAcceptance:true};
    let result = {kind:'done'};
    if (op.kind === 'initialize') result = {kind:'initialized',binaryVersion:'fixture-runtime'};
    if (op.kind === 'resumeThread' || op.kind === 'openThread')
      result = {kind:'threadLoaded',threadId:op.threadId || ctx.bindingNonce,settings:op.settings};
    if (op.kind === 'readHistory') result = {kind:'historyPage',threadId:op.threadId,
      items:[{id:op.threadId+'-history',seq:0,type:'assistant',content:op.threadId+' history'}],openItems:[]};
    if (op.kind === 'submitTurn') result = {kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:'turn'};
    return {kind:'complete',result};
  },
  runtimeReduce(ctx, state, frames) { return {state:null,events:[]}; }
};
"#;

struct RuntimeFixture {
    fixture: ServiceFixture,
    tab: String,
    requests: Arc<parking_lot::Mutex<Vec<Value>>>,
    _process_ports: agent_runtime::process::test_ports::Registration,
    _storage: crate::storage::Storage,
}
impl std::ops::Deref for RuntimeFixture {
    type Target = ServiceFixture;
    fn deref(&self) -> &Self::Target {
        &self.fixture
    }
}
impl Drop for RuntimeFixture {
    fn drop(&mut self) {
        agent_runtime::engine::stop_blocking(&self.fixture.service, &self.tab).unwrap();
        crate::service::session_persist::stop_worker(&self.fixture.service).unwrap();
    }
}
impl RuntimeFixture {
    fn new() -> Self {
        let (mut mux, tab) = MultiplexerState::new();
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_type = Some(PROVIDER.into());
        target.agent_session_id = Some("own-thread".into());
        target.session_binding_nonce = Some("own-nonce".into());
        target.reported_session_binding_nonce = Some("own-nonce".into());
        target.agent_runtime_config = Some(json!({"systemPrompt":PROMPT,"model":"supplied-model"}));
        target.backing = TabBacking::ChatBackend {
            plugin_id: PROVIDER.into(),
            session_id: "own-thread".into(),
        };
        target.cwd = Some(
            crate::test_support::fixture_manifest_dir()
                .to_string_lossy()
                .into_owned()
                .into(),
        );
        target.role = Some(crate::multiplexer::types::AgentRole::Worker);
        let fixture = ServiceFixture::new(mux);
        fixture.set_agent_full_access(false);
        let storage = fixture.install_seeded_storage_desk();
        let requests = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let captured = requests.clone();
        let process_ports = agent_runtime::process::test_ports::register(&tab, move || {
            crate::async_runtime::block_on(async {
                use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
                let (process, peer) = agent_runtime::process::test_ports::connected();
                let (peer_reader, mut peer_writer) = tokio::io::split(peer);
                let captured = captured.clone();
                crate::async_runtime::spawn(async move {
                    let mut lines = BufReader::new(peer_reader).lines();
                    while let Some(line) = lines.next_line().await.unwrap() {
                        let frame: Value = serde_json::from_str(&line).unwrap();
                        captured.lock().push(frame.clone());
                        let reply = json!({"id":frame["id"],"result":{"params":frame["params"]}});
                        if peer_writer
                            .write_all(format!("{reply}\n").as_bytes())
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                });
                process
            })
        });
        let methods = [
            "initialize",
            "openThread",
            "resumeThread",
            "readHistory",
            "submitTurn",
        ]
        .into_iter()
        .map(|kind| (kind.to_string(), json!([kind])))
        .collect::<serde_json::Map<String, Value>>();
        let manifest = PluginManifest::from_json_str(&json!({
            "id":PROVIDER,"version":"1.0.0","displayName":"Fixture","binaryName":"node",
            "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
                "agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":"node","args":[]},"requestMethods":methods,"notificationMethods":[]}},
            "spawn":{"readinessStrategy":{"kind":"poll_shell_prompt","timeoutSecs":1},
                "taskDelivery":"inline_flag","systemPromptDelivery":{"kind":"cli_flag","flag":"--opaque-system"},
                "verifySubmit":{"kind":"none"}},
            "permissions":{"subprocess":{"allow":["node"]}}
        }).to_string()).unwrap();
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(JsProvider::new(
                manifest,
                Arc::new(PluginVm::from_source(PROVIDER, SOURCE.into()).unwrap()),
            )));
        agent_runtime::transport::launch(
            &fixture.service,
            PROVIDER,
            &tab,
            json!({"systemPrompt":PROMPT,"model":"supplied-model"}),
        )
        .unwrap();
        Self {
            fixture,
            tab,
            requests,
            _process_ports: process_ports,
            _storage: storage,
        }
    }
    fn requests(&self) -> Vec<Value> {
        self.requests.lock().clone()
    }
    fn prompt(&self) -> String {
        let mux = self.service.mux.read();
        let prompt = mux
            .get_tab(&self.tab)
            .unwrap()
            .agent_runtime_config
            .as_ref()
            .unwrap()["systemPrompt"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(prompt.matches(PROMPT).count(), 1);
        assert_eq!(
            prompt
                .matches(crate::service::orchestrator::ROLE_AUTHORITY)
                .count(),
            1
        );
        prompt
    }
}

#[test]
fn runtime_resume_keeps_thread_rotates_nonce_and_never_replays_uncertain_input() {
    let fixture = RuntimeFixture::new();
    let prompt = fixture.prompt();
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    let prior = session.context.lock().clone();
    let pending = fixture
        .service
        .chat_send_ledger
        .try_start(&fixture.tab, "opaque input", None)
        .unwrap();
    agent_runtime::transport::send(
        &fixture.service,
        &fixture.tab,
        &crate::service::messenger::AgentInput::Human(
            crate::service::messenger::HumanInput::composer("opaque input"),
        ),
        Some(&pending.send_id),
    )
    .unwrap();
    fixture
        .service
        .chat_send_ledger
        .mark_rpc_uncertain(&pending.send_id);
    let count = fixture.requests().len();
    let mut bootstrap = crate::multiplexer::types::PendingDeliveryRecord::new(
        "already-written-digest".into(),
        crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
    )
    .with_send_id("already-claimed-bootstrap".into());
    bootstrap
        .begin_attempt(crate::service::launch_delivery::next_attempt())
        .unwrap();
    bootstrap.mark_unconfirmed();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .pending_delivery = Some(bootstrap);
    let outcome = fixture
        .service
        .agent_restart(&fixture.tab, false, 0)
        .unwrap();
    assert!(outcome.agent_ready && outcome.binding_restored);
    assert!(outcome.old_pty_id.is_none() && outcome.new_pty_id.is_none());
    assert_eq!(outcome.session_id.as_deref(), Some("own-thread"));
    assert_eq!(outcome.session_action(), "resumed");
    let current = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    assert!(Arc::ptr_eq(&session, &current));
    let context = current.context.lock().clone();
    assert!(context.epoch > prior.epoch);
    assert_ne!(context.binding_nonce, prior.binding_nonce);
    assert_eq!(context.thread_id, prior.thread_id);
    assert!(current
        .thread_loaded
        .load(std::sync::atomic::Ordering::Acquire));
    let requests = fixture.requests();
    assert_eq!(
        requests[count..]
            .iter()
            .map(|request| request["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["initialize", "resumeThread", "readHistory"]
    );
    assert_eq!(requests[count + 1]["params"]["instructions"], prompt);
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&fixture.tab).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("own-thread"));
    assert_eq!(
        tab.reported_session_binding_nonce,
        Some(context.binding_nonce)
    );
    assert_eq!(
        tab.restorable_launch
            .runtime_launch_options
            .as_ref()
            .unwrap()
            .binding_nonce,
        tab.session_binding_nonce.as_deref().unwrap()
    );
}

#[test]
fn runtime_fresh_opens_new_thread_with_same_identity_and_requires_authorization() {
    let fixture = RuntimeFixture::new();
    let prompt = fixture.prompt();
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .clone();
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    let prior = session.context.lock().clone();
    let count = fixture.requests().len();
    let outcome = fixture
        .service
        .agent_restart_observed_with_options(
            &fixture.tab,
            RestartOptions {
                fresh: true,
                force: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(outcome.agent_ready);
    assert_ne!(outcome.session_id, tab.agent_session_id);
    assert_eq!(outcome.session_action(), "fresh_launch");
    let current = session.context.lock().clone();
    assert!(current.epoch > prior.epoch);
    assert_ne!(current.binding_nonce, prior.binding_nonce);
    let requests = fixture.requests();
    assert_eq!(requests[count + 1]["method"], "openThread");
    assert_eq!(requests[count + 1]["params"]["instructions"], prompt);
    assert_eq!(
        requests[count + 1]["params"]["settings"]["model"],
        "supplied-model"
    );
    assert!(requests[count..]
        .iter()
        .all(|request| request["method"] != "submitTurn"));
    let mux = fixture.service.mux.read();
    let current = mux.get_tab(&fixture.tab).unwrap();
    assert_eq!(current.role, tab.role);
    assert_eq!(current.agent_runtime_config, tab.agent_runtime_config);
    assert!(
        matches!(&current.backing,TabBacking::ChatBackend{session_id,..} if Some(session_id)==outcome.session_id.as_ref())
    );
}

#[test]
fn runtime_load_failure_never_reports_ready_and_remains_visibly_degraded() {
    let fixture = RuntimeFixture::new();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .agent_runtime_config = Some(json!({"systemPrompt":"opaque refused identity"}));
    let failure = fixture
        .service
        .agent_restart_observed(&fixture.tab, false, 0)
        .unwrap_err();
    assert!(
        matches!(failure,RestartAttemptError::Failed{outcome,..} if !outcome.agent_ready && outcome.restart_accepted)
    );
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    assert!(!session
        .thread_loaded
        .load(std::sync::atomic::Ordering::Acquire));
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&fixture.tab).unwrap();
    assert_eq!(
        tab.agent_runtime_state.as_ref().unwrap()["state"],
        "degraded"
    );
    assert!(!tab.agent_booting);
    assert_ne!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady)
    );
}

#[test]
fn runtime_provider_handoff_has_a_typed_refusal_before_any_restart() {
    let fixture = RuntimeFixture::new();
    assert!(matches!(
        fixture
            .service
            .require_restart_provider_handoff(PROVIDER, "other-provider"),
        Err(AgentRestartError::RuntimeProviderHandoffUnsupported)
    ));
    assert!(matches!(
        fixture
            .service
            .require_restart_provider_handoff("other-provider", PROVIDER),
        Err(AgentRestartError::RuntimeProviderHandoffUnsupported)
    ));
    assert!(fixture
        .service
        .require_restart_provider_handoff(PROVIDER, PROVIDER)
        .is_ok());
    let count = fixture.requests().len();
    assert!(matches!(
        fixture.service.agent_restart_observed_with_options(
            &fixture.tab,
            RestartOptions {
                target_provider: Some("other-provider".into()),
                ..Default::default()
            }
        ),
        Err(RestartAttemptError::Request(
            AgentRestartError::RuntimeProviderHandoffUnsupported
        ))
    ));
    assert_eq!(fixture.requests().len(), count);
}

#[test]
fn task_bound_runtime_worker_preserves_attempt_and_ack_across_resume_and_fresh() {
    use crate::plan::{OrchestratorRef, Plan, PlanMode};
    use crate::service::plan::{task_assignment::AssignmentRuntimeBinding, NewTask};
    for fresh in [false, true] {
        let fixture = RuntimeFixture::new();
        let store = &fixture.service.plan_store;
        let mut plan = Plan::new(
            "runtime restart".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab("orchestrator"),
        );
        plan.mode = PlanMode::Execution;
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "supplied task".into(),
                    kind: "implement".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        store
            .assign_task_with_runtime_binding(
                &plan.id,
                &task_id,
                &fixture.tab,
                None,
                Some("worker"),
                Some(&AssignmentRuntimeBinding {
                    session_id: "own-thread".into(),
                    session_binding_nonce: "own-nonce".into(),
                    provider: Some(PROVIDER.into()),
                    model: None,
                    session_model: None,
                    reasoning_effort: None,
                }),
                None,
                3,
            )
            .unwrap();
        let before = store.show_task(&task_id).unwrap().task;
        let attempt = before.execution_attempts[0].id.clone();
        store
            .acknowledge_task_attempt(&plan.id, &task_id, &fixture.tab, &attempt, 4)
            .unwrap();
        let before = store.show_task(&task_id).unwrap().task;
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&fixture.tab)
            .unwrap()
            .task_binding = Some(crate::multiplexer::types::TaskBinding {
            plan_id: Some(plan.id.clone()),
            task_id: Some(task_id.clone()),
            attempt_id: Some(attempt.clone()),
            spawn_handle: None,
            bound_at: 3,
        });
        let outcome = fixture
            .service
            .agent_restart_observed_with_options(
                &fixture.tab,
                RestartOptions {
                    fresh,
                    force: fresh,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(outcome.agent_ready && outcome.binding_restored);
        let after = store.show_task(&task_id).unwrap().task;
        assert_eq!(after.execution_attempts.len(), 1);
        let old = &before.execution_attempts[0];
        let current = &after.execution_attempts[0];
        assert_eq!(current.id, old.id);
        assert_eq!(current.acknowledged_at, old.acknowledged_at);
        assert_eq!(current.closed_at, old.closed_at);
        assert_eq!(current.retired_at, old.retired_at);
        assert_eq!(after.status, before.status);
        assert_eq!(
            after.interruption.owner_epoch,
            before.interruption.owner_epoch
        );
        assert_eq!(current.session_id, outcome.session_id);
        assert_eq!(after.interruption.owner_session_id, outcome.session_id);
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&fixture.tab).unwrap();
        assert_eq!(current.session_binding_nonce, tab.session_binding_nonce);
        assert_eq!(
            tab.task_binding.as_ref().unwrap().attempt_id.as_deref(),
            Some(attempt.as_str())
        );
        drop(mux);
        let reopened = crate::service::plan::PlanStore::new_for_test(fixture.temp.path());
        let persisted = reopened.show_task(&task_id).unwrap().task;
        assert_eq!(
            persisted.execution_attempts[0].session_id,
            current.session_id
        );
        assert_eq!(
            persisted.execution_attempts[0].session_binding_nonce,
            current.session_binding_nonce
        );
    }
}

#[test]
fn fresh_runtime_restart_without_force_is_refused_before_rotating_live_binding() {
    let fixture = RuntimeFixture::new();
    let before = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .clone();
    assert!(matches!(
        fixture.service.agent_restart_observed_with_options(
            &fixture.tab,
            RestartOptions {
                fresh: true,
                ..Default::default()
            }
        ),
        Err(RestartAttemptError::Request(AgentRestartError::BadRequest(
            _
        )))
    ));
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&fixture.tab).unwrap();
    assert_eq!(tab.session_binding_nonce, before.session_binding_nonce);
    assert_eq!(tab.agent_session_id, before.agent_session_id);
}

#[test]
fn provider_update_idle_now_busy_after_turn_preserves_thread_without_input() {
    let fixture = RuntimeFixture::new();
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    let original = session.context.lock().clone();
    let count = fixture.requests().len();
    crate::service::provider_runtime_update::completed(
        &fixture.service,
        PROVIDER,
        "1",
        "fixture-runtime",
    );
    crate::async_runtime::block_on(async {
        let mut changes = session.control_changes.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                changes.borrow_and_update();
                let context = session.context.lock().clone();
                if context.binding_nonce != original.binding_nonce
                    && session
                        .thread_loaded
                        .load(std::sync::atomic::Ordering::Acquire)
                    && matches!(
                        session.state.lock().phase,
                        agent_runtime::model::RuntimePhase::Ready
                    )
                {
                    break;
                }
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    });
    let idle = session.context.lock().clone();
    assert_eq!(idle.thread_id, original.thread_id);
    session.context.lock().active_turn_id = Some("busy-turn".into());
    let count_busy = fixture.requests().len();
    let deferred = crate::async_runtime::block_on(agent_runtime::restart::restart_idle(
        &fixture.service,
        &fixture.tab,
        PROVIDER,
        &idle.binding_nonce,
    ))
    .unwrap();
    assert!(deferred.is_none());
    assert_eq!(fixture.requests().len(), count_busy);
    crate::service::provider_runtime_update::completed(
        &fixture.service,
        PROVIDER,
        "2",
        "fixture-runtime",
    );
    session.context.lock().active_turn_id = None;
    agent_runtime::engine::publish(&fixture.service, &fixture.tab, &session);
    crate::async_runtime::block_on(async {
        let mut changes = session.control_changes.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                changes.borrow_and_update();
                if session.context.lock().binding_nonce != idle.binding_nonce
                    && session
                        .thread_loaded
                        .load(std::sync::atomic::Ordering::Acquire)
                    && matches!(
                        session.state.lock().phase,
                        agent_runtime::model::RuntimePhase::Ready
                    )
                {
                    break;
                }
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    });
    assert_eq!(session.context.lock().thread_id, original.thread_id);
    assert!(fixture.requests()[count..]
        .iter()
        .all(|request| request["method"] != "submitTurn"));
}

#[test]
fn provider_update_version_mismatch_recovers_once_then_reports_failure() {
    let fixture = RuntimeFixture::new();
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    let count = fixture.requests().len();
    let thread = session.context.lock().thread_id.clone();
    crate::service::provider_runtime_update::completed(&fixture.service, PROVIDER, "old", "wanted");
    crate::async_runtime::block_on(async {
        let mut changes = session.control_changes.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                changes.borrow_and_update();
                if fixture
                    .service
                    .mux
                    .read()
                    .get_tab(&fixture.tab)
                    .unwrap()
                    .agent_runtime_state
                    .as_ref()
                    .is_some_and(|state| state["state"] == "degraded")
                {
                    break;
                }
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    });
    let requests = fixture.requests();
    assert_eq!(
        requests[count..]
            .iter()
            .filter(|request| request["method"] == "initialize")
            .count(),
        2
    );
    assert_eq!(
        requests[count..]
            .iter()
            .filter(|request| request["method"] == "resumeThread")
            .count(),
        2
    );
    assert!(requests[count..]
        .iter()
        .all(|request| request["method"] != "submitTurn"));
    assert_eq!(session.context.lock().thread_id, thread);
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .clone();
    assert_eq!(
        tab.agent_runtime_state.as_ref().unwrap()["state"],
        "degraded"
    );
    assert_eq!(
        tab.agent_runtime_state.as_ref().unwrap()["binaryVersion"],
        "fixture-runtime"
    );
}
