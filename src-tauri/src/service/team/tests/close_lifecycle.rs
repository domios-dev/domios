use super::*;
use crate::multiplexer::types::TabBacking;
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn ordinary_close_preserves_team_but_explicit_close_removes_it() {
    let (mut mux, team_id, orchestrator, planner, worker) = team_members();
    let (_plan_dir, plan_store) = isolated_plan_store();
    mux.close_tab(&orchestrator).unwrap();
    mux.close_tab(&planner).unwrap();
    mux.close_tab(&worker).unwrap();
    assert!(mux.get_team(&team_id).is_some());
    let events = crate::api::events::EventBroadcaster::new();
    close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::Human,
        1,
    )
    .unwrap();
    assert!(mux.get_team(&team_id).is_none());
}

#[test]
fn close_team_unknown_and_repeat_are_structured_errors() {
    let (mut mux, team_id, orchestrator, _, _) = team_members();
    let (_plan_dir, plan_store) = isolated_plan_store();
    let events = crate::api::events::EventBroadcaster::new();
    assert!(close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        "team_missing",
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        1,
    )
    .unwrap_err()
    .contains("not found"));
    close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        1,
    )
    .unwrap();
    assert!(close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        1,
    )
    .unwrap_err()
    .contains("not found"));
}

/// rel#63: a Team that lost every member (e.g. each left individually) must
/// still be closeable. The old member-authority check required the caller's
/// tab to currently BE a team member — a precondition a zero-member Team can
/// never satisfy again, deadlocking the close forever. The caller's tab stays
/// alive here (unlike a closed-tab caller, which fails for the unrelated
/// "caller tab not found" reason) — it is simply no longer on the team.
#[test]
fn zero_member_team_closes_and_cleans_up_crons() {
    let (mut mux, team_id, orchestrator, planner, worker) = team_members();
    let (_plan_dir, plan_store) = isolated_plan_store();
    for tab_id in [&orchestrator, &planner, &worker] {
        mux.get_tab_mut(tab_id).unwrap().team_id = None;
    }
    assert!(
        mux.team_member_tab_ids(&team_id).is_empty(),
        "fixture must produce a genuinely zero-member Team"
    );
    assert!(mux.get_team(&team_id).is_some());

    let events = crate::api::events::EventBroadcaster::new();
    let (closed, cleanup, _) = close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        1,
    )
    .expect("a zero-member Team must be closeable by a caller no longer on it");

    assert_eq!(closed.id, team_id);
    assert!(mux.get_team(&team_id).is_none());
    assert!(cleanup.member_tab_ids.is_empty());
    // The orchestrator tab itself was never a team member at close time, so it
    // is untouched by the close (it is not team cleanup's to remove).
    assert!(mux.get_tab(&orchestrator).is_some());
}

#[test]
fn close_team_removes_chat_backend_member_and_registry() {
    let (mut mux, team_id, orchestrator, _, worker) = team_members();
    let (_plan_dir, plan_store) = isolated_plan_store();
    mux.get_tab_mut(&worker).unwrap().backing = TabBacking::ChatBackend {
        plugin_id: "chat-backend".into(),
        session_id: "session-1".into(),
    };
    let events = crate::api::events::EventBroadcaster::new();

    let (_, cleanup, _) = close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator.clone()),
        1,
    )
    .unwrap();

    assert!(mux.get_team(&team_id).is_none());
    assert!(mux.get_tab(&worker).is_none());
    assert_eq!(
        cleanup.chat_backings,
        vec![("chat-backend".into(), "session-1".into())]
    );
}

#[test]
fn team_close_retains_provider_ids_for_launch_artifact_release() {
    let (mut mux, team_id, orchestrator, _, worker) = team_members();
    let (_plan_dir, plan_store) = isolated_plan_store();
    mux.get_tab_mut(&worker).unwrap().agent_type = Some("codex".into());
    let events = crate::api::events::EventBroadcaster::new();

    let (_, cleanup, _) = close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator),
        1,
    )
    .unwrap();

    assert_eq!(cleanup.agent_providers, vec![(worker, "codex".into())]);
}

#[test]
fn close_team_detaches_cartographer_manager_with_every_member_tab() {
    let (mut mux, team_id, orchestrator, _planner, _worker) = team_members();
    let (_plan_dir, plan_store) = isolated_plan_store();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let cartographer = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role_kind(
        &cartographer,
        AgentRole::Manager,
        Some(codeterm_types::agent::role_kind::builtins::CARTOGRAPHER),
        Some(&orchestrator),
    )
    .unwrap();
    mux.get_tab_mut(&cartographer).unwrap().agent_type = Some("provider-neutral".into());
    let events = crate::api::events::EventBroadcaster::new();

    let (_, cleanup, _) = close_team_with_state_change(
        &mut mux,
        &events,
        &plan_store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator),
        1,
    )
    .expect("Team close removes the attached Cartographer");

    assert!(mux.get_team(&team_id).is_none());
    assert!(mux.get_tab(&cartographer).is_none());
    assert!(cleanup.member_tab_ids.contains(&cartographer));
    assert!(mux
        .snapshot()
        .tabs
        .values()
        .all(|tab| tab.team_id.as_deref() != Some(team_id.as_str())));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn team_close_collects_members_even_after_role_change() {
    let (mux, team_id, orchestrator, planner, worker) = team_members();
    let fixture = ServiceFixture::new(mux);
    fixture.service.clear_tab_role(&planner).unwrap();
    let mut mux = fixture.service.mux.write();
    assert_eq!(mux.get_tab(&planner).unwrap().team_id, None);
    let remaining_members = mux.team_member_tab_ids(&team_id);
    assert_eq!(remaining_members.len(), 2);
    assert!(remaining_members.contains(&orchestrator));
    assert!(remaining_members.contains(&worker));

    let events = crate::api::events::EventBroadcaster::new();
    let (_, cleanup, _) = close_team_with_state_change(
        &mut mux,
        &events,
        fixture.service.plan_store.as_ref(),
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator),
        1,
    )
    .expect("Team close collects every remaining member after an explicit role clear");

    assert!(!cleanup.member_tab_ids.contains(&planner));
    assert!(cleanup.member_tab_ids.contains(&worker));
    assert!(mux.get_tab(&planner).is_some());
    assert!(mux.get_tab(&worker).is_none());
    assert!(mux.get_team(&team_id).is_none());
    assert!(mux
        .all_tabs()
        .all(|tab| tab.team_id.as_deref() != Some(team_id.as_str())));
}

#[test]
fn team_close_finalizes_attempts_and_returns_tasks_to_pending() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, team_id, orchestrator, _, worker) = team_members();
    let plan = store
        .create_plan_for_team(
            "Team close".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&orchestrator),
            team_id.clone(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "owned task".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["finished".into()],
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let orphan_task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "orphaned task".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["returned to queue".into()],
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, &worker, None, 3)
        .unwrap();
    let review_task_id = add_team_lane_task(
        &store,
        &plan.id,
        "reviewable task",
        &worker,
        &dir.path().join("review-lane"),
        "lane/review",
        "missing-base",
        4,
    );
    let mut plan = store.get(&plan.id).unwrap();
    {
        let task = plan
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap();
        task.status = crate::plan::TaskStatus::InProgress;
        let mut second_attempt = task.execution_attempts[0].clone();
        second_attempt.id = "attempt_second".into();
        task.execution_attempts.push(second_attempt);
    }
    let review_before = {
        let task = plan
            .tasks
            .iter_mut()
            .find(|task| task.id == review_task_id)
            .unwrap();
        task.status = crate::plan::TaskStatus::InReview;
        task.review_stage = crate::plan::ReviewStage::Qa;
        task.qa_state = Some(crate::plan::QaState::Queued);
        task.review_round = 3;
        task.review_oracle = Some("review oracle".into());
        task.status_label = Some("review remains open".into());
        task.lane.worktree_path = Some(dir.path().join("review-lane").display().to_string());
        task.lane.branch = Some("lane/review".into());
        task.lane.base = Some("missing-base".into());
        task.lane.cleanup_status = Some("review-retained".into());
        task.lane.cleanup_hints = vec!["retain for review".into()];
        task.clone()
    };
    plan.tasks
        .iter_mut()
        .find(|task| task.id == orphan_task_id)
        .unwrap()
        .status = crate::plan::TaskStatus::InProgress;
    store.save(&mut plan, 6).unwrap();
    let events = crate::api::events::EventBroadcaster::new();

    close_team_with_state_change(
        &mut mux,
        &events,
        &store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator),
        7,
    )
    .unwrap();

    let closed = store.get(&plan.id).unwrap();
    let task = closed.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.status, crate::plan::TaskStatus::Pending);
    assert_eq!(task.assigned_tab_id, None);
    assert_eq!(task.execution_attempts.len(), 2);
    assert!(task.execution_attempts.iter().all(|attempt| {
        attempt.closed_at == Some(7) && attempt.closure_reason.as_deref() == Some("team_closed")
    }));
    assert!(task
        .milestones
        .iter()
        .any(|milestone| { milestone.kind == "team_close" && milestone.text.contains(&team_id) }));
    assert!(closed
        .strides
        .iter()
        .any(|stride| { stride.kind == "team_close" && stride.text.contains(&team_id) }));
    assert_eq!(
        closed
            .tasks
            .iter()
            .find(|task| task.id == orphan_task_id)
            .unwrap()
            .status,
        crate::plan::TaskStatus::Pending
    );
    assert!(closed
        .tasks
        .iter()
        .all(|task| task.status != crate::plan::TaskStatus::InProgress));
    let review = closed
        .tasks
        .iter()
        .find(|task| task.id == review_task_id)
        .unwrap();
    assert_eq!(review.status, crate::plan::TaskStatus::InReview);
    assert_eq!(review.review_stage, review_before.review_stage);
    assert_eq!(review.qa_state, review_before.qa_state);
    assert_eq!(review.review_round, review_before.review_round);
    assert_eq!(review.review_oracle, review_before.review_oracle);
    assert_eq!(review.status_label, review_before.status_label);
    assert_eq!(review.lane, review_before.lane);
    assert_eq!(review.assigned_tab_id, review_before.assigned_tab_id);
    assert_eq!(review.execution_attempts.len(), 1);
    assert_eq!(review.execution_attempts[0].closed_at, Some(7));
    assert_eq!(
        review.execution_attempts[0].closure_reason.as_deref(),
        Some("team_closed")
    );
}

#[test]
fn team_close_does_not_rewrite_done_task_lane_for_a_closed_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, team_id, orchestrator, _, worker) = team_members();
    let plan = store
        .create_plan_for_team(
            "Historical lane".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&orchestrator),
            team_id.clone(),
            vec![],
            1,
        )
        .unwrap();
    let closed_lane = dir.path().join("historical-lane");
    let task_id = add_team_lane_task(
        &store,
        &plan.id,
        "completed task",
        &worker,
        &closed_lane,
        "lane/completed",
        "main",
        2,
    );
    let mut plan = store.get(&plan.id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = crate::plan::TaskStatus::Done;
    task.assigned_tab_id = None;
    task.lane.tab_id = None;
    task.lane.cleanup_status = Some("previous_cleanup_debt".into());
    task.lane.cleanup_hints = vec!["preserve historical lane record".into()];
    task.execution_attempts[0].closed_at = Some(3);
    task.execution_attempts[0].retired_at = Some(3);
    task.execution_attempts[0].closure_reason = Some("completed".into());
    let lane_before = task.lane.clone();
    store.save(&mut plan, 4).unwrap();

    let events = crate::api::events::EventBroadcaster::new();
    close_team_with_state_change(
        &mut mux,
        &events,
        &store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator),
        5,
    )
    .unwrap();

    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.status, crate::plan::TaskStatus::Done);
    assert_eq!(task.lane, lane_before);
}

#[test]
fn team_close_removes_clean_lanes_and_records_debt_for_lanes_with_work() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init"]);
    git(&repo, &["config", "user.name", "CodeTerm Tests"]);
    git(&repo, &["config", "user.email", "tests@codeterm.invalid"]);
    fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(&repo, &["add", "base.txt"]);
    git(&repo, &["commit", "-m", "base"]);
    git(&repo, &["branch", "-M", "main"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let clean_lane = dir.path().join("clean-lane");
    let work_lane = dir.path().join("work-lane");
    let dirty_lane = dir.path().join("dirty-lane");
    let retained_lane = dir.path().join("retained-lane");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "lane/clean",
            clean_lane.to_str().unwrap(),
            &base,
        ],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "release/retained",
            retained_lane.to_str().unwrap(),
            &base,
        ],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "lane/work",
            work_lane.to_str().unwrap(),
            &base,
        ],
    );
    git(
        &repo,
        &["worktree", "add", "-b", "lane/dirty", dirty_lane.to_str().unwrap(), &base],
    );
    fs::write(work_lane.join("work.txt"), "work\n").unwrap();
    git(&work_lane, &["add", "work.txt"]);
    git(&work_lane, &["commit", "-m", "work"]);
    fs::write(dirty_lane.join("uncommitted.txt"), "keep\n").unwrap();

    let store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (mut mux, team_id, orchestrator, _, worker) = team_members();
    let plan = store
        .create_plan_for_team(
            "Team lanes".into(),
            repo.display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&orchestrator),
            team_id.clone(),
            vec![],
            1,
        )
        .unwrap();
    let clean_task = add_team_lane_task(
        &store,
        &plan.id,
        "clean lane",
        &worker,
        &clean_lane,
        "lane/clean",
        &base,
        2,
    );
    let work_task = add_team_lane_task(
        &store,
        &plan.id,
        "lane with work",
        &worker,
        &work_lane,
        "lane/work",
        &base,
        3,
    );
    let dirty_task = add_team_lane_task(
        &store,
        &plan.id,
        "dirty lane",
        &worker,
        &dirty_lane,
        "lane/dirty",
        &base,
        4,
    );
    let orphan_task = add_team_lane_task(
        &store,
        &plan.id,
        "lane branch without worktree",
        &worker,
        &dir.path().join("missing-lane"),
        "lane/orphan",
        &base,
        5,
    );
    git(&repo, &["branch", "lane/orphan", &base]);
    let mut plan_state = store.get(&plan.id).unwrap();
    let clean = plan_state.tasks.iter_mut().find(|task| task.id == clean_task).unwrap();
    clean.status = crate::plan::TaskStatus::InProgress;
    let unplanned = plan_state.tasks.iter_mut().find(|task| task.id == work_task).unwrap();
    unplanned.kind = "unplanned".into();
    store.save(&mut plan_state, 5).unwrap();
    let retained_task = add_team_lane_task(
        &store,
        &plan.id,
        "non-lane branch",
        &worker,
        &retained_lane,
        "release/retained",
        &base,
        6,
    );
    let events = crate::api::events::EventBroadcaster::new();

    close_team_with_state_change(
        &mut mux,
        &events,
        &store,
        &team_id,
        &TeamMutationCaller::AgentTab(orchestrator),
        5,
    )
    .unwrap();

    let worktrees = git(&repo, &["worktree", "list", "--porcelain"]);
    let normalized_worktrees = worktrees.replace('\\', "/");
    let clean_lane_path = clean_lane.display().to_string().replace('\\', "/");
    let work_lane_path = work_lane.display().to_string().replace('\\', "/");
    let retained_lane_path = retained_lane.display().to_string().replace('\\', "/");
    let dirty_lane_path = dirty_lane.display().to_string().replace('\\', "/");
    assert!(!normalized_worktrees.contains(&clean_lane_path));
    assert!(normalized_worktrees.contains(&work_lane_path));
    assert!(normalized_worktrees.contains(&dirty_lane_path));
    assert!(normalized_worktrees.contains(&retained_lane_path));
    assert!(!git_status_success(
        &repo,
        &["show-ref", "--verify", "--quiet", "refs/heads/lane/clean"]
    ));
    assert!(!git_status_success(
        &repo,
        &["show-ref", "--verify", "--quiet", "refs/heads/lane/orphan"]
    ));
    assert!(git_status_success(
        &repo,
        &["show-ref", "--verify", "--quiet", "refs/heads/lane/work"]
    ));
    assert!(git_status_success(
        &repo,
        &["show-ref", "--verify", "--quiet", "refs/heads/lane/dirty"]
    ));
    assert!(git_status_success(
        &repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            "refs/heads/release/retained"
        ]
    ));

    let closed = store.get(&plan.id).unwrap();
    let clean = closed
        .tasks
        .iter()
        .find(|task| task.id == clean_task)
        .unwrap();
    assert_eq!(clean.lane.worktree_path, None);
    assert_eq!(clean.lane.cleanup_status.as_deref(), Some("done"));
    let orphan = closed.tasks.iter().find(|task| task.id == orphan_task).unwrap();
    assert_eq!(orphan.lane.worktree_path, None);
    assert_eq!(orphan.lane.branch, None);
    assert_eq!(orphan.lane.cleanup_status.as_deref(), Some("done"));
    let work = closed
        .tasks
        .iter()
        .find(|task| task.id == work_task)
        .unwrap();
    assert_eq!(work.status, crate::plan::TaskStatus::Pending);
    assert_eq!(
        work.lane.worktree_path.as_deref(),
        Some(work_lane.to_str().unwrap())
    );
    assert_eq!(work.lane.branch.as_deref(), Some("lane/work"));
    assert_eq!(work.lane.cleanup_status.as_deref(), Some("cleanup_debt"));
    assert!(work
        .lane
        .cleanup_hints
        .iter()
        .any(|hint| {
            hint.contains(work_lane.to_str().unwrap())
                && hint.contains("lane/work")
                && hint.contains("unmerged commits ahead of destination main")
        }));
    let dirty = closed.tasks.iter().find(|task| task.id == dirty_task).unwrap();
    assert_eq!(dirty.lane.cleanup_status.as_deref(), Some("cleanup_debt"));
    assert!(dirty.lane.cleanup_hints.iter().any(|hint| {
        hint.contains("dirty worktree") && hint.contains(dirty_lane.to_str().unwrap())
    }));
    let retained = closed
        .tasks
        .iter()
        .find(|task| task.id == retained_task)
        .unwrap();
    assert_eq!(
        retained.lane.cleanup_status.as_deref(),
        Some("cleanup_debt")
    );
    assert_eq!(
        retained.lane.worktree_path.as_deref(),
        Some(retained_lane.to_str().unwrap())
    );
    assert!(retained
        .lane
        .cleanup_hints
        .iter()
        .any(|hint| hint.contains("outside the lane/ namespace")));
}

#[test]
fn team_close_inventory_includes_terminal_unplanned_child_and_worktree_less_lanes() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let (_, team_id, orchestrator, _, _) = team_members();
    let plan = store
        .create_plan_for_team(
            "Terminal lane inventory".into(),
            dir.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&orchestrator),
            team_id.clone(),
            vec![],
            1,
        )
        .unwrap();
    let parent_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "unplanned terminal task".into(),
                kind: "unplanned".into(),
                acceptance_criteria: vec!["completed".into()],
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let child_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "child lane".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["completed".into()],
                parent_task_id: Some(parent_id.clone()),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let mut plan = store.get(&plan.id).unwrap();
    let parent = plan.tasks.iter_mut().find(|task| task.id == parent_id).unwrap();
    parent.status = crate::plan::TaskStatus::Done;
    parent.lane = crate::plan::TaskLaneMetadata {
        branch: Some("lane/unplanned-terminal".into()),
        base: Some("fork-point".into()),
        destination: Some("main".into()),
        ..Default::default()
    };
    let child = plan.tasks.iter_mut().find(|task| task.id == child_id).unwrap();
    child.lane = crate::plan::TaskLaneMetadata {
        branch: Some("lane/child".into()),
        base: Some("fork-point".into()),
        destination: Some("main".into()),
        ..Default::default()
    };
    store.save(&mut plan, 4).unwrap();

    let lanes = team_close_lanes(&store.list_full(), &team_id, &[]);

    assert_eq!(lanes.len(), 2);
    assert!(lanes.iter().any(|lane| {
        lane.task_id == parent_id
            && lane.branch.as_deref() == Some("lane/unplanned-terminal")
            && lane.worktree_path.is_none()
    }));
    assert!(lanes.iter().any(|lane| {
        lane.task_id == child_id
            && lane.branch.as_deref() == Some("lane/child")
            && lane.worktree_path.is_none()
    }));
}

#[tokio::test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
async fn service_close_team_preserves_untracked_lane_after_pty_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init"]);
    git(&repo, &["config", "user.name", "CodeTerm Tests"]);
    git(&repo, &["config", "user.email", "tests@codeterm.invalid"]);
    fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(&repo, &["add", "base.txt"]);
    git(&repo, &["commit", "-m", "base"]);
    git(&repo, &["branch", "-M", "main"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let worktree = dir.path().join("untracked-lane");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "lane/untracked",
            worktree.to_str().unwrap(),
            &base,
        ],
    );
    let untracked = worktree.join("keep.txt");
    fs::write(&untracked, "keep this work\n").unwrap();

    let (mux, team_id, orchestrator, _, worker) = team_members();
    let fixture = ServiceFixture::new(mux);
    let store = &fixture.service.plan_store;
    let plan = store
        .create_plan_for_team(
            "Service lane close".into(),
            repo.display().to_string(),
            crate::plan::OrchestratorRef::from_tab(&orchestrator),
            team_id.clone(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = add_team_lane_task(
        store,
        &plan.id,
        "untracked work",
        &worker,
        &worktree,
        "lane/untracked",
        &base,
        2,
    );

    fixture
        .service
        .close_team(&team_id, TeamMutationCaller::AgentTab(orchestrator))
        .await
        .unwrap();

    assert_eq!(fs::read_to_string(&untracked).unwrap(), "keep this work\n");
    let worktrees = git(&repo, &["worktree", "list", "--porcelain"]).replace('\\', "/");
    let worktree_path = worktree.display().to_string().replace('\\', "/");
    assert!(worktrees.contains(&worktree_path));
    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.lane.cleanup_status.as_deref(), Some("cleanup_debt"));
    assert_eq!(
        task.lane.worktree_path.as_deref(),
        Some(worktree.to_str().unwrap())
    );
    assert!(task
        .lane
        .cleanup_hints
        .iter()
        .any(|hint| { hint.contains("keep.txt") && hint.contains("lane/untracked") }));
}

#[tokio::test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
async fn team_close_deletes_team_bound_recurring_series() {
    let (mut mux, team_id, orchestrator, _, _) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let independent_tab = mux.create_tab(&workspace_id).unwrap();
    let fixture = ServiceFixture::new(mux);
    let rule = crate::delayed_send::recurrence::RecurrenceRule {
        every_ms: 60_000,
        until_ms: None,
        count: Some(3),
    };
    for (tab_id, team) in [
        (orchestrator.as_str(), Some(team_id.as_str())),
        (independent_tab.as_str(), None),
    ] {
        fixture
            .service
            .delayed_send
            .enqueue_with_recurrence(
                tab_id.into(),
                None,
                None,
                "tick".into(),
                Some(crate::delayed_send::now_ms() + 600_000),
                crate::delayed_send::ReminderCreator::default(),
                Some(rule.clone()),
                team.map(str::to_string),
            )
            .unwrap();
    }

    fixture
        .service
        .close_team(&team_id, TeamMutationCaller::Human)
        .await
        .unwrap();

    let remaining = fixture.service.delayed_send.list(None);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].tab_id, independent_tab);
    assert_eq!(remaining[0].team_id, None);
}

fn add_team_lane_task(
    store: &crate::service::plan::PlanStore,
    plan_id: &str,
    name: &str,
    tab_id: &str,
    worktree_path: &Path,
    branch: &str,
    base: &str,
    now_ms: u64,
) -> String {
    let task_id = store
        .add_task(
            plan_id,
            NewTask {
                name: name.into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["finished".into()],
                ..Default::default()
            },
            now_ms,
        )
        .unwrap();
    let mut plan = store.get(plan_id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = crate::plan::TaskStatus::InProgress;
    task.assigned_tab_id = Some(tab_id.into());
    task.lane = crate::plan::TaskLaneMetadata {
        tab_id: Some(tab_id.into()),
        worktree_path: Some(worktree_path.display().to_string()),
        branch: Some(branch.into()),
        base: Some(base.into()),
        destination: Some("main".into()),
        ..Default::default()
    };
    task.execution_attempts
        .push(crate::plan::TaskExecutionAttempt {
            actual_reasoning_effort: None,
            id: format!("attempt_{task_id}"),
            executor_tab_id: tab_id.into(),
            worktree_path: Some(worktree_path.display().to_string()),
            branch: Some(branch.into()),
            base: Some(base.into()),
            destination: Some("main".into()),
            ..Default::default()
        });
    store.save(&mut plan, now_ms + 1).unwrap();
    task_id
}

fn git_command(cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(cwd).args(args);
    crate::process_util::no_window_std(&mut command);
    command
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = git_command(cwd, args).output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git_status_success(cwd: &Path, args: &[&str]) -> bool {
    git_command(cwd, args).status().unwrap().success()
}
