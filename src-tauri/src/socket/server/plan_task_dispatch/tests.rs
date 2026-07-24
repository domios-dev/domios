use super::{
    await_agent_ready, build_brief, build_brief_with_claimed_persona, build_brief_with_personality,
    complete_dispatch_transaction, dispatch_worker_identity, lane_metadata_for_dispatch,
    regression_blocked_error, resolve_dispatch_lane, resolve_dispatch_routing,
    rollback_failed_dispatch, unqualified_model_error,
};
use crate::plan::{
    IdentityProvenance, OrchestratorRef, PlanMode, Task, TaskLaneMetadata, TaskStatus,
};
use crate::service::plan::milestone_gates::RegressionGate;
use crate::service::plan::task_queries::TaskSection;
use crate::service::plan::{NewTask, PlanStore};
use crate::service::{provision_lane_for_test, SpawnIdentityProvenance, WorkerLaneProvenance};
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

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

#[test]
fn explicit_canonical_cwd_is_rejected_before_provisioning() {
    let (_root, project) = git_project();
    let provision_count = Cell::new(0);
    let error = resolve_dispatch_lane(
        Some(project.to_str().unwrap()),
        project.to_str().unwrap(),
        &TaskLaneMetadata::default(),
    )
    .unwrap_err();
    if error.code == 0 {
        provision_count.set(provision_count.get() + 1);
    }
    assert_eq!(error.code, -32602);
    assert_eq!(error.data.unwrap()["stage"], "cwd_validation");
    assert_eq!(provision_count.get(), 0);
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
fn dispatch_rollback_closes_pane_and_removes_provisioned_lane() {
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
    let mut pane_closed = false;

    rollback_failed_dispatch(
        Some(lane),
        || {
            task_reverted = true;
            Ok(())
        },
        || {
            pane_closed = true;
            Ok(())
        },
    )
    .unwrap();

    assert!(task_reverted);
    assert!(pane_closed);
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
fn omitted_cwd_provisions_one_lane_and_persists_matching_attempt_provenance() {
    let (root, project) = git_project();
    let resolved = resolve_dispatch_lane(
        None,
        project.to_str().unwrap(),
        &TaskLaneMetadata::default(),
    )
    .unwrap();
    assert!(resolved.existing.is_none());
    let provision_count = Cell::new(0);
    provision_count.set(provision_count.get() + 1);
    let lane = provision_lane_for_test(&resolved.spawn_cwd, "dispatch_success").unwrap();
    let actual = lane.verify().unwrap();
    assert_eq!(provision_count.get(), 1);
    assert_eq!(git_worktree_count(&project), 2);

    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "Dispatch".into(),
            project.display().to_string(),
            OrchestratorRef::from_pane("orch"),
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
                    workspace_id: Some("workspace".into()),
                    destination: Some("master".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let original = store.show_task(&task_id).unwrap().task;
    store
        .commit_task_dispatch(
            &plan.id,
            &task_id,
            "pane-worker",
            "Ada",
            "session-worker",
            lane_metadata_for_dispatch(&original.lane, &actual),
            4,
        )
        .unwrap();

    let task = store.show_task(&task_id).unwrap().task;
    let attempt = task.execution_attempts.last().unwrap();
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(
        task.lane.worktree_path.as_deref(),
        Some(actual.worktree_path.as_str())
    );
    assert_eq!(task.lane.branch.as_deref(), Some(actual.branch.as_str()));
    assert_eq!(attempt.worktree_path, task.lane.worktree_path);
    assert_eq!(attempt.branch, task.lane.branch);
    assert_eq!(attempt.base, task.lane.base);
    assert_eq!(attempt.destination, task.lane.destination);
    assert_eq!(attempt.session_id.as_deref(), Some("session-worker"));
    assert_eq!(task.lane.workspace_id.as_deref(), Some("workspace"));
    lane.cleanup().unwrap();
}

#[test]
fn readiness_and_same_pane_binding_precede_commit_and_delivery() {
    let order = RefCell::new(Vec::new());
    let evidence = complete_dispatch_transaction(
        Instant::now() + Duration::from_secs(1),
        |_| {
            order.borrow_mut().push("agent_ready");
            Ok(())
        },
        |_| {
            order.borrow_mut().push("same_pane_session");
            Ok("session-worker".into())
        },
        || {
            order.borrow_mut().push("provenance");
            Ok(provenance())
        },
        |_, session| {
            assert_eq!(session, "session-worker");
            order.borrow_mut().push("commit");
            Ok(())
        },
        || {
            order.borrow_mut().push("delivery");
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
        vec![
            "agent_ready",
            "same_pane_session",
            "provenance",
            "commit",
            "delivery"
        ]
    );
}

#[test]
fn every_transaction_stage_failure_runs_compensation() {
    for (failure, expected_stage) in [
        ("agent_ready", "agent_ready"),
        ("pane_removed", "session_binding"),
        ("missing_session", "session_binding"),
        ("wrong_pane_session", "session_binding"),
        ("provenance", "provenance_readback"),
        ("task_commit", "task_commit"),
        ("brief_delivery", "brief_delivery"),
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
                    "pane_removed" | "missing_session" | "wrong_pane_session"
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
                    Ok(provenance())
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
                if failure == "brief_delivery" {
                    Err("delivery failed".into())
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
fn post_ready_failure_restores_exact_original_task_and_attempt_state() {
    let (root, project) = git_project();
    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "Rollback".into(),
            project.display().to_string(),
            OrchestratorRef::from_pane("orch"),
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
        |_| Ok(()),
        |_| Ok("session-worker".into()),
        || Ok(actual.clone()),
        |provenance, session| {
            store
                .commit_task_dispatch(
                    &plan.id,
                    &task_id,
                    "pane-worker",
                    "Ada",
                    session,
                    lane_metadata_for_dispatch(&original.lane, provenance),
                    4,
                )
                .map_err(|error| error.to_string())
        },
        || Err("delivery rejected".into()),
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

    assert_eq!(error.data.unwrap()["stage"], "brief_delivery");
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

    let r = resolve_dispatch_routing(None, Some("codex/gpt-5.6-terra".into()), None, &providers());
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
    let expected = "You are Hopper. Keep the role guidance first.\n\n# Trace composition\n\n## When done\nRun: codeterm agent report \"STATUS: done | blocked | partial\\nCHANGES: ...\\nTESTS RUN: ...\"\n";
    assert_eq!(brief, expected);
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
    let expected = "You are Grace. Keep the role guidance first.\n\n# Trace composition\n\n## When done\nRun: codeterm agent report \"STATUS: done | blocked | partial\\nCHANGES: ...\\nTESTS RUN: ...\"\n";
    assert_eq!(brief, expected);
    assert_eq!(brief.matches("You are Grace.").count(), 1);
    assert!(!brief.contains("Suggested Hopper"));
    assert!(!brief.contains("rust-backend-dev"));
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
fn brief_omits_personality_line_when_absent() {
    let t = task(json!({
        "id": "task_np",
        "name": "Bare",
        "type": "code",
        "status": "pending",
    }));
    let brief = build_brief(&t, &[]);
    let expected = "# Bare\n\n## When done\nRun: codeterm agent report \"STATUS: done | blocked | partial\\nCHANGES: ...\\nTESTS RUN: ...\"\n";
    assert_eq!(brief, expected);
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

#[test]
fn dispatch_brief_uses_atomically_claimed_persona_after_reroll() {
    let t = task(json!({
        "id": "task_batch_collision",
        "name": "Dispatch",
        "type": "code",
        "status": "pending",
        "personality": "Lamport",
        "personality_provenance": IdentityProvenance::Suggested,
        "role_guidance": "You are a concurrency engineer.",
    }));

    let brief = build_brief_with_claimed_persona(&t, &[], "Ada");

    assert!(brief.starts_with("You are Ada. You are a concurrency engineer."));
    assert!(!brief.contains("You are Lamport"));
}

fn task(value: serde_json::Value) -> Task {
    serde_json::from_value(value).expect("task fixture")
}

#[test]
fn brief_includes_body_acceptance_and_report_command() {
    let t = task(json!({
        "id": "task_1",
        "name": "Wire dispatch RPC",
        "type": "code",
        "status": "pending",
        "body": "Compose spawn, assign, send.",
        "acceptance_criteria": ["atomic", "returns pane id"],
    }));
    let brief = build_brief(&t, &[]);
    assert!(brief.contains("# Wire dispatch RPC"));
    assert!(brief.contains("Compose spawn, assign, send."));
    assert!(brief.contains("- atomic"));
    assert!(brief.contains("- returns pane id"));
    // The report command is mandatory so the worker can report back.
    assert!(brief.contains("codeterm agent report"));
    assert!(!brief.contains("--base "));
}

#[test]
fn brief_lists_section_links_and_test_plan() {
    let t = task(json!({
        "id": "task_2",
        "name": "T",
        "type": "code",
        "status": "pending",
        "test_plan": "cargo test dispatch",
    }));
    let sections = vec![TaskSection {
        id: "sec_9".into(),
        title: "Phase 1".into(),
    }];
    let brief = build_brief(&t, &sections);
    assert!(brief.contains("## Test plan"));
    assert!(brief.contains("cargo test dispatch"));
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
    assert!(brief.contains("codeterm agent report"));
}
