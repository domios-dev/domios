use super::*;
use crate::service::test_fixture::ServiceFixture;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use std::sync::Arc;

const PROVIDER: &str = "dispatch-runtime-fixture";
const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    return {action:{kind:'request',call:{method:op.kind,params:op}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    let result = {kind:'done'};
    if (op.kind === 'initialize') result = {kind:'initialized'};
    if (op.kind === 'openThread' || op.kind === 'resumeThread')
      result = {kind:'threadLoaded',threadId:op.threadId || 'thread-' + ctx.tabId,settings:op.settings};
    if (op.kind === 'readHistory')
      result = {kind:'historyPage',threadId:op.threadId,items:[],openItems:[]};
    if (op.kind === 'submitTurn')
      result = {kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:'own-turn'};
    return {kind:'complete',result,events:[]};
  },
  runtimeReduce(ctx, state, frames) { return {state:null,events:[]}; }
};
"#;

struct RuntimeDispatchFixture {
    fixture: ServiceFixture,
    _storage: crate::storage::Storage,
}

impl Drop for RuntimeDispatchFixture {
    fn drop(&mut self) {
        let tabs = self.fixture.service.mux.read().snapshot().tabs;
        for id in tabs.keys() {
            crate::service::agent_runtime::engine::stop(id);
        }
        crate::service::session_persist::stop_worker(self.fixture._app.handle()).unwrap();
    }
}

fn fixture() -> (RuntimeDispatchFixture, String, String) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner).unwrap();
    mux.resolve_or_mint_team_for_parent(&owner).unwrap();
    {
        let tab = mux.get_tab_mut(&owner).unwrap();
        tab.agent_session_id = Some("owner-session".into());
        tab.session_binding_nonce = Some("owner-nonce".into());
        tab.reported_session_binding_nonce = Some("owner-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    let storage = crate::storage::Storage::open(
        &fixture.temp.path().join("runtime-desk"),
        crate::storage::StorageConfig::default(),
    )
    .unwrap();
    crate::service::session_persist::install_storage_desk_adapter(
        fixture._app.handle(),
        crate::session::store::StorageDeskAdapter::from_storage(storage.clone()),
    )
    .unwrap();
    let methods = [
        "initialize",
        "openThread",
        "resumeThread",
        "readHistory",
        "submitTurn",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), json!([name])))
    .collect::<serde_json::Map<String, serde_json::Value>>();
    let manifest = PluginManifest::from_json_str(&json!({
        "id": PROVIDER, "version":"1.0.0", "displayName":"Fixture",
        "capabilities":{"agentProvider":true,
            "chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtime":"stdio-jsonrpc",
                "command":{"binary":"node","args":[format!("{}/../tests/fixtures/agent-runtime/fake-peer.mjs",env!("CARGO_MANIFEST_DIR"))]},
                "requestMethods":methods,"notificationMethods":[]}},
        "permissions":{"subprocess":{"allow":["node"]}}
    }).to_string()).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(PROVIDER, SOURCE.into()).unwrap()),
        )));
    (
        RuntimeDispatchFixture {
            fixture,
            _storage: storage,
        },
        owner,
        workspace,
    )
}

#[test]
fn socket_plan_task_dispatch_runtime_preserves_binding_and_reaches_worker_ack() {
    let (harness, owner, workspace) = fixture();
    let svc = &harness.fixture.service;
    let team = svc
        .mux
        .read()
        .get_tab(&owner)
        .unwrap()
        .team_id
        .clone()
        .unwrap();
    let mut plan = svc
        .plan_store
        .create_plan_for_team(
            "runtime dispatch".into(),
            harness.fixture.temp.path().display().to_string(),
            OrchestratorRef {
                tab_id: Some(owner.clone()),
                session_id: Some("owner-session".into()),
                title: None,
            },
            team,
            vec![],
            1,
        )
        .unwrap();
    plan.mode = PlanMode::Approved;
    plan.planning_state = crate::plan::PlanningState::Approved;
    svc.plan_store.save(&mut plan, 2).unwrap();
    let task = svc
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "runtime task".into(),
                personality: Some("Ada".into()),
                executor_role: Some("Probe".into()),
                role_guidance: Some("opaque-role".into()),
                body: "opaque-task".into(),
                execution_intent: crate::plan::TaskExecutionIntent::NonRepository,
                ..Default::default()
            },
            3,
        )
        .unwrap();
    crate::service::plan::plan_skills::create_plan_skill(
        &svc.plan_store,
        &plan.id,
        crate::service::plan::plan_skills::NewPlanSkill {
            slug: "opaque-skill".into(),
            title: Some("Opaque".into()),
            area: None,
            check: None,
            rationale: None,
        },
    )
    .unwrap();
    let manifest =
        crate::service::plan::plan_skills::plan_skill_manifest(&svc.plan_store, &plan.id).unwrap();
    assert_eq!(manifest.len(), 1);
    let before = svc.mux.read().snapshot().tabs.len();
    let response = super::super::dispatch(
        &Some(json!({
            "plan_id":plan.id,"task_id":task,"agent":PROVIDER,
            "caller_tab_id":owner,"parent_tab_id":owner,"workspace_id":workspace,
            "session_id":"owner-session","session_binding_nonce":"owner-nonce",
            "execution_intent":"read_only","authorized_gates":["opaque-gate"],
            "provider_reserve_pct":17.25,
        })),
        harness.fixture._app.handle(),
        &Arc::new(svc.clone()),
        10,
    )
    .unwrap();
    let worker = response["tab_id"].as_str().unwrap();
    let attempt_id = response["attempt_id"].as_str().unwrap();
    assert_eq!(svc.mux.read().snapshot().tabs.len(), before + 1);
    let tab = svc.mux.read().get_tab(worker).unwrap().clone();
    assert!(matches!(
        tab.backing,
        crate::multiplexer::types::TabBacking::ChatBackend { .. }
    ));
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady)
    );
    let binding = tab.task_binding.unwrap();
    assert_eq!(binding.plan_id.as_deref(), Some(plan.id.as_str()));
    assert_eq!(binding.task_id.as_deref(), Some(task.as_str()));
    assert_eq!(binding.attempt_id.as_deref(), Some(attempt_id));
    let detail = svc.plan_store.show_task(&task).unwrap();
    assert_eq!(
        detail.task.status_label.as_deref(),
        Some("awaiting_worker_ack")
    );
    assert_eq!(
        detail.task.interruption.execution_intent,
        crate::plan::ExecutionIntent::ReadOnly
    );
    let attempt = detail.task.execution_attempts.last().unwrap();
    assert_eq!(attempt.id, attempt_id);
    assert_eq!(
        attempt.authorized_gates.as_deref(),
        Some(&["opaque-gate".into()][..])
    );
    assert_eq!(
        serde_json::to_vec(&attempt.plan_skill_manifest).unwrap(),
        serde_json::to_vec(&manifest).unwrap()
    );
    assert_eq!(attempt.session_id, tab.agent_session_id);
    assert_eq!(attempt.session_binding_nonce, tab.session_binding_nonce);
    svc.plan_store
        .acknowledge_task_attempt(&plan.id, &task, worker, attempt_id, 11)
        .unwrap();
    let acknowledged = svc.plan_store.show_task(&task).unwrap().task;
    assert_eq!(acknowledged.status, TaskStatus::InProgress);
    assert!(acknowledged
        .execution_attempts
        .last()
        .unwrap()
        .acknowledged_at
        .is_some());
}

#[test]
fn runtime_sync_spawn_reports_bound_session_readiness() {
    let (harness, owner, workspace) = fixture();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    let spawn = harness
        .fixture
        .service
        .spawn_agent_sync_with_ready(
            crate::service::SpawnAgentParams {
                agent: Some(PROVIDER.into()),
                workspace_id: Some(workspace),
                parent_tab_id: Some(owner),
                cwd: Some(harness.fixture.temp.path().display().to_string()),
                ..Default::default()
            },
            Some(tx),
        )
        .unwrap();
    assert_eq!(rx.try_recv().unwrap(), Ok(()));
    let tab = harness
        .fixture
        .service
        .mux
        .read()
        .get_tab(&spawn.tab_id)
        .unwrap()
        .clone();
    assert!(tab.agent_session_id.is_some());
    assert_eq!(
        tab.reported_session_binding_nonce,
        tab.session_binding_nonce
    );
}
