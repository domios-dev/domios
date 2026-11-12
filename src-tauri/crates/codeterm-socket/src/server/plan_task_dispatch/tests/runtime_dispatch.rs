use super::*;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use codeterm_service_core::service::test_fixture::ServiceFixture;
use std::sync::Arc;

mod checkout_binding;

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
    _storage: codeterm_service_core::storage::Storage,
}

impl Drop for RuntimeDispatchFixture {
    fn drop(&mut self) {
        let tabs = self.fixture.service.mux.read().snapshot().tabs;
        for id in tabs.keys() {
            codeterm_service_core::service::agent_runtime::engine::stop_blocking(
                &self.fixture.service,
                id,
            )
            .unwrap();
        }
        codeterm_service_core::service::session_persist::stop_worker(&self.fixture.service)
            .unwrap();
    }
}

fn fixture() -> (RuntimeDispatchFixture, String, String) {
    fixture_with_profiles(None)
}

fn fixture_with_profiles(
    profiles: Option<serde_json::Value>,
) -> (RuntimeDispatchFixture, String, String) {
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
    let storage = fixture.install_seeded_storage_desk();
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
    let mut manifest = json!({
        "id": PROVIDER, "version":"1.0.0", "displayName":"Fixture",
        "commands":{"autoApproveFlags":["opaque-full-access"]},
        "capabilities":{"agentProvider":true,
            "chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtimeModes":[{"id":"full-access","displayName":"Full"}],"runtime":"stdio-jsonrpc",
                "command":{"binary":"node","args":[format!("{}/../../../tests/fixtures/agent-runtime/fake-peer.mjs",codeterm_service_core::test_support::fixture_manifest_dir().display())]},
                "requestMethods":methods,"notificationMethods":[]}},
        "permissions":{"subprocess":{"allow":["node"]}},
        "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},
            "taskDelivery":"pty_input","verifySubmit":{"kind":"none"},
            "executionSandbox":{
            "platforms":[codeterm_service_core::agent::provider::current_platform_tag()],
            "readOnly":{"launchArgs":[],"runtimeMode":"read-only"}
        }}
    });
    if let Some(profiles) = profiles {
        manifest["capabilities"]["agentRuntime"]["launchProfiles"] = profiles;
    }
    let manifest = PluginManifest::from_json_str(&manifest.to_string()).unwrap();
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
    assert_runtime_task_dispatch(Some("Ada"), "Ada", false);
}

#[test]
fn socket_task_added_with_explicit_personality_dispatches_with_planned_title() {
    assert_runtime_task_dispatch(Some("Prism"), "Prism", false);
}

#[test]
fn readonly_nonrepo_cwd_dispatch_ack_and_in_review_persist_coherent_attempt() {
    assert_runtime_task_dispatch(Some("Probe"), "Probe", true);
}

fn assert_runtime_task_dispatch(
    personality: Option<&str>,
    expected_title: &str,
    repository_task: bool,
) {
    let (harness, owner, workspace) = fixture_with_profiles(Some(json!({
        "selected": {"binary": "node", "args": [format!("{}/../../../tests/fixtures/agent-runtime/fake-peer.mjs", codeterm_service_core::test_support::fixture_manifest_dir().display())]}
    })));
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
    plan.planning_state = codeterm_service_core::plan::PlanningState::Approved;
    svc.plan_store.save(&mut plan, 2).unwrap();
    let task_name = "Prism runtime task";
    let mut add = json!({
        "plan_id": plan.id, "name": task_name, "executor_role": "Probe",
        "role_guidance": "opaque-role", "body": "opaque-task",
        "execution_intent": "non_repository", "caller_tab_id": owner,
        "session_id": "owner-session", "session_binding_nonce": "owner-nonce",
    });
    add["acceptance_criteria"] = json!(["opaque-result"]);
    add["test_plan"] = json!("opaque-verification");
    if repository_task {
        add["execution_intent"] = json!("repository_mutation");
    }
    if let Some(personality) = personality {
        add["personality"] = json!(personality);
    }
    let added = crate::server::plan_task_rpc::dispatch(
        "plan.task.add",
        &Some(add),
        &Arc::new(svc.clone()),
        3,
    )
    .unwrap();
    let task = added["task_id"].as_str().unwrap().to_owned();
    assert_eq!(
        svc.plan_store
            .inspect_task_for_test(&task)
            .unwrap()
            .task
            .personality
            .as_deref(),
        Some(expected_title),
    );
    codeterm_service_core::service::plan::plan_skills::create_plan_skill(
        &svc.plan_store,
        &plan.id,
        codeterm_service_core::service::plan::plan_skills::NewPlanSkill {
            slug: "opaque-skill".into(),
            title: Some("Opaque".into()),
            area: None,
            check: None,
            rationale: None,
        },
    )
    .unwrap();
    let manifest = codeterm_service_core::service::plan::plan_skills::plan_skill_manifest(
        &svc.plan_store,
        &plan.id,
    )
    .unwrap();
    assert_eq!(manifest.len(), 1);
    let (changed_tx, changed) = std::sync::mpsc::channel();
    harness.fixture.ui.listen("plan-changed", move |_| {
        let _ = changed_tx.send(());
    });
    let working_dir = harness.fixture.temp.path().join("nonrepo-cwd");
    std::fs::create_dir(&working_dir).unwrap();
    let before = svc.mux.read().snapshot().tabs.len();
    let response = super::super::dispatch(
        &Some(json!({
            "plan_id":plan.id,"task_id":task,"agent":PROVIDER,"launch_profile":"selected",
            "caller_tab_id":owner,"parent_tab_id":owner,"workspace_id":workspace,
            "session_id":"owner-session","session_binding_nonce":"owner-nonce",
            "execution_intent":"read_only","authorized_gates":["opaque-gate"],
            "cwd": working_dir,
            "provider_reserve_pct":17.25,
        })),
        &Arc::new(svc.clone()),
        10,
    )
    .unwrap();
    let worker = response["tab_id"].as_str().unwrap();
    let attempt_id = response["attempt_id"].as_str().unwrap();
    assert_eq!(response["placement"], "read_only");
    assert_eq!(svc.mux.read().snapshot().tabs.len(), before + 1);
    // Dispatch returns before the Worker runtime is ready; its startup commits the attempt binding.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while svc
        .plan_store
        .inspect_task_for_test(&task)
        .unwrap()
        .task
        .execution_attempts
        .last()
        .and_then(|attempt| attempt.session_binding_nonce.clone())
        .is_none()
    {
        changed
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .expect("dispatch startup did not commit the Worker binding");
    }
    let tab = svc.mux.read().get_tab(worker).unwrap().clone();
    assert_eq!(
        tab.agent_runtime_config.as_ref().unwrap()["launchProfile"],
        "selected"
    );
    assert_eq!(
        svc.plan_store
            .list_task_attempts(&plan.id, &task)
            .unwrap()
            .last()
            .unwrap()
            .attempt
            .launch_profile
            .as_deref(),
        Some("selected")
    );
    assert!(tab.restorable_launch.execution_sandbox.is_none());
    assert!(tab.restorable_launch.permission_bypass_requested);
    assert_eq!(
        tab.restorable_launch
            .runtime_launch_options
            .as_ref()
            .unwrap()
            .mode,
        "full-access"
    );
    assert_eq!(
        tab.agent_runtime_config.as_ref().unwrap()["mode"],
        "full-access"
    );
    assert!(codeterm_paths::paths_equal(
        tab.cwd.as_deref().unwrap(),
        &working_dir.to_string_lossy()
    ));
    assert_eq!(tab.title, expected_title);
    assert_ne!(tab.title, "Worker");
    assert_eq!(tab.description.as_deref(), Some(task_name));
    assert_eq!(
        tab.description_origin,
        codeterm_service_core::multiplexer::types::TabDescriptionOrigin::Task
    );
    svc.set_tab_description(worker, "manual worker note")
        .unwrap();
    assert!(!svc
        .update_task_tab_description(worker, "renamed runtime task")
        .unwrap());
    assert!(!svc
        .assign_task_tab_description(worker, "reassigned runtime task")
        .unwrap());
    assert_eq!(
        svc.mux
            .read()
            .get_tab(worker)
            .unwrap()
            .description
            .as_deref(),
        Some("manual worker note")
    );
    assert!(matches!(
        tab.backing,
        codeterm_service_core::multiplexer::types::TabBacking::ChatBackend { .. }
    ));
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady)
    );
    let binding = tab.task_binding.unwrap();
    assert_eq!(binding.plan_id.as_deref(), Some(plan.id.as_str()));
    assert_eq!(binding.task_id.as_deref(), Some(task.as_str()));
    assert_eq!(binding.attempt_id.as_deref(), Some(attempt_id));
    let detail = svc.plan_store.inspect_task_for_test(&task).unwrap();
    assert_eq!(
        detail.task.status_label.as_deref(),
        Some("awaiting_worker_ack")
    );
    assert_eq!(
        detail.task.interruption.execution_intent,
        codeterm_service_core::plan::ExecutionIntent::ReadOnly
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
    let acknowledged = svc.plan_store.inspect_task_for_test(&task).unwrap().task;
    assert_eq!(acknowledged.status, TaskStatus::InProgress);
    assert!(acknowledged
        .execution_attempts
        .last()
        .unwrap()
        .acknowledged_at
        .is_some());
    let attempt = acknowledged.execution_attempts.last().unwrap();
    assert_eq!(
        attempt.placement,
        codeterm_service_core::plan::WorkerPlacement::ReadOnly
    );
    assert_eq!(
        attempt.execution_intent,
        Some(codeterm_service_core::plan::ExecutionIntent::ReadOnly)
    );
    assert_eq!(attempt.checkout_path, None);
    assert_eq!(attempt.worktree_path, None);
    assert_eq!(acknowledged.lane.worktree_path, None);
    let accepted = svc
        .plan_store
        .ingest_bound_worker_report(
            &plan.id,
            &task,
            &codeterm_service_core::plan::AgentRef::from_tab(worker),
            codeterm_service_core::service::plan::task_attempts::BoundWorkerReport {
                message: "opaque-result".into(),
                status: Some(codeterm_types::agent::ReportStatus::InReview),
                authority: Some(
                    codeterm_service_core::service::plan::interruption::TaskMutationAuthority {
                        tab_id: worker.into(),
                        session_id: attempt.session_id.clone().unwrap(),
                        session_binding_nonce: attempt.session_binding_nonce.clone().unwrap(),
                        owner_epoch: attempt.interruption.owner_epoch,
                    },
                ),
                ..Default::default()
            },
            12,
        )
        .unwrap()
        .committed_review;
    assert!(accepted);
    let persisted = svc.plan_store.inspect_task_for_test(&task).unwrap().task;
    assert_eq!(persisted.status, TaskStatus::InReview);
    let report = persisted
        .execution_attempts
        .last()
        .unwrap()
        .report
        .as_ref()
        .unwrap();
    assert_eq!(report.status.as_deref(), Some("in_review"));
    assert_eq!(report.sha, None);
}

#[test]
fn runtime_sync_spawn_reports_bound_session_readiness() {
    let (harness, owner, workspace) = fixture();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let spawn = harness
        .fixture
        .service
        .spawn_agent_sync_with_ready(
            codeterm_service_core::service::SpawnAgentParams {
                agent: Some(PROVIDER.into()),
                workspace_id: Some(workspace),
                parent_tab_id: Some(owner),
                cwd: Some(harness.fixture.temp.path().display().to_string()),
                ..Default::default()
            },
            Some(tx),
        )
        .unwrap();
    assert_eq!(rx.blocking_recv().unwrap(), Ok(()));
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

#[test]
fn dispatch_launch_profile_refuses_before_tab_member_or_attempt_reservation() {
    let (harness, owner, workspace) = fixture_with_profiles(Some(json!({
        "selected": {"binary": "node", "args": [], "ownerConfigured": true}
    })));
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
            "profile refusal".into(),
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
    plan.planning_state = codeterm_service_core::plan::PlanningState::Approved;
    svc.plan_store.save(&mut plan, 2).unwrap();
    let added = crate::server::plan_task_rpc::dispatch("plan.task.add", &Some(json!({
        "plan_id": plan.id, "name": "Profile task", "executor_role": "Probe", "personality": "Ada",
        "role_guidance": "opaque-role", "body": "opaque-task", "execution_intent": "non_repository",
        "acceptance_criteria": ["opaque-result"], "test_plan": "opaque-verification",
        "caller_tab_id": owner, "session_id": "owner-session", "session_binding_nonce": "owner-nonce"
    })), &Arc::new(svc.clone()), 3).unwrap();
    let task = added["task_id"].as_str().unwrap();
    let before = svc.mux.read().snapshot().tabs.len();
    for (profile, code) in [
        (None, "launch_profile_required"),
        (Some("unknown"), "unknown_launch_profile"),
        (Some("selected"), "launch_profile_not_installed"),
    ] {
        let error = super::super::dispatch_with_spawn(&Some(json!({
            "plan_id": plan.id, "task_id": task, "agent": PROVIDER,
            "launch_profile": profile, "caller_tab_id": owner, "parent_tab_id": owner,
            "workspace_id": workspace, "session_id": "owner-session", "session_binding_nonce": "owner-nonce",
            "execution_intent": "read_only"
        })), &Arc::new(svc.clone()), 10, |_, _, _| panic!("refused profile reached spawn"), || panic!("refused profile reserved a tab")).unwrap_err();
        assert_eq!(error.code, -32602);
        assert_eq!(error.data.unwrap()["code"], code);
        assert_eq!(svc.mux.read().snapshot().tabs.len(), before);
        assert!(svc
            .plan_store
            .list_task_attempts(&plan.id, task)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn socket_respawn_profile_refusal_leaves_no_reservation_attempt_or_tab() {
    let (harness, owner, _) = fixture_with_profiles(Some(json!({
        "selected": {"binary": "node", "args": [], "ownerConfigured": true}
    })));
    let svc = &harness.fixture.service;
    let team = svc
        .mux
        .read()
        .get_tab(&owner)
        .unwrap()
        .team_id
        .clone()
        .unwrap();
    let plan = svc
        .plan_store
        .create_plan_for_team(
            "respawn refusal".into(),
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
    let task = svc
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Profile task".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    svc.plan_store
        .assign_task(&plan.id, &task, "source-worker", None, 3)
        .unwrap();
    let mut approved = svc.plan_store.get(&plan.id).unwrap();
    approved.mode = PlanMode::Approved;
    approved.planning_state = codeterm_service_core::plan::PlanningState::Approved;
    svc.plan_store.save(&mut approved, 4).unwrap();
    let source = svc.plan_store.list_task_attempts(&plan.id, &task).unwrap()[0]
        .attempt_id
        .clone();
    let before =
        serde_json::to_value(svc.plan_store.inspect_task_for_test(&task).unwrap().task).unwrap();
    let tabs = svc.mux.read().snapshot().tabs.len();
    for (profile, code) in [
        ("unknown", "unknown_launch_profile"),
        ("selected", "launch_profile_not_installed"),
    ] {
        let error = crate::server::plan_task_rpc::dispatch(
            "plan.task.respawn",
            &Some(json!({
                "plan_id": plan.id, "task_id": task, "attempt_id": source, "provider": PROVIDER,
                "launch_profile": profile, "caller_tab_id": owner,
                "session_id": "owner-session", "session_binding_nonce": "owner-nonce"
            })),
            &Arc::new(svc.clone()),
            5,
        )
        .unwrap_err();
        assert_eq!(error.code, -32602);
        let data = error.data.unwrap();
        assert_eq!(data["code"], code);
        assert_eq!(data["declared_profile_ids"], json!(["selected"]));
        assert_eq!(
            serde_json::to_value(svc.plan_store.inspect_task_for_test(&task).unwrap().task)
                .unwrap(),
            before
        );
        assert_eq!(svc.mux.read().snapshot().tabs.len(), tabs);
        let reservation = svc
            .plan_store
            .reserve_task_respawn_startup(&plan.id, &task, &source, "probe-reservation")
            .unwrap();
        drop(reservation);
    }
}

#[test]
fn dispatch_profile_creation_preserves_title_and_startup_owner_before_readiness() {
    for profile in [None, Some("selected")] {
        let profiles = profile.map(|_| json!({"selected": {"binary": "node", "args": []}}));
        let (harness, owner, workspace) = fixture_with_profiles(profiles);
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
                "startup profile identity".into(),
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
        plan.planning_state = codeterm_service_core::plan::PlanningState::Approved;
        svc.plan_store.save(&mut plan, 2).unwrap();
        let added = crate::server::plan_task_rpc::dispatch("plan.task.add", &Some(json!({
            "plan_id": plan.id, "name": "Profile startup task", "personality": "Beacon", "executor_role": "Probe",
            "role_guidance": "opaque-role", "body": "opaque-task", "execution_intent": "non_repository",
            "acceptance_criteria": ["opaque-result"], "test_plan": "opaque-verification",
            "caller_tab_id": owner, "session_id": "owner-session", "session_binding_nonce": "owner-nonce"
        })), &Arc::new(svc.clone()), 3).unwrap();
        let task = added["task_id"].as_str().unwrap();
        let inspected = Cell::new(false);
        let error = super::super::dispatch_with_spawn(&Some(json!({
            "plan_id": plan.id, "task_id": task, "agent": PROVIDER, "launch_profile": profile,
            "caller_tab_id": owner, "parent_tab_id": owner, "workspace_id": workspace,
            "session_id": "owner-session", "session_binding_nonce": "owner-nonce", "execution_intent": "read_only"
        })), &Arc::new(svc.clone()), 10, |service, params, _ready_tx| {
            let authority = params.task_launch_authority.as_ref().unwrap();
            let task = service.plan_store.inspect_task_for_test(&authority.task_id).unwrap().task;
            let attempt = task.execution_attempts.iter().find(|attempt| attempt.id == authority.attempt_id).unwrap();
            let title = params.title.as_ref().expect("Worker title must exist before launch");
            assert_eq!(attempt.executor_title.as_ref(), Some(title));
            assert_eq!(attempt.launch_profile.as_deref(), profile);
            assert!(attempt.session_id.is_none());
            assert!(attempt.session_binding_nonce.is_none());
            assert_eq!(task.status_label.as_deref(), Some("starting"));
            let owner = service.plan_store.dispatch_startup_owner_candidate(&authority.plan_id, &authority.task_id).unwrap().expect("profile recording must preserve the startup owner");
            assert_eq!(owner.attempt_id, authority.attempt_id);
            assert_eq!(owner.tab_id, attempt.executor_tab_id);
            assert_eq!(task.lane.workspace_id, None);
            assert!(service.mux.read().get_tab(&owner.tab_id).is_some(), "the pending Worker tab must stay reserved");
            assert_eq!(&service.mux.read().display_label_for_tab(&owner.tab_id), title, "the pending Worker title must stay reserved");
            let pending = service.plan_store.show_task_attempt(&authority.plan_id, &authority.task_id, &authority.attempt_id).unwrap();
            assert_eq!(pending.attempt.executor_title.as_ref(), Some(title));
            assert_eq!(pending.attempt.executor_tab_id, owner.tab_id);
            inspected.set(true);
            Err("profile startup identity inspected before readiness".into())
        }, || {}).unwrap_err();
        assert!(inspected.get());
        assert!(error
            .message
            .contains("profile startup identity inspected before readiness"));
    }
}
