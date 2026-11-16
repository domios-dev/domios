use super::*;
use codeterm_service_core::plan::{AgentRef, ExecutionIntent, TaskFiles, WorkerPlacement};
use codeterm_service_core::service::plan::interruption::TaskMutationAuthority;
use codeterm_service_core::service::plan::task_attempts::BoundWorkerReport;

#[test]
fn foreign_cwd_dispatch_persists_launch_checkout_and_worker_report() {
    assert_checkout_binding(true);
}

#[test]
fn canonical_dispatch_keeps_shared_checkout_binding() {
    assert_checkout_binding(false);
}

fn assert_checkout_binding(foreign_cwd: bool) {
    let (_canonical_dir, canonical) = super::super::git_project();
    let (_foreign_dir, foreign) = super::super::git_project();
    std::fs::write(foreign.join("foreign.txt"), "foreign history").unwrap();
    git(&foreign, &["add", "foreign.txt"]);
    git(&foreign, &["commit", "-qm", "foreign"]);
    let foreign_lane = _foreign_dir.path().join("plugin-worktree");
    git(
        &foreign,
        &[
            "worktree",
            "add",
            "-b",
            "lane/plugin",
            foreign_lane.to_str().unwrap(),
        ],
    );
    let checkout = if foreign_cwd {
        &foreign_lane
    } else {
        &canonical
    };
    let canonical_start = git(&canonical, &["rev-parse", "HEAD"]);
    let start = git(checkout, &["rev-parse", "HEAD"]);
    let (harness, owner, workspace) = fixture();
    let svc = &harness.fixture.service;
    let startup = super::super::startup_tasks::StartupTasks::install(svc);
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
            "checkout binding".into(),
            canonical.display().to_string(),
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
                name: "checkout binding".into(),
                personality: Some("Finch".into()),
                acceptance_criteria: vec!["opaque-result".into()],
                test_plan: Some("opaque-verification".into()),
                execution_intent:
                    codeterm_service_core::plan::TaskExecutionIntent::RepositoryMutation,
                files: TaskFiles {
                    edit: vec!["owned.txt".into()],
                    ..Default::default()
                },
                ..Default::default()
            },
            2,
        )
        .unwrap();
    plan = svc.plan_store.get(&plan.id).unwrap();
    plan.mode = PlanMode::Approved;
    plan.planning_state = codeterm_service_core::plan::PlanningState::Approved;
    svc.plan_store.save(&mut plan, 3).unwrap();
    let mut params = json!({
        "plan_id": plan.id, "task_id": task, "agent": PROVIDER,
        "caller_tab_id": owner, "parent_tab_id": owner, "workspace_id": workspace,
        "session_id": "owner-session", "session_binding_nonce": "owner-nonce",
        "execution_intent": "mutating"
    });
    if foreign_cwd {
        params["cwd"] = json!(checkout);
    }
    let response =
        super::super::super::dispatch(&Some(params), &Arc::new(svc.clone()), 10).unwrap();
    startup.finish();
    let worker = response["tab_id"].as_str().unwrap();
    let attempt_id = response["attempt_id"].as_str().unwrap();
    let persisted = svc.plan_store.inspect_task_for_test(&task).unwrap().task;
    let attempt = persisted.execution_attempts.last().unwrap();
    assert_eq!(attempt.id, attempt_id);
    assert_eq!(attempt.execution_intent, Some(ExecutionIntent::Mutating));
    assert_eq!(
        attempt.placement,
        if foreign_cwd {
            WorkerPlacement::IsolatedLane
        } else {
            WorkerPlacement::Shared
        }
    );
    assert_eq!(
        std::fs::canonicalize(attempt.checkout_path.as_deref().unwrap()).unwrap(),
        std::fs::canonicalize(checkout).unwrap()
    );
    assert_eq!(attempt.shared_start_sha.as_deref(), Some(start.as_str()));
    assert_eq!(
        std::fs::canonicalize(attempt.worktree_path.as_deref().unwrap()).unwrap(),
        std::fs::canonicalize(checkout).unwrap()
    );
    svc.plan_store
        .acknowledge_task_attempt(&plan.id, &task, worker, attempt_id, 11)
        .unwrap();
    std::fs::write(checkout.join("owned.txt"), "own change").unwrap();
    git(checkout, &["add", "owned.txt"]);
    git(checkout, &["commit", "-qm", "own change"]);
    let sha = git(checkout, &["rev-parse", "HEAD"]);
    let report = BoundWorkerReport {
        message: "checkout result".into(),
        status: Some(codeterm_types::agent::ReportStatus::InReview),
        sha: Some(sha.clone()),
        authority: Some(TaskMutationAuthority {
            tab_id: worker.into(),
            session_id: attempt.session_id.clone().unwrap(),
            session_binding_nonce: attempt.session_binding_nonce.clone().unwrap(),
            owner_epoch: attempt.interruption.owner_epoch,
        }),
        ..Default::default()
    };
    assert!(
        svc.plan_store
            .ingest_bound_worker_report(&plan.id, &task, &AgentRef::from_tab(worker), report, 12)
            .unwrap()
            .committed_review
    );
    let persisted = svc.plan_store.inspect_task_for_test(&task).unwrap().task;
    assert_eq!(persisted.status, TaskStatus::InReview);
    assert_eq!(
        persisted
            .execution_attempts
            .last()
            .unwrap()
            .report
            .as_ref()
            .unwrap()
            .sha
            .as_deref(),
        Some(sha.as_str())
    );
    assert_eq!(
        git(&canonical, &["rev-parse", "HEAD"]),
        if foreign_cwd { canonical_start } else { sha }
    );
}

fn git(path: &std::path::Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    codeterm_service_core::process_util::no_window_std(&mut command);
    let output = command.current_dir(path).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}
