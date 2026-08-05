use super::{
    apply_dispatch_starting_contract, await_agent_ready, build_brief, build_brief_with_personality,
    checkout_required, complete_dispatch_transaction, dispatch_ready_event,
    dispatch_worker_identity, lane_metadata_for_dispatch, preflight_review_capacity,
    regression_blocked_error, resolve_dispatch_lane, resolve_dispatch_routing,
    rollback_failed_dispatch, task_launch_authority_for_dispatch, unqualified_model_error,
    verify_dispatched_task_binding, worker_ack_timeout_event, DispatchEvidence,
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
    provision_lane_for_test, SpawnIdentityProvenance, SpawnResult, WorkerLaneProvenance,
};
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

mod checkout_occupancy;
mod lane_safety;
mod qa_pass;
mod recovery_race;
mod worker_ack;

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
    let err = regression_blocked_error(gates);
    assert_eq!(err.code, -32010);
    assert!(err.message.contains("phase-1"));
    assert!(err.message.contains("web-ui-serve-discovery"));
    let data = err.data.expect("structured block data");
    assert_eq!(data["type"], "regression_gate_blocked");
    let keys: Vec<&str> = data["blocking_gates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, vec!["phase-1", "web-ui-serve-discovery"]);
}

fn providers() -> Vec<String> {
    vec!["claude".into(), "codex".into(), "opencode".into()]
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
    assert_eq!(response["accepted"], true);
    assert_eq!(response["state"], "awaiting_dispatch_ready");
    assert_eq!(response["readiness"]["ready"], false);
    assert_eq!(response["readiness"]["awaiting_event"], "dispatch_ready");
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
    );
    assert_eq!(event["kind"], "dispatch_ready");
    assert_eq!(event["tab_id"], "tab-worker");
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
            "plan", "task", &tab_id, "attempt", &evidence,
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
        project.to_str().unwrap(),
        &TaskLaneMetadata::default(),
    )
    .unwrap();
    let provenance = resolved.existing.expect("canonical provenance");
    assert_eq!(
        std::fs::canonicalize(&resolved.spawn_cwd).unwrap(),
        std::fs::canonicalize(&project).unwrap()
    );
    let lane = lane_metadata_for_dispatch(&TaskLaneMetadata::default(), &provenance);
    assert!(lane.worktree_path.is_none());
    assert!(lane.branch.is_none());
    assert!(lane.base.is_none());
    assert_eq!(git_worktree_count(&project), 1);
}

#[test]
fn explicit_cwd_conflicting_with_recorded_lane_is_rejected_before_provisioning() {
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
    let error = resolve_dispatch_lane(
        Some(other.to_str().unwrap()),
        project.to_str().unwrap(),
        &lane_metadata,
    )
    .unwrap_err();
    assert_eq!(error.code, -32602);
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
                    Err("launch failed".into())
                } else {
                    Ok(())
                }
            },
            |_| {
                if matches!(
                    failure,
                    "tab_removed" | "missing_session" | "wrong_tab_session"
                ) {
                    Err(failure.into())
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
        |_| Err("spawned tab did not bind a session".into()),
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

#[test]
fn agent_ready_oneshot_error_closed_sender_and_expired_deadline_fail() {
    let (error_tx, error_rx) = tokio::sync::oneshot::channel();
    error_tx
        .send(Err("provider rejected launch".into()))
        .unwrap();
    assert_eq!(
        await_agent_ready(error_rx, Instant::now() + Duration::from_secs(1)).unwrap_err(),
        "provider rejected launch"
    );

    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    drop(closed_tx);
    assert!(
        await_agent_ready(closed_rx, Instant::now() + Duration::from_secs(1))
            .unwrap_err()
            .contains("closed")
    );

    let (_pending_tx, pending_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    assert!(
        await_agent_ready(pending_rx, Instant::now() - Duration::from_secs(1))
            .unwrap_err()
            .contains("expired")
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
        |_| Err("TaskDeliveryFailed: rejected".into()),
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
                    ),
                    4,
                )
                .map_err(|error| error.to_string())
        },
        || {
            rollback_failed_dispatch(
                Some(lane),
                || {
                    store
                        .restore_task_dispatch_snapshot(&plan.id, &original, PlanMode::Approved, 5)
                        .map_err(|error| error.to_string())
                },
                || {
                    closed.set(true);
                    Ok(())
                },
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

#[test]
fn routing_explicit_agent_wins_and_keeps_model() {
    let r = resolve_dispatch_routing(
        Some("codex".into()),
        Some("gpt-5".into()),
        None,
        &providers(),
    );
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model.as_deref(), Some("gpt-5"));
}

#[test]
fn routing_model_naming_a_provider_becomes_that_provider() {
    // This is the exact BUG A repro: `--model codex` must route to the codex
    // provider at its default model, not to the default provider.
    let r = resolve_dispatch_routing(None, Some("codex".into()), None, &providers());
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model, None);
}

#[test]
fn routing_unknown_model_stays_a_model_for_default_provider() {
    let r = resolve_dispatch_routing(None, Some("opus".into()), None, &providers());
    assert_eq!(r.agent, None);
    assert_eq!(r.model.as_deref(), Some("opus"));
}

#[test]
fn routing_qualified_model_selects_its_registered_provider() {
    let r = resolve_dispatch_routing(None, Some("claude:opus".into()), None, &providers());
    assert_eq!(r.agent.as_deref(), Some("claude"));
    assert_eq!(r.model.as_deref(), Some("opus"));

    let r = resolve_dispatch_routing(None, Some("codex:gpt-5.6-terra".into()), None, &providers());
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model.as_deref(), Some("gpt-5.6-terra"));
}

#[test]
fn routing_never_silently_substitutes_an_unqualified_model() {
    let routing = resolve_dispatch_routing(None, Some("opus".into()), None, &providers());
    let notice = crate::service::spawn::ModelFallbackNotice {
        planned_model: Some("opus".into()),
        requested_model: "opus".into(),
        provider_resolved_model: "gpt-5.6-terra".into(),
        actual_model: None,
        reason: "requested_model_unavailable".into(),
    };

    let error = unqualified_model_error(&routing, "codex", &notice).unwrap();
    assert_eq!(error.code, -32602);
    assert_eq!(error.data.unwrap()["type"], "unqualified_model_unavailable");

    let explicit = resolve_dispatch_routing(
        Some("codex".into()),
        Some("opus".into()),
        None,
        &providers(),
    );
    let explicit_error = unqualified_model_error(&explicit, "codex", &notice).unwrap();
    assert_eq!(explicit_error.code, -32602);
    assert_eq!(
        explicit_error.data.unwrap()["type"],
        "unqualified_model_unavailable"
    );

    let qualified = resolve_dispatch_routing(None, Some("claude:opus".into()), None, &providers());
    assert!(unqualified_model_error(&qualified, "claude", &notice).is_none());
}

#[test]
fn routing_falls_back_to_task_model_when_no_rpc_model() {
    let r = resolve_dispatch_routing(None, None, Some("codex".into()), &providers());
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model, None);
}

#[test]
fn routing_rpc_model_overrides_task_model() {
    let r = resolve_dispatch_routing(
        None,
        Some("opencode".into()),
        Some("codex".into()),
        &providers(),
    );
    assert_eq!(r.agent.as_deref(), Some("opencode"));
    assert_eq!(r.model, None);
}

#[test]
fn routing_empty_inputs_use_defaults() {
    let r = resolve_dispatch_routing(Some("  ".into()), Some("".into()), None, &providers());
    assert_eq!(r.agent, None);
    assert_eq!(r.model, None);
}

#[test]
fn brief_opens_with_personality_line_when_set() {
    let t = task(json!({
        "id": "task_p",
        "name": "Wire dispatch RPC",
        "type": "code",
        "status": "pending",
        "personality": "Hopper",
    }));
    let brief = build_brief(&t, &[]);
    assert!(brief.starts_with("You are Hopper — executor for: Wire dispatch RPC"));
    assert!(brief.contains("# Wire dispatch RPC"));
}

#[test]
fn brief_opens_with_role_guidance_before_title_and_body() {
    let t = task(json!({
        "id": "task_rg",
        "name": "Stabilize flicker",
        "type": "code",
        "status": "pending",
        "personality": "Shannon",
        "role_guidance": "You are a React/TypeScript perf engineer. RED LINE: assert the DOM end state.",
        "body": "Fix the bounce in the thread view.",
    }));
    let brief = build_brief(&t, &[]);
    let guidance =
        "You are Shannon. You are a React/TypeScript perf engineer. RED LINE: assert the DOM end state.";
    let guidance_pos = brief.find(guidance).expect("role guidance in brief");
    let title_pos = brief.find("# Stabilize flicker").expect("title in brief");
    let body_pos = brief
        .find("Fix the bounce in the thread view.")
        .expect("body in brief");
    assert_eq!(guidance_pos, 0);
    assert!(guidance_pos < title_pos);
    assert!(guidance_pos < body_pos);
}

#[test]
fn brief_deduplicates_matching_persona_guidance_prefix() {
    let t = task(json!({
        "id": "task_dup",
        "name": "Trace composition",
        "type": "code",
        "status": "pending",
        "personality": "Hopper",
        "role_guidance": "You are Hopper. Keep the role guidance first.",
    }));

    let brief = build_brief(&t, &[]);
    assert!(brief.starts_with(
        "You are Hopper. Keep the role guidance first.\n\
         Your assigned task is: Trace composition"
    ));
    assert!(brief.contains("# Trace composition"));
    assert!(!brief.contains("You are Hopper. You are Hopper."));
}

#[test]
fn brief_uses_claimed_personality_once_without_mutating_routing_metadata() {
    let t = task(json!({
        "id": "task_claimed",
        "name": "Trace composition",
        "type": "code",
        "status": "pending",
        "personality": "Suggested Hopper",
        "executor_role": "rust-backend-dev",
        "role_guidance": "You are Grace. Keep the role guidance first.",
    }));

    let brief = build_brief_with_personality(&t, &[], Some("Grace"));
    assert!(brief.starts_with(
        "You are Grace. Keep the role guidance first.\n\
         Executor role: rust-backend-dev.\n\
         Your assigned task is: Trace composition"
    ));
    assert_eq!(brief.matches("You are Grace.").count(), 1);
    assert!(!brief.contains("Suggested Hopper"));
    assert!(brief.contains("Executor role: rust-backend-dev."));
}

#[test]
fn test_author_brief_carries_charter_reference_before_task() {
    let t = task(json!({
        "id": "task_test_author",
        "name": "Author spawn-readiness drill",
        "type": "test",
        "status": "pending",
        "executor_role": "test-author",
        "role_guidance": crate::prompts::TEST_AUTHOR_GUIDANCE,
        "body": "Write the drill that catches a readiness race.",
    }));
    let brief = build_brief(&t, &[]);
    let guidance_pos = brief
        .find(crate::prompts::TEST_AUTHOR_GUIDANCE)
        .expect("canonical test-author guidance in brief");
    let title_pos = brief
        .find("# Author spawn-readiness drill")
        .expect("title in brief");
    let body_pos = brief
        .find("Write the drill that catches a readiness race.")
        .expect("body in brief");

    assert_eq!(guidance_pos, 0);
    assert!(brief.contains("docs/contributing/test-authoring.md"));
    assert!(guidance_pos < title_pos);
    assert!(guidance_pos < body_pos);
}

#[test]
fn brief_supplies_task_owned_identity_when_personality_is_absent() {
    let t = task(json!({
        "id": "task_np",
        "name": "Bare",
        "type": "code",
        "status": "pending",
    }));
    let brief = build_brief(&t, &[]);
    assert!(brief.starts_with("You are the Worker responsible for: Bare"));
    assert!(brief.contains("# Bare"));
}

#[test]
fn dispatch_preserves_only_explicit_user_personality() {
    let t = task(json!({
        "id": "task_user_persona",
        "name": "Dispatch",
        "type": "code",
        "status": "pending",
        "personality": "Ada",
        "personality_provenance": IdentityProvenance::UserSupplied,
    }));
    let (title, provenance) = dispatch_worker_identity(&t);
    assert_eq!(title.as_deref(), Some("Ada"));
    assert_eq!(provenance, Some(SpawnIdentityProvenance::UserSupplied));
}

#[test]
fn dispatch_treats_planner_personality_as_provisional() {
    let t = task(json!({
        "id": "task_suggested_persona",
        "name": "Dispatch",
        "type": "code",
        "status": "pending",
        "personality": "Ada",
        "personality_provenance": IdentityProvenance::Suggested,
    }));
    let (title, provenance) = dispatch_worker_identity(&t);
    assert_eq!(title.as_deref(), Some("Ada"));
    assert_eq!(provenance, Some(SpawnIdentityProvenance::Suggested));
}

#[test]
fn dispatch_treats_batch_personality_without_legacy_provenance_as_provisional() {
    let t = task(json!({
        "id": "task_batch_persona",
        "name": "Dispatch",
        "type": "code",
        "status": "pending",
        "personality": "Lamport",
    }));
    let (title, provenance) = dispatch_worker_identity(&t);
    assert_eq!(title.as_deref(), Some("Lamport"));
    assert_eq!(provenance, Some(SpawnIdentityProvenance::Suggested));
}

fn task(value: serde_json::Value) -> Task {
    serde_json::from_value(value).expect("task fixture")
}

#[test]
fn socket_dispatch_requires_daemon_owned_capacity_provenance() {
    let mut plan = Plan::new(
        "review debt".into(),
        "/repo".into(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.review_policy.credit = -(plan.review_policy.wip_limit as i64);
    let review = task(json!({
        "id": "review",
        "name": "review",
        "type": "implement",
        "status": "pending",
        "tags": ["review-capacity"],
    }));
    let implementation = task(json!({
        "id": "implementation",
        "name": "implementation",
        "type": "implement",
        "status": "pending",
    }));

    assert!(matches!(
        preflight_review_capacity(&plan, &review),
        Err(crate::service::plan::PlanError::ReviewWipExceeded { .. })
    ));
    plan.review_policy
        .capacity_grants
        .push(crate::plan::ReviewCapacityGrant {
            task_id: review.id.clone(),
            class: crate::plan::ReviewCapacityClass::Review,
            granted_by: crate::plan::AgentRef::from_tab("orch"),
            granted_at: 1,
        });
    preflight_review_capacity(&plan, &review).unwrap();
    assert!(matches!(
        preflight_review_capacity(&plan, &implementation),
        Err(crate::service::plan::PlanError::ReviewWipExceeded { .. })
    ));
}

#[test]
fn brief_includes_body_acceptance_and_task_authority() {
    let t = task(json!({
        "id": "task_1",
        "name": "Wire dispatch RPC",
        "type": "code",
        "status": "pending",
        "body": "Compose spawn, assign, send.",
        "acceptance_criteria": ["atomic", "returns Tab id"],
    }));
    let brief = build_brief(&t, &[]);
    assert!(brief.contains("# Wire dispatch RPC"));
    assert!(brief.contains("Compose spawn, assign, send."));
    assert!(brief.contains("- atomic"));
    assert!(brief.contains("- returns Tab id"));
    assert!(brief.starts_with("You are the Worker responsible for: Wire dispatch RPC"));
    assert!(!brief.contains("codeterm agent report"));
    assert!(!brief.contains("--base "));
}

#[test]
fn worker_prompt_places_ack_before_assignment_work() {
    let t = task(json!({
        "id": "task_prompt_order",
        "name": "Repair prompt order",
        "type": "code",
        "status": "pending",
        "personality": "Hopper",
        "executor_role": "rust-backend-dev",
        "role_guidance": "Audit the final provider prompt.",
        "body": "Keep the task body complete.",
        "intent": "Prefer one shared composition path.",
        "acceptance_criteria": ["identity first"],
        "correctness": "No provider sees the transport marker.",
        "test_plan": "cargo test worker_prompt"
    }));
    let brief = build_brief(&t, &[]);
    let params = crate::service::SpawnAgentParams {
        role: Some("Worker".into()),
        task: Some(brief),
        ..Default::default()
    };
    let prompt = crate::service::spawn_onboarding_task(&params).unwrap();

    let identity = prompt.find("You are Hopper.").unwrap();
    let context = prompt
        .find(crate::prompts::WORKER_CODETERM_CONTEXT.trim())
        .unwrap();
    let body = prompt.find("Keep the task body complete.").unwrap();
    let intent = prompt.find("## Intent").unwrap();
    let correctness = prompt.find("## Correctness").unwrap();
    let ack = prompt.find("codeterm agent ack").unwrap();
    let report = prompt
        .find("codeterm agent report --status in_review")
        .unwrap();
    let fallback = prompt
        .find("load the provider skill named `codeterm`")
        .unwrap();

    assert!(identity < context && context < ack && ack < body);
    assert!(body < intent && intent < correctness);
    assert!(correctness < report && report < fallback);
    assert_eq!(prompt.matches("codeterm agent ack").count(), 1);
    assert!(!prompt.contains(crate::service::plan::worker_identity::WORKER_IDENTITY_END));
}

#[test]
fn brief_lists_section_links_and_test_plan() {
    let t = task(json!({
        "id": "task_2",
        "name": "T",
        "type": "code",
        "status": "pending",
        "test_plan": "cargo test dispatch",
        "qa_plan": "cargo test independent acceptance",
    }));
    let sections = vec![TaskSection {
        id: "sec_9".into(),
        title: "Phase 1".into(),
    }];
    let brief = build_brief(&t, &sections);
    assert!(brief.contains("## Test plan"));
    assert!(brief.contains("cargo test dispatch"));
    assert!(!brief.contains("independent acceptance"));
    assert!(!brief.contains("qa_plan"));
    assert!(brief.contains("## Sections"));
    assert!(brief.contains("Phase 1 (sec_9)"));
}
#[test]
fn brief_omits_empty_optional_sections() {
    let t = task(json!({
        "id": "task_3",
        "name": "Bare",
        "type": "code",
        "status": "pending",
    }));
    let brief = build_brief(&t, &[]);
    assert!(!brief.contains("## Acceptance criteria"));
    assert!(!brief.contains("## Test plan"));
    assert!(!brief.contains("## Sections"));
    assert!(!brief.contains("## Strides"));
    assert!(!brief.contains("codeterm agent report"));
}
