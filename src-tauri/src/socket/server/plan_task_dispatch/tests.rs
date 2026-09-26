use super::dispatch_failure::dispatch_wait_failed;
use super::workspace_admission::{admit_dispatch_workspace, DISPATCH_WORKSPACE_CLOSING_CODE};
use super::{
    apply_dispatch_starting_contract, await_agent_ready, build_brief, build_brief_with_manifest,
    build_brief_with_personality, checkout_required, cleanup_succeeded,
    complete_dispatch_transaction, dispatch_authorized_gates, dispatch_ready_event,
    dispatch_startup_failed_event, dispatch_worker_identity, lane_metadata_for_dispatch,
    preflight_dispatch_task_status, preflight_review_capacity, regression_blocked_error,
    resolve_dispatch_lane, roll_back_dispatch_attempt, rollback_failed_dispatch,
    set_dispatch_task_brief, startup_wait_is_recoverable, task_launch_authority_for_dispatch,
    verify_dispatched_task_binding, worker_ack_timeout_event, DispatchEvidence,
    DispatchFailureDisposition, DispatchWaitFailure, DISPATCH_READY_TIMEOUT,
};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{
    IdentityProvenance, OrchestratorRef, Plan, PlanMode, Task, TaskLaneMetadata, TaskStatus,
};
use crate::pty::manager::PtyManager;
use crate::service::plan::milestone_gates::RegressionGate;
use crate::service::plan::task_queries::TaskSection;
use crate::service::plan::{NewTask, PlanStore};
use crate::service::{
    provision_lane_for_test, LaneProtection, SpawnIdentityProvenance, SpawnResult,
    WorkerLaneProvenance,
};
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

mod brief_tests;
mod canonical_writer;
mod checkout_occupancy;
mod concurrent_dispatch;
mod dispatch_placement;
mod dispatch_recovery;
mod lane_persist;
mod lane_provisioning;
mod lane_resolution_multirepo;
mod lane_safety;
mod late_attempt_failure;
mod qa_pass;
mod recovery_race;
mod routing;
mod worker_ack;

#[test]
fn plan_task_dispatch_refuses_unknown_model_before_spawn() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    let (mut mux, _root) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let before = mux.snapshot().tabs.len();

    let refused = crate::service::spawn::validate_agent_model(&registry, "claude", Some("opuss"))
        .map_err(|error| error.to_string())
        .and_then(|_| {
            let clearance = crate::service::spawn::provider_launch_preflight(&registry, "claude")
                .map_err(|error| error.to_string())?;
            crate::service::spawn::create_cleared_agent_tab(&mut mux, &workspace_id, &clearance)
                .map_err(|error| error.to_string())
        });
    let error = refused.expect_err("dispatch must refuse before reserving a Worker Tab");
    assert!(error.contains("opuss"), "{error}");
    assert!(error.contains("opus"), "{error}");
    assert_eq!(mux.snapshot().tabs.len(), before);
}

#[test]
fn nonrepo_team_runs_noncode_workers_without_git_probe() {
    let provenance_called = Cell::new(false);
    let evidence = complete_dispatch_transaction(
        Instant::now() + Duration::from_secs(1),
        |_| Ok(()),
        |_| Ok("session-nonrepo".into()),
        || {
            provenance_called.set(true);
            Ok(None)
        },
        |lane, _| {
            assert!(lane.is_none());
            Ok(())
        },
        || panic!("successful non-repository dispatch must not roll back"),
    )
    .unwrap();
    assert!(provenance_called.get());
    assert!(evidence.lane.is_none());
}

#[test]
fn dispatch_effort_becomes_host_owned_plan_intent() {
    let params = json!({ "reasoning_effort": "high" });
    let (planned, requested) = super::dispatch_reasoning_effort(Some(&params));
    assert_eq!(planned.as_deref(), Some("high"));
    assert_eq!(requested.as_deref(), Some("high"));
}

#[test]
fn dispatched_attempt_records_routed_model_and_effort() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = Plan::new(
        "attempt routing".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "unplanned model".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let task = store.show_task(&task_id).unwrap().task;
    assert!(task.model.is_none());
    let routing = super::resolve_dispatch_routing(
        None,
        Some("codex:gpt-6-luna".into()),
        task.model,
        &["claude".into(), "codex".into()],
    );
    let attempt_id = store
        .begin_task_dispatch_with_reserved_attempt_and_gates_and_observation(
            &plan.id,
            &task_id,
            "worker-tab",
            "Worker",
            TaskLaneMetadata::default(),
            crate::plan::ExecutionIntent::ReadOnly,
            None,
            routing.agent.as_deref(),
            "attempt-routed-model",
            Some(vec![]),
            crate::plan::WorkerPlacement::IsolatedLane,
            routing.model,
            Some("xhigh".into()),
            None,
            false,
            true,
            3,
        )
        .unwrap();

    let task = store.show_task(&task_id).unwrap().task;
    let attempt = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert_eq!(attempt.provider.as_deref(), Some("codex"));
    assert_eq!(attempt.model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(attempt.reasoning_effort.as_deref(), Some("xhigh"));

    store
        .update_dispatched_attempt_reasoning_effort(
            &plan.id,
            &task_id,
            "worker-tab",
            &attempt_id,
            Some("high".into()),
            4,
        )
        .unwrap();
    let task = store.show_task(&task_id).unwrap().task;
    let attempt = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    assert_eq!(attempt.reasoning_effort.as_deref(), Some("high"));

    let updated_at = store.get(&plan.id).unwrap().updated_at;
    store
        .update_dispatched_attempt_reasoning_effort(
            &plan.id,
            &task_id,
            "worker-tab",
            &attempt_id,
            Some("high".into()),
            5,
        )
        .unwrap();
    assert_eq!(store.get(&plan.id).unwrap().updated_at, updated_at);
}

#[test]
fn dispatch_gate_tokens_are_preserved_as_opaque_values() {
    let params = json!({
        "authorized_gates": ["gate-token-alpha", "gate-token-beta"]
    });
    assert_eq!(
        dispatch_authorized_gates(Some(&params)).unwrap(),
        vec!["gate-token-alpha", "gate-token-beta"]
    );
    assert!(dispatch_authorized_gates(None).unwrap().is_empty());
}

#[test]
fn repository_mutation_in_nonrepo_fails_checkout_required_before_spawn() {
    let error = checkout_required("ordinary folder is not a checkout");
    assert_eq!(error.code, -32012);
    assert_eq!(error.data.unwrap()["type"], "checkout_required");
}

#[test]
fn qa_git_oracles_are_skipped_for_nonrepository_tasks() {
    let task = task(json!({
        "id": "task_nonrepo",
        "name": "research",
        "type": "task",
        "status": "pending",
        "execution_intent": "non_repository"
    }));
    assert!(!task.execution_intent.requires_repository());
    assert!(task.lane.worktree_path.is_none());
    assert!(task.lane.base.is_none());
}

#[test]
fn repository_lane_guards_remain_required_for_code_mutation() {
    let task = task(json!({
        "id": "task_repo",
        "name": "code",
        "type": "code",
        "status": "pending"
    }));
    assert!(task.execution_intent.requires_repository());
}

#[test]
fn paused_task_dispatch_refuses_before_spawn_without_agent_resume_instruction() {
    let paused = task(json!({
        "id": "task_paused",
        "name": "waiting for user",
        "type": "task",
        "status": "paused"
    }));

    let error = preflight_dispatch_task_status("plan_1", &paused).unwrap_err();
    let data = error.data.expect("structured paused-task refusal");

    assert_eq!(error.code, -32602);
    assert_eq!(data["stage"], "task_status");
    assert_eq!(data["hold_kind"], "unclassified");
    assert!(data.get("fix").is_none());
}

#[test]
fn regression_block_error_names_every_gate_in_message_and_data() {
    let gates = vec![
        RegressionGate {
            key: "phase-1".into(),
            label: "Phase 1".into(),
        },
        RegressionGate {
            key: "web-ui-serve-discovery".into(),
            label: "Web UI serve discovery".into(),
        },
    ];
    let retrospectives = vec![RegressionGate {
        key: "phase-2".into(),
        label: "Phase 2".into(),
    }];
    let err = regression_blocked_error(gates, retrospectives);
    assert_eq!(err.code, -32010);
    assert!(err.message.contains("phase-1"));
    assert!(err.message.contains("web-ui-serve-discovery"));
    assert!(err.message.contains("phase-2"));
    let data = err.data.expect("structured block data");
    assert_eq!(data["type"], "regression_gate_blocked");
    assert_eq!(data["pending_retrospectives"][0]["key"], "phase-2");
    let keys: Vec<&str> = data["blocking_gates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, vec!["phase-1", "web-ui-serve-discovery"]);
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_project() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "fixture"]);
    (root, project)
}

fn provenance() -> WorkerLaneProvenance {
    WorkerLaneProvenance {
        canonical_cwd: "/project".into(),
        worktree_path: "/project-worktrees/lane".into(),
        branch: "lane/test".into(),
        base: "master".into(),
    }
}

fn spawn_result(tab_id: &str) -> SpawnResult {
    SpawnResult {
        tab_id: tab_id.into(),
        pty_id: String::new(),
        agent: "codex".into(),
        model: None,
        planned_model: None,
        requested_model: None,
        provider_resolved_model: None,
        actual_model: None,
        reasoning_effort: None,
        reasoning_effort_provenance: Default::default(),
        title: "worker".into(),
        description: None,
        role: Some("Worker".into()),
        role_profile: None,
        warning: None,
        model_fallback: None,
        budget_notice: None,
    }
}

fn starting_response(tab_id: &str) -> serde_json::Value {
    let mut response = json!({"ok": true});
    apply_dispatch_starting_contract(&mut response, tab_id, &spawn_result(tab_id));
    response
}

#[test]
fn dispatched_binding_verification_is_exact_and_loud() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan_for_team(
            "Binding".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .set_task_assignment(&task_id, Some("tab-worker"), None, 3)
        .unwrap();

    verify_dispatched_task_binding(&store, &plan.id, &task_id, "tab-worker").unwrap();
    let error =
        verify_dispatched_task_binding(&store, &plan.id, &task_id, "tab-unbound").unwrap_err();
    assert!(error.contains("ORCHESTRATION PRODUCT INCIDENT"));
    assert!(error.contains("Impact:"));
    assert!(error.contains("Fallback:"));
    assert!(error.contains("no plan or task was guessed"));
}

fn collect_tab_and_tab_ids(value: &serde_json::Value, ids: &mut HashSet<String>) {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if key == "tab_id" {
                    if let Some(id) = value.as_str() {
                        ids.insert(id.to_string());
                    }
                }
                collect_tab_and_tab_ids(value, ids);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_tab_and_tab_ids(value, ids);
            }
        }
        _ => {}
    }
}

struct DispatchRuntime {
    mux: MultiplexerState,
    pty: PtyManager,
    workspace_id: String,
    parent_tab_id: String,
    baseline_tab_count: usize,
}

impl DispatchRuntime {
    fn new() -> Self {
        let (mut mux, _) = MultiplexerState::new();
        let workspace = mux.list_workspaces().into_iter().next().unwrap();
        let parent_tab_id = workspace.tab_ids.first().unwrap().clone();
        mux.set_orchestrator_role(&parent_tab_id).unwrap();
        Self {
            baseline_tab_count: workspace.tab_count,
            workspace_id: workspace.id,
            parent_tab_id,
            mux,
            pty: PtyManager::new(),
        }
    }

    fn create_worker(&mut self) -> (String, String) {
        let tab_id = self.mux.create_tab(&self.workspace_id).unwrap();
        self.mux
            .set_tab_role(&tab_id, AgentRole::Worker, Some(&self.parent_tab_id))
            .unwrap();
        let (pty_id, _) = self
            .pty
            .spawn(
                None,
                Some(std::env::current_dir().unwrap().display().to_string()),
                80,
                24,
                HashMap::new(),
            )
            .unwrap();
        self.mux.link_pty(&tab_id, pty_id.clone());
        self.mux.get_tab_mut(&tab_id).unwrap().agent_session_id = Some("session-worker".into());
        (tab_id, pty_id)
    }

    fn close_worker(&mut self, tab_id: &str) -> Result<(), String> {
        for pty_id in self.mux.close_tab(tab_id)? {
            self.pty.kill(&pty_id)?;
        }
        Ok(())
    }

    fn tab_count(&self) -> usize {
        self.mux
            .list_workspaces()
            .into_iter()
            .find(|workspace| workspace.id == self.workspace_id)
            .unwrap()
            .tab_count
    }
}

#[test]
fn dispatch_response_has_single_authoritative_tab_id() {
    for authoritative_tab_id in ["d3d39cd2", "b46ee19a", "5f3decc7"] {
        let response = starting_response(authoritative_tab_id);
        let mut ids = HashSet::new();
        collect_tab_and_tab_ids(&response, &mut ids);
        assert_eq!(
            ids,
            HashSet::from([authoritative_tab_id.to_string()]),
            "response contained an identity other than {authoritative_tab_id}"
        );
    }
}

#[test]
fn dispatch_response_never_claims_ready_before_event() {
    let response = starting_response("tab-worker");
    assert_eq!(response["accepted"], false);
    assert_eq!(response["spawned"], true);
    assert_eq!(response["state"], "awaiting_worker_ack");
    assert_eq!(response["readiness"]["ready"], false);
    assert_eq!(response["readiness"]["awaiting_event"], "worker_ack");
    assert_eq!(response["spawn"]["pty_id"], "");
    assert_ne!(response["kind"], "dispatch_ready");

    let event = dispatch_ready_event(
        "plan",
        "task",
        "tab-worker",
        "attempt",
        &DispatchEvidence {
            session_id: "session-worker".into(),
            lane: Some(provenance()),
        },
        "/canonical/project",
        &[],
    );
    assert_eq!(event["kind"], "dispatch_ready");
    assert_eq!(event["tab_id"], "tab-worker");
}

/// The immediate response is the observable contract the Orchestrator reads: a
/// spawned Worker that has not bound is never reported as an accepted dispatch.
#[test]
fn spawned_unbound_dispatch_is_reported_as_awaiting_not_accepted() {
    let response = starting_response("tab-codex");
    assert_eq!(
        response["accepted"], false,
        "an unbound spawn is not accepted"
    );
    assert_eq!(response["spawned"], true);
    assert_eq!(response["state"], "awaiting_worker_ack");
    assert_eq!(response["readiness"]["awaiting_event"], "worker_ack");
}

#[test]
fn session_binding_timeout_is_recoverable_but_definitive_binding_is_not() {
    let late = dispatch_wait_failed(
        "session_binding",
        DispatchWaitFailure::Uncertain(
            "spawned tab 'tab-worker' did not bind a session before the deadline".into(),
        ),
    );
    assert!(startup_wait_is_recoverable(&late));

    let removed = dispatch_wait_failed(
        "session_binding",
        DispatchWaitFailure::Definitive(
            "spawned tab 'tab-worker' was removed before session binding".into(),
        ),
    );
    assert!(!startup_wait_is_recoverable(&removed));
}

#[test]
fn uncertain_agent_ready_timeout_is_recoverable() {
    let ready = dispatch_wait_failed(
        "agent_ready",
        DispatchWaitFailure::Uncertain("agent readiness deadline expired".into()),
    );
    assert!(startup_wait_is_recoverable(&ready));
}

#[test]
fn one_dispatch_creates_exactly_one_tab() {
    let runtime = RefCell::new(DispatchRuntime::new());
    let (tab_id, pty_id) = runtime.borrow_mut().create_worker();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    ready_tx.send(Ok(())).unwrap();

    let evidence = complete_dispatch_transaction(
        Instant::now() + Duration::from_secs(1),
        |deadline| await_agent_ready(ready_rx, deadline),
        |_| {
            Ok(runtime
                .borrow()
                .mux
                .get_tab(&tab_id)
                .and_then(|tab| tab.agent_session_id.clone())
                .expect("session record"))
        },
        || Ok(Some(provenance())),
        |_, session_id| {
            assert_eq!(session_id, "session-worker");
            Ok(())
        },
        || panic!("successful dispatch must not roll back"),
    )
    .unwrap();
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    event_tx
        .send(dispatch_ready_event(
            "plan",
            "task",
            &tab_id,
            "attempt",
            &evidence,
            "/canonical/project",
            &[],
        ))
        .unwrap();
    let ready_event = event_rx.recv().unwrap();

    assert_eq!(ready_event["kind"], "dispatch_ready");
    let runtime_ref = runtime.borrow();
    assert_eq!(runtime_ref.tab_count(), runtime_ref.baseline_tab_count + 1);
    assert_eq!(
        runtime_ref.mux.find_workspace_for_tab(&tab_id),
        Some(runtime_ref.workspace_id.clone())
    );
    let worker = runtime_ref.mux.get_tab(&tab_id).unwrap();
    let parent = runtime_ref.mux.get_tab(&runtime_ref.parent_tab_id).unwrap();
    assert!(parent.team_id.is_some());
    assert_eq!(worker.team_id, parent.team_id);
    assert_eq!(worker.pty_id.as_deref(), Some(pty_id.as_str()));
    assert_eq!(worker.agent_session_id.as_deref(), Some("session-worker"));
    assert!(runtime_ref.pty.get_session(&pty_id).is_some());
    let mut response_ids = HashSet::new();
    collect_tab_and_tab_ids(&starting_response(&tab_id), &mut response_ids);
    assert_eq!(response_ids, HashSet::from([tab_id.clone()]));
    drop(runtime_ref);
    runtime.borrow_mut().close_worker(&tab_id).unwrap();
}

#[test]
fn dispatch_failure_leaves_no_orphan() {
    let runtime = RefCell::new(DispatchRuntime::new());
    let (tab_id, pty_id) = runtime.borrow_mut().create_worker();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    ready_tx
        .send(Err("injected startup failure".into()))
        .unwrap();

    let error = complete_dispatch_transaction(
        Instant::now() + Duration::from_secs(1),
        |deadline| await_agent_ready(ready_rx, deadline),
        |_| Ok("session-worker".into()),
        || Ok(Some(provenance())),
        |_, _| Ok(()),
        || {
            rollback_failed_dispatch(
                None,
                || Ok(()),
                || runtime.borrow_mut().close_worker(&tab_id),
            )
        },
    )
    .unwrap_err();

    assert_eq!(error.data.unwrap()["stage"], "agent_ready");
    let runtime = runtime.borrow();
    assert_eq!(runtime.tab_count(), runtime.baseline_tab_count);
    assert!(runtime.mux.get_tab(&tab_id).is_none());
    assert!(runtime.pty.get_session(&pty_id).is_none());
}

#[test]
fn explicit_canonical_cwd_is_an_orchestrator_checkout_choice() {
    let (_root, project) = git_project();
    let resolved = resolve_dispatch_lane(
        Some(project.to_str().unwrap()),
        Some(project.to_str().unwrap()),
        None,
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap();
    let provenance = resolved.existing.expect("canonical provenance");
    assert_eq!(
        std::fs::canonicalize(&resolved.spawn_cwd).unwrap(),
        std::fs::canonicalize(&project).unwrap()
    );
    let lane = lane_metadata_for_dispatch(
        &TaskLaneMetadata::default(),
        &provenance,
        project.to_str().unwrap(),
    );
    assert!(lane.worktree_path.is_none());
    assert!(lane.branch.is_none());
    assert!(lane.base.is_none());
    assert_eq!(git_worktree_count(&project), 1);
}

#[test]
fn recorded_lane_is_preferred_over_a_conflicting_explicit_cwd() {
    let (_root, project) = git_project();
    let lane = provision_lane_for_test(project.to_str().unwrap(), "recorded").unwrap();
    let recorded = lane.verify().unwrap();
    let other = project.parent().unwrap().join("other");
    std::fs::create_dir_all(&other).unwrap();
    let lane_metadata = TaskLaneMetadata {
        worktree_path: Some(recorded.worktree_path.clone()),
        branch: Some(recorded.branch.clone()),
        base: Some(recorded.base.clone()),
        ..Default::default()
    };
    let resolved = resolve_dispatch_lane(
        Some(other.to_str().unwrap()),
        Some(project.to_str().unwrap()),
        None,
        &lane_metadata,
        LaneProtection::Enforced,
    )
    .unwrap();
    assert_eq!(resolved.spawn_cwd, recorded.worktree_path);
    assert_eq!(git_worktree_count(&project), 2);
    lane.cleanup().unwrap();
}

fn git_worktree_count(project: &Path) -> usize {
    let output = Command::new("git")
        .current_dir(project)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| line.starts_with("worktree "))
        .count()
}

#[test]
fn dispatch_rollback_closes_tab_and_removes_provisioned_lane() {
    let (_root, project) = git_project();

    let lane = provision_lane_for_test(project.to_str().unwrap(), "dispatch_failure").unwrap();
    let lane_path = std::path::PathBuf::from(&lane.cwd);
    let branch = Command::new("git")
        .current_dir(&lane_path)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    let branch = String::from_utf8(branch.stdout).unwrap().trim().to_string();
    let mut task_reverted = false;
    let mut tab_closed = false;

    rollback_failed_dispatch(
        Some(lane),
        || {
            task_reverted = true;
            Ok(())
        },
        || {
            tab_closed = true;
            Ok(())
        },
    )
    .unwrap();

    assert!(task_reverted);
    assert!(tab_closed);
    assert!(!lane_path.exists());
    let branch_exists = Command::new("git")
        .current_dir(&project)
        .args(["branch", "--list", &branch])
        .output()
        .unwrap();
    assert!(String::from_utf8(branch_exists.stdout)
        .unwrap()
        .trim()
        .is_empty());
}

#[test]
fn launch_delivery_readiness_and_same_tab_binding_precede_commit() {
    let order = RefCell::new(Vec::new());
    let evidence = complete_dispatch_transaction(
        Instant::now() + Duration::from_secs(1),
        |_| {
            order.borrow_mut().push("agent_ready");
            Ok(())
        },
        |_| {
            order.borrow_mut().push("same_tab_session");
            Ok("session-worker".into())
        },
        || {
            order.borrow_mut().push("provenance");
            Ok(Some(provenance()))
        },
        |_, session| {
            assert_eq!(session, "session-worker");
            order.borrow_mut().push("commit");
            Ok(())
        },
        || {
            order.borrow_mut().push("rollback");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(evidence.session_id, "session-worker");
    assert_eq!(
        order.into_inner(),
        vec!["agent_ready", "same_tab_session", "provenance", "commit"]
    );
}

#[test]
fn every_transaction_stage_failure_runs_compensation() {
    for (failure, expected_stage) in [
        ("agent_ready", "agent_ready"),
        ("tab_removed", "session_binding"),
        ("missing_session", "session_binding"),
        ("wrong_tab_session", "session_binding"),
        ("provenance", "provenance_readback"),
        ("task_commit", "task_commit"),
    ] {
        let rolled_back = Cell::new(false);
        let error = complete_dispatch_transaction(
            Instant::now() + Duration::from_secs(1),
            |_| {
                if failure == "agent_ready" {
                    Err(DispatchWaitFailure::Definitive("launch failed".into()))
                } else {
                    Ok(())
                }
            },
            |_| {
                if matches!(
                    failure,
                    "tab_removed" | "missing_session" | "wrong_tab_session"
                ) {
                    Err(DispatchWaitFailure::Definitive(failure.into()))
                } else {
                    Ok("session-worker".into())
                }
            },
            || {
                if failure == "provenance" {
                    Err("readback mismatch".into())
                } else {
                    Ok(Some(provenance()))
                }
            },
            |_, _| {
                if failure == "task_commit" {
                    Err("mutation failed".into())
                } else {
                    Ok(())
                }
            },
            || {
                rolled_back.set(true);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.data.unwrap()["stage"], expected_stage, "{failure}");
        assert!(rolled_back.get(), "{failure}");
    }
}

#[test]
fn binding_failure_never_false_starts() {
    let committed = Cell::new(false);
    let rolled_back = Cell::new(false);
    let error = complete_dispatch_transaction(
        Instant::now() + Duration::from_secs(1),
        |_| Ok(()),
        |_| {
            Err(DispatchWaitFailure::Definitive(
                "spawned tab did not bind a session".into(),
            ))
        },
        || Ok(Some(provenance())),
        |_, _| {
            committed.set(true);
            Ok(())
        },
        || {
            rolled_back.set(true);
            Ok(())
        },
    )
    .unwrap_err();

    assert_eq!(error.data.unwrap()["stage"], "session_binding");
    assert!(!committed.get());
    assert!(rolled_back.get());
}

/// rel#66: a dispatch whose readiness evidence is merely absent — the ready
/// deadline elapsed with no signal either way — is not a dispatch that
/// failed. The worker may still be starting, so the lane and tab must
/// survive. Fails on unmodified `complete_dispatch_transaction`, which rolls
/// back on any stage error regardless of what the error means.
#[test]
fn agent_ready_timeout_never_rolls_back_a_possibly_live_worker() {
    let error = complete_dispatch_transaction(
        Instant::now() + Duration::from_secs(1),
        |_| {
            Err(DispatchWaitFailure::Uncertain(
                "agent readiness deadline expired".into(),
            ))
        },
        |_| Ok("session-worker".into()),
        || Ok(Some(provenance())),
        |_, _| Ok(()),
        || panic!("readiness evidence being absent must never roll back a possibly-live worker"),
    )
    .unwrap_err();

    let data = error.data.unwrap();
    assert_eq!(data["stage"], "agent_ready");
    assert_eq!(data["worker_may_be_alive"], true);
}

/// Same rel#66 guarantee at the session-binding stage: the tab is still
/// present, only its session hasn't bound yet, so the worker may still be
/// alive under it.
#[test]
fn session_binding_timeout_never_rolls_back_a_possibly_live_worker() {
    let error = complete_dispatch_transaction(
        Instant::now() + Duration::from_secs(1),
        |_| Ok(()),
        |_| {
            Err(DispatchWaitFailure::Uncertain(
                "spawned tab did not bind a session before the deadline".into(),
            ))
        },
        || Ok(Some(provenance())),
        |_, _| Ok(()),
        || panic!("readiness evidence being absent must never roll back a possibly-live worker"),
    )
    .unwrap_err();

    let data = error.data.unwrap();
    assert_eq!(data["stage"], "session_binding");
    assert_eq!(data["worker_may_be_alive"], true);
}

#[test]
fn dispatch_binding_wait_uses_the_measured_startup_deadline() {
    assert_eq!(
        DISPATCH_READY_TIMEOUT,
        crate::service::worker_ack::STARTUP_BINDING_DEADLINE
    );
}

#[test]
fn survivor_report_omits_cleanup_and_names_the_live_tab_state() {
    let event = dispatch_startup_failed_event(
        "plan",
        "task",
        "tab",
        "attempt",
        "session_binding",
        "binding deadline expired",
        DispatchFailureDisposition::Survivor {
            attempt_finalized: true,
        },
    );

    assert_eq!(
        event["tab_state"],
        crate::service::worker_ack::TAB_STATE_ALIVE_AWAITING_INSPECTION
    );
    assert_eq!(
        event["attempt_state"],
        crate::service::worker_ack::ATTEMPT_STATE_FINALIZED
    );
    assert!(event.get("cleanup").is_none());
    assert!(event.get("cleanup_error").is_none());
}

#[test]
fn definitive_dispatch_failure_keeps_truthful_cleanup_result() {
    let event = dispatch_startup_failed_event(
        "plan",
        "task",
        "tab",
        "attempt",
        "task_commit",
        "commit rejected",
        DispatchFailureDisposition::Cleanup { ok: true },
    );

    assert_eq!(event["cleanup"]["ok"], true);
    assert!(event.get("tab_state").is_none());
}

#[test]
fn workspace_close_only_cleanup_failure_is_satisfied() {
    let close_only = dispatch_startup_failed_event(
        "plan",
        "task",
        "tab",
        "attempt",
        "compensation",
        "Tab close failed: Workspace is closing: workspace",
        DispatchFailureDisposition::Cleanup {
            ok: cleanup_succeeded(
                "compensation",
                "Tab close failed: Workspace is closing: workspace",
            ),
        },
    );
    assert_eq!(close_only["cleanup"]["ok"], true);

    let mixed_failure = dispatch_startup_failed_event(
        "plan",
        "task",
        "tab",
        "attempt",
        "compensation",
        "Tab close failed: Workspace is closing: workspace; lane cleanup failed: disk",
        DispatchFailureDisposition::Cleanup {
            ok: cleanup_succeeded(
                "compensation",
                "Tab close failed: Workspace is closing: workspace; lane cleanup failed: disk",
            ),
        },
    );
    assert_eq!(mixed_failure["cleanup"]["ok"], false);
}

#[test]
fn closing_workspace_admission_refuses_before_spawn_seam() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let _reservation = mux.reserve_workspace_close(&workspace_id).unwrap();
    let spawned = Cell::new(false);

    let result = admit_dispatch_workspace(&workspace_id, |id| {
        mux.ensure_workspace_membership_mutable(id)
    });
    if result.is_ok() {
        spawned.set(true);
    }
    let error = result.unwrap_err();

    assert_eq!(error.code, DISPATCH_WORKSPACE_CLOSING_CODE);
    assert_eq!(error.data.unwrap()["type"], "dispatch_workspace_closing");
    assert!(!spawned.get());
}

#[test]
fn healthy_workspace_admission_reaches_spawn_seam() {
    let (mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let spawned = Cell::new(false);

    let result = admit_dispatch_workspace(&workspace_id, |id| {
        mux.ensure_workspace_membership_mutable(id)
    });
    if result.is_ok() {
        spawned.set(true);
    }
    result.unwrap();

    assert!(spawned.get());
}

/// Companion to the two tests above: a dispatch that DEFINITIVELY failed
/// (the worker explicitly signaled failure) must still roll back. Without
/// this, "never roll back a possibly-live worker" would be satisfied by
/// simply never rolling back at all.
#[test]
fn agent_ready_definitive_failure_still_rolls_back_the_lane() {
    let rolled_back = Cell::new(false);
    let error = complete_dispatch_transaction(
        Instant::now() + Duration::from_secs(1),
        |_| {
            Err(DispatchWaitFailure::Definitive(
                "provider rejected launch".into(),
            ))
        },
        |_| Ok("session-worker".into()),
        || Ok(Some(provenance())),
        |_, _| Ok(()),
        || {
            rolled_back.set(true);
            Ok(())
        },
    )
    .unwrap_err();

    assert_eq!(error.data.unwrap()["stage"], "agent_ready");
    assert!(rolled_back.get());
}

#[test]
fn agent_ready_explicit_error_is_definitive_channel_close_and_timeout_are_uncertain() {
    let (error_tx, error_rx) = tokio::sync::oneshot::channel();
    error_tx
        .send(Err("provider rejected launch".into()))
        .unwrap();
    let explicit =
        await_agent_ready(error_rx, Instant::now() + Duration::from_secs(1)).unwrap_err();
    assert_eq!(explicit.message(), "provider rejected launch");
    assert!(
        matches!(explicit, DispatchWaitFailure::Definitive(_)),
        "an explicit launch error must be definitive"
    );

    // A closed channel after generation supersession is absent evidence, not proof the worker failed.
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    drop(closed_tx);
    let closed = await_agent_ready(closed_rx, Instant::now() + Duration::from_secs(1)).unwrap_err();
    assert!(closed.message().contains("closed"));
    assert!(
        matches!(closed, DispatchWaitFailure::Uncertain(_)),
        "a closed channel must be uncertain, never definitive"
    );

    let (_pending_tx, pending_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let expired =
        await_agent_ready(pending_rx, Instant::now() - Duration::from_secs(1)).unwrap_err();
    assert!(expired.message().contains("expired"));
    assert!(
        matches!(expired, DispatchWaitFailure::Uncertain(_)),
        "a readiness deadline expiring must be uncertain, never definitive"
    );
}

#[test]
fn launch_delivery_failure_restores_exact_original_task_and_attempt_state() {
    let (root, project) = git_project();
    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "Rollback".into(),
            project.display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let mut approved = store.get(&plan.id).unwrap();
    approved.mode = PlanMode::Approved;
    store.save(&mut approved, 2).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker".into(),
                lane: TaskLaneMetadata {
                    destination: Some("release".into()),
                    workspace_id: Some("workspace".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let original = store.show_task(&task_id).unwrap().task;
    let lane = provision_lane_for_test(project.to_str().unwrap(), "rollback").unwrap();
    let actual = lane.verify().unwrap();
    let worktree_path = actual.worktree_path.clone();
    let branch = actual.branch.clone();
    let closed = Cell::new(false);
    let error = complete_dispatch_transaction(
        Instant::now() + Duration::from_secs(1),
        |_| {
            Err(DispatchWaitFailure::Definitive(
                "TaskDeliveryFailed: rejected".into(),
            ))
        },
        |_| Ok("session-worker".into()),
        || Ok(Some(actual.clone())),
        |provenance, session| {
            store
                .commit_task_dispatch(
                    &plan.id,
                    &task_id,
                    "tab-worker",
                    "Ada",
                    session,
                    lane_metadata_for_dispatch(
                        &original.lane,
                        provenance.as_ref().expect("repository provenance"),
                        project.to_str().unwrap(),
                    ),
                    4,
                )
                .map_err(|error| error.to_string())
        },
        || {
            roll_back_dispatch_attempt(
                &store,
                &plan.id,
                &original,
                &PlanMode::Approved,
                "tab-worker",
                "attempt-unrecorded",
                Some(lane),
                || {
                    closed.set(true);
                    Ok(())
                },
                5,
            )
        },
    )
    .unwrap_err();

    assert_eq!(error.data.unwrap()["stage"], "agent_ready");
    assert!(closed.get());
    assert_eq!(store.show_task(&task_id).unwrap().task, original);
    assert_eq!(store.get(&plan.id).unwrap().mode, PlanMode::Approved);
    assert!(!Path::new(&worktree_path).exists());
    let branches = Command::new("git")
        .current_dir(&project)
        .args(["branch", "--list", &branch])
        .output()
        .unwrap();
    assert!(String::from_utf8(branches.stdout)
        .unwrap()
        .trim()
        .is_empty());
}

fn task(value: serde_json::Value) -> Task {
    serde_json::from_value(value).expect("task fixture")
}
