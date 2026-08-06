use super::*;
use crate::plan::{
    ExecutionIntent, InterruptionCheckpoint, OrchestratorRef, Plan, ProviderAllowance,
    TaskLaneMetadata, TaskStatus,
};
use crate::service::plan::interruption::{checkpoint_digest, PROVIDER_EXHAUSTED_LABEL};
use crate::service::plan::{NewTask, PlanStore};

fn allowance(remaining_pct: f32, observed_at: u64) -> ProviderAllowance {
    ProviderAllowance {
        remaining_pct,
        observed_at,
        stale_after_ms: 100,
        reserve_pct: 10.0,
    }
}

/// A PlanStore holding one live, mutating attempt owned by old-tab/old-session
/// at epoch 1 — the exact shape a dispatched worker carries when a provider
/// crosses its cap.
fn bound_store() -> (tempfile::TempDir, PlanStore, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "exhaustion".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "mutate".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let attempt_id = store
        .begin_task_dispatch_with_policy(
            &plan.id,
            &task_id,
            "old-tab",
            "Worker",
            TaskLaneMetadata::default(),
            ExecutionIntent::Mutating,
            Some(allowance(90.0, 3)),
            Some("claude"),
            3,
        )
        .unwrap();
    store
        .finish_task_dispatch(
            &plan.id,
            &task_id,
            "old-tab",
            &attempt_id,
            "old-session",
            "old-nonce",
            TaskLaneMetadata::default(),
            4,
        )
        .unwrap();
    (dir, store, plan.id, task_id, attempt_id)
}

fn synthetic_checkpoint(task_id: &str, attempt_id: &str) -> InterruptionCheckpoint {
    let diff_artifact = "diff --git a/file b/file\n+dirty\n".to_string();
    InterruptionCheckpoint {
        task_id: task_id.into(),
        attempt_id: attempt_id.into(),
        tab_id: "old-tab".into(),
        session_id: "old-session".into(),
        owner_epoch: 1,
        repo_path: "repo".into(),
        worktree_path: "worktree".into(),
        base_head: "base".into(),
        current_head: "head".into(),
        git_status: "1 .M file".into(),
        diff_sha256: checkpoint_digest(&diff_artifact),
        diff_artifact,
        completed_tests: vec![],
        unresolved_work: "resume at reset".into(),
        next_acceptance_gate: "canonical QA".into(),
    }
}

/// PRESERVE: a confirmed exhaustion pauses the live worker without destroying
/// it. The attempt stays OPEN (tab survives), the owner is intact (never
/// epoch-locked to a dead tab), and a resumable checkpoint is sealed.
#[test]
fn provider_exhaustion_pause_preserves_worker_and_seals_checkpoint() {
    let (_dir, store, plan_id, task_id, attempt_id) = bound_store();
    store
        .begin_provider_exhaustion_pause(
            &plan_id,
            &task_id,
            &attempt_id,
            "old-tab",
            "old-session",
            1,
            5,
        )
        .unwrap();
    let sealed = synthetic_checkpoint(&task_id, &attempt_id);
    store
        .finish_provider_exhaustion_pause(&plan_id, &task_id, &attempt_id, 1, sealed.clone(), 6)
        .unwrap();

    let task = store.show_task(&task_id).unwrap().task;
    // Not destroyed / not handed off: attempt open, owner preserved.
    assert!(
        task.execution_attempts[0].closed_at.is_none(),
        "paused attempt must stay open — the tab is preserved, not closed"
    );
    assert!(!task.interruption.handoff_required);
    assert_eq!(task.interruption.owner_epoch, 1);
    assert_eq!(task.interruption.owner_tab_id.as_deref(), Some("old-tab"));
    assert_eq!(
        task.interruption.owner_session_id.as_deref(),
        Some("old-session")
    );
    // Paused surface + resumable checkpoint.
    assert_eq!(task.status, TaskStatus::Paused);
    assert_eq!(task.status_label.as_deref(), Some(PROVIDER_EXHAUSTED_LABEL));
    assert_eq!(task.interruption.checkpoint, Some(sealed));
    assert_eq!(
        task.execution_attempts[0].interruption.reason.as_deref(),
        Some("provider_exhausted")
    );
}

/// SEAM with W3-C: the paused task is re-dispatchable at the SAME epoch — the
/// live worker resumes when quota returns with no epoch bump and no deadlock.
#[test]
fn provider_exhaustion_pause_is_resumable_without_epoch_churn() {
    let (_dir, store, plan_id, task_id, attempt_id) = bound_store();
    store
        .begin_provider_exhaustion_pause(
            &plan_id,
            &task_id,
            &attempt_id,
            "old-tab",
            "old-session",
            1,
            5,
        )
        .unwrap();
    // The same live owner still authenticates — the task is not orphaned.
    assert!(store
        .verify_task_owner_epoch(&task_id, "old-tab", "old-session", 1)
        .is_ok());
    store
        .resume_from_provider_exhaustion(&plan_id, &task_id, "old-tab", "old-session", 1, 7)
        .unwrap();
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.status_label, None);
    // Same epoch, same owner: no new epoch-deadlock introduced.
    assert_eq!(task.interruption.owner_epoch, 1);
    assert!(task.execution_attempts[0].closed_at.is_none());
}

/// GUARD: only ProviderExhausted changes to a pause. The non-exhaustion
/// interruption path still fences into a closed-attempt, handoff-required state.
#[test]
fn non_exhaustion_interruption_path_still_hands_off() {
    let (_dir, store, plan_id, task_id, attempt_id) = bound_store();
    store
        .interrupt_task_attempt(
            &plan_id,
            &task_id,
            &attempt_id,
            "old-tab",
            "old-session",
            1,
            "process_exited",
            synthetic_checkpoint(&task_id, &attempt_id),
            5,
        )
        .unwrap();
    let task = store.show_task(&task_id).unwrap().task;
    assert!(task.interruption.handoff_required);
    assert_eq!(task.status_label.as_deref(), Some("handoff_required"));
    assert!(task.execution_attempts[0].closed_at.is_some());
    assert_ne!(task.status, TaskStatus::Paused);
}

fn run(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success(), "git {}", args.join(" "));
}

#[test]
fn exhaustion_after_dirty_edit_seals_and_verifies_exact_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    run(dir.path(), &["init"]);
    run(
        dir.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    run(dir.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(dir.path().join("tracked.txt"), "base\n").unwrap();
    run(dir.path(), &["add", "tracked.txt"]);
    run(dir.path(), &["commit", "-m", "base"]);
    let base = git(dir.path(), &["rev-parse", "HEAD"]).unwrap();
    std::fs::write(dir.path().join("tracked.txt"), "dirty\n").unwrap();
    std::fs::write(dir.path().join("untracked.bin"), [0_u8, 1, 255]).unwrap();

    let checkpoint = seal_git_checkpoint(CheckpointRequest {
        task_id: "task",
        attempt_id: "attempt",
        tab_id: "tab",
        session_id: "session",
        owner_epoch: 1,
        repo_path: dir.path(),
        worktree_path: dir.path(),
        base_head: base.trim(),
        completed_tests: vec![],
        unresolved_work: "continue".into(),
        next_acceptance_gate: "focused QA".into(),
    })
    .unwrap();

    assert!(checkpoint.git_status.contains("tracked.txt"));
    assert!(checkpoint.git_status.contains("untracked.bin"));
    assert!(checkpoint.diff_artifact.contains("untracked.bin\0"));
    verify_git_checkpoint(&checkpoint).unwrap();
    std::fs::write(dir.path().join("tracked.txt"), "changed again\n").unwrap();
    assert!(verify_git_checkpoint(&checkpoint).is_err());
}

#[test]
fn checkpoint_bound_includes_tracked_diff_and_total_envelope() {
    let dir = tempfile::tempdir().unwrap();
    run(dir.path(), &["init"]);
    run(
        dir.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    run(dir.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(dir.path().join("large.bin"), []).unwrap();
    run(dir.path(), &["add", "large.bin"]);
    run(dir.path(), &["commit", "-m", "base"]);
    let base = git(dir.path(), &["rev-parse", "HEAD"]).unwrap();
    std::fs::write(
        dir.path().join("large.bin"),
        vec![b'x'; CHECKPOINT_ARTIFACT_MAX_BYTES],
    )
    .unwrap();
    let result = seal_git_checkpoint(CheckpointRequest {
        task_id: "task",
        attempt_id: "attempt",
        tab_id: "tab",
        session_id: "session",
        owner_epoch: 1,
        repo_path: dir.path(),
        worktree_path: dir.path(),
        base_head: base.trim(),
        completed_tests: vec![],
        unresolved_work: "continue".into(),
        next_acceptance_gate: "focused QA".into(),
    });
    assert!(result.unwrap_err().contains("4 MiB"));
}

#[test]
fn interruption_events_are_provider_neutral() {
    assert_eq!(
        WorkerInterruptionEvent::ProviderExhausted.reason(),
        "provider_exhausted"
    );
    assert_eq!(
        WorkerInterruptionEvent::ProcessExited.reason(),
        "process_exited"
    );
    assert_eq!(
        WorkerInterruptionEvent::ForcedContextReset.reason(),
        "forced_context_reset"
    );
}

#[test]
fn pending_interruption_fences_duplicate_process_exit_event() {
    let tab_id = "duplicate-tab";
    pending_interruptions()
        .lock()
        .unwrap()
        .insert(tab_id.into());
    assert!(pending_interruptions().lock().unwrap().contains(tab_id));
    assert!(pending_interruptions().lock().unwrap().remove(tab_id));
}

#[cfg_attr(not(windows), ignore = "mock desktop runtime is Windows-owned")]
#[test]
fn production_adoption_binds_exact_authenticated_session_and_epoch() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_session_id = Some("new-session".into());
    tab.session_binding_nonce = Some("nonce".into());
    tab.reported_session_binding_nonce = Some("nonce".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let repo = fixture.temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    run(&repo, &["init"]);
    run(&repo, &["config", "user.email", "fixture@example.invalid"]);
    run(&repo, &["config", "user.name", "Fixture"]);
    std::fs::write(repo.join("file.txt"), "base\n").unwrap();
    run(&repo, &["add", "file.txt"]);
    run(&repo, &["commit", "-m", "base"]);
    let head = git(&repo, &["rev-parse", "HEAD"]).unwrap();
    std::fs::write(repo.join("file.txt"), "dirty\n").unwrap();

    let mut plan = Plan::new(
        "handoff".into(),
        repo.display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "adopt".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let lane = TaskLaneMetadata {
        worktree_path: Some(repo.display().to_string()),
        base: Some(head.trim().into()),
        ..Default::default()
    };
    let attempt_id = fixture
        .service
        .plan_store
        .begin_task_dispatch_with_policy(
            &plan.id,
            &task_id,
            "old-tab",
            "Old",
            lane.clone(),
            ExecutionIntent::Mutating,
            Some(crate::plan::ProviderAllowance {
                remaining_pct: 80.0,
                observed_at: 3,
                stale_after_ms: 100,
                reserve_pct: 5.0,
            }),
            Some("codex"),
            3,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .finish_task_dispatch(
            &plan.id,
            &task_id,
            "old-tab",
            &attempt_id,
            "old-session",
            "old-nonce",
            lane,
            4,
        )
        .unwrap();
    let checkpoint = seal_git_checkpoint(CheckpointRequest {
        task_id: &task_id,
        attempt_id: &attempt_id,
        tab_id: "old-tab",
        session_id: "old-session",
        owner_epoch: 1,
        repo_path: &repo,
        worktree_path: &repo,
        base_head: head.trim(),
        completed_tests: vec![],
        unresolved_work: "continue".into(),
        next_acceptance_gate: "canonical QA".into(),
    })
    .unwrap();
    fixture
        .service
        .plan_store
        .interrupt_task_attempt(
            &plan.id,
            &task_id,
            &attempt_id,
            "old-tab",
            "old-session",
            1,
            "provider_exhausted",
            checkpoint,
            5,
        )
        .unwrap();

    let (epoch, _) = fixture
        .service
        .adopt_worker_checkpoint_sync(&task_id, &tab_id, "new-session", 6)
        .unwrap();
    assert_eq!(epoch, 2);
    assert!(fixture
        .service
        .verify_task_owner_session(&task_id, &tab_id, "stale-session", 2)
        .is_err());
    assert!(fixture
        .service
        .verify_task_owner_session(&task_id, &tab_id, "new-session", 2)
        .is_ok());
}
